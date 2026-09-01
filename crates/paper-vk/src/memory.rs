#[cfg(target_os = "linux")]
mod imp {
    use std::sync::Once;
    use std::time::Duration;

    static SCHEDULED: Once = Once::new();

    pub(crate) fn schedule_cold_start_reclaim() {
        SCHEDULED.call_once(|| {
            if std::env::var("SKWD_VK_RECLAIM_DRIVER").as_deref() == Ok("0") {
                return;
            }
            let _ = std::thread::Builder::new().name("rss-reclaim".into()).spawn(|| {
                std::thread::sleep(Duration::from_secs(3));
                reclaim_cold_start_pages();
            });
        });
    }

    fn reclaim_cold_start_pages() {
        let Ok(smaps) = std::fs::read_to_string("/proc/self/smaps") else {
            return;
        };
        let mut reclaimed = 0usize;
        let mut mappings = 0usize;
        for (start, len) in clean_nvidia_compiler_mappings(&smaps) {
            // Clean file-backed pages fault back on demand; dirty or anonymous ones would not.
            if unsafe { libc::madvise(start.cast(), len, libc::MADV_DONTNEED) } == 0 {
                reclaimed = reclaimed.saturating_add(len);
                mappings += 1;
            }
        }
        unsafe {
            libc::malloc_trim(0);
        }
        if mappings > 0 {
            tracing::info!(
                mappings,
                virtual_mib = reclaimed / (1024 * 1024),
                "skwd-wall-vk: released clean NVIDIA compiler pages"
            );
        }
    }

    #[derive(Clone, Copy)]
    struct Mapping {
        start: *mut libc::c_void,
        len: usize,
        clean: bool,
    }

    fn mapping_header(line: &str) -> Option<Mapping> {
        let mut fields = line.split_whitespace();
        let range = fields.next()?;
        let permissions = fields.next()?;
        if !permissions.starts_with('r') || permissions.as_bytes().get(1) == Some(&b'w') {
            return None;
        }
        if !line.contains("libnvidia-gpucomp.so") {
            return None;
        }
        let (start, end) = range.split_once('-')?;
        let start = usize::from_str_radix(start, 16).ok()?;
        let end = usize::from_str_radix(end, 16).ok()?;
        (end > start).then_some(Mapping {
            start: start as *mut libc::c_void,
            len: end - start,
            clean: true,
        })
    }

    fn clean_nvidia_compiler_mappings(smaps: &str) -> Vec<(*mut libc::c_void, usize)> {
        fn finish(mapping: Option<Mapping>, out: &mut Vec<(*mut libc::c_void, usize)>) {
            if let Some(mapping) = mapping.filter(|mapping| mapping.clean) {
                out.push((mapping.start, mapping.len));
            }
        }

        let mut out = Vec::new();
        let mut current = None;
        for line in smaps.lines() {
            if line.split_whitespace().next().is_some_and(|field| field.contains('-')) {
                finish(current.take(), &mut out);
                current = mapping_header(line);
                continue;
            }
            let Some(mapping) = current.as_mut() else {
                continue;
            };
            if matches!(line.split_once(':'), Some(("Anonymous" | "Private_Dirty" | "Shared_Dirty", value)) if value.split_whitespace().next() != Some("0"))
            {
                mapping.clean = false;
            }
        }
        finish(current, &mut out);
        out
    }

    #[cfg(test)]
    mod tests;
}

#[cfg(target_os = "linux")]
pub(crate) use imp::schedule_cold_start_reclaim;

#[cfg(not(target_os = "linux"))]
pub(crate) fn schedule_cold_start_reclaim() {}
