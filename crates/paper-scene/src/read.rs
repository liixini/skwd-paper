use anyhow::{Result, anyhow};

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(len).ok_or_else(|| anyhow!("length overflow"))?;
        if end > self.data.len() {
            return Err(anyhow!("truncated: need {len} bytes at {}", self.pos));
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn skip(&mut self, len: usize) -> Result<()> {
        self.take(len).map(|_| ())
    }

    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn len_string(&mut self, max: usize) -> Result<String> {
        let len = self.u32()? as usize;
        if len > max {
            return Err(anyhow!("string length {len} exceeds cap {max}"));
        }
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    pub(crate) fn nul_string(&mut self, max: usize) -> Result<String> {
        let start = self.pos;
        let cap = (start + max).min(self.data.len());
        let end = self.data[start..cap]
            .iter()
            .position(|&byte| byte == 0)
            .map(|idx| start + idx)
            .ok_or_else(|| anyhow!("unterminated string at {start}"))?;
        let text = String::from_utf8_lossy(&self.data[start..end]).into_owned();
        self.pos = end + 1;
        Ok(text)
    }
}
