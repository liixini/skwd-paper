use std::ops::Range;

use paper_text::{Font, script_runs};
use swash::shape::cluster::Glyph as ShapedGlyph;
use swash::text::Codepoint;
use swash::text::cluster::{CharCluster, Parser, Status, Token};

const ASCII_SPACE: u8 = b' ';

impl super::Face {
    pub(super) fn load_fallbacks(&mut self, assets: &super::Assets, text: &str) {
        if self.fallback_loaded {
            return;
        }
        let charmap = self.font.face().charmap();
        if text.chars().all(|ch| charmap.map(ch) != 0 || ch.is_whitespace()) {
            return;
        }
        let bytes = assets
            .system_fonts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fallbacks(assets);
        self.fallbacks =
            bytes.into_iter().filter_map(|bytes| Font::parse(bytes, self.font.em())).collect();
        self.fallback_loaded = true;
    }

    fn runs(&self, text: &str) -> Vec<(Range<usize>, usize)> {
        let mut runs: Vec<(Range<usize>, usize)> = Vec::new();
        let mut cluster = CharCluster::new();
        for (range, script) in script_runs(text) {
            let start = range.start;
            let tokens = text[range].char_indices().map(|(index, ch)| Token {
                ch,
                offset: (start + index) as u32,
                len: ch.len_utf8() as u8,
                info: ch.properties().into(),
                data: 0,
            });
            let mut parser = Parser::new(script, tokens);
            while parser.next(&mut cluster) {
                let selected = std::iter::once(&self.font)
                    .chain(self.fallbacks.iter())
                    .position(|font| {
                        let charmap = font.face().charmap();
                        cluster.map(|ch| charmap.map(ch)) == Status::Complete
                    })
                    .unwrap_or(0);
                let range = cluster.range().to_range();
                if let Some((previous, font)) = runs.last_mut()
                    && *font == selected
                    && previous.end == range.start
                {
                    previous.end = range.end;
                } else {
                    runs.push((range, selected));
                }
            }
        }
        for index in 1..runs.len().saturating_sub(1) {
            if runs[index].1 == 0
                && runs[index - 1].1 != 0
                && runs[index - 1].1 == runs[index + 1].1
                && text[runs[index].0.clone()].bytes().all(|byte| byte == ASCII_SPACE)
                && self.fallbacks[runs[index - 1].1 - 1]
                    .face()
                    .charmap()
                    .map(char::from(ASCII_SPACE))
                    != 0
            {
                runs[index].1 = runs[index - 1].1;
            }
        }
        runs.dedup_by(|next, previous| {
            if next.1 == previous.1 && previous.0.end == next.0.start {
                previous.0.end = next.0.end;
                true
            } else {
                false
            }
        });
        runs
    }

    #[cfg(test)]
    pub(super) fn uses_fallback(&self, text: &str) -> bool {
        !self.fallbacks.is_empty() && self.runs(text).iter().any(|(_, font)| *font != 0)
    }

    pub(super) fn shape(&self, text: &str, mut visit: impl FnMut(&Font, ShapedGlyph)) {
        if self.fallbacks.is_empty() {
            self.font.shape(text, |glyph| visit(&self.font, glyph));
            return;
        }
        let runs = self.runs(text);
        if runs.iter().all(|(_, font)| *font == 0) {
            self.font.shape(text, |glyph| visit(&self.font, glyph));
            return;
        }
        for (range, selected) in runs {
            let font = if selected == 0 { &self.font } else { &self.fallbacks[selected - 1] };
            font.shape(&text[range], |mut glyph| {
                glyph.advance = glyph.advance.floor();
                visit(font, glyph);
            });
        }
    }
}
