use super::{Texture, Video};
use crate::tex::Pixels;
use crate::tex::stream::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

#[derive(PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Package(String, Option<u32>),
    Asset(PathBuf),
}

struct Entry {
    pixels: Weak<Pixels>,
    pages: Vec<Weak<Pixels>>,
    video: Option<Weak<[u8]>>,
    stream: Option<Weak<[Cell]>>,
    template: Texture,
}

#[derive(Default)]
pub(crate) struct TextureCache {
    decoded: HashMap<Key, Entry>,
    charged: HashMap<usize, (Weak<Pixels>, bool)>,
}

fn address(pixels: &Arc<Pixels>) -> usize {
    Arc::as_ptr(pixels) as usize
}

impl Entry {
    fn of(texture: &Texture) -> Self {
        let mut template = texture.clone();
        template.pixels = Arc::default();
        template.pages = Vec::new();
        if matches!(template.video, Some(Video::Bytes(_))) {
            template.video = None;
        }
        template.stream = None;
        Self {
            pixels: Arc::downgrade(&texture.pixels),
            pages: texture.pages.iter().map(Arc::downgrade).collect(),
            video: match &texture.video {
                Some(Video::Bytes(payload)) => Some(Arc::downgrade(payload)),
                _ => None,
            },
            stream: texture.stream.as_ref().map(Arc::downgrade),
            template,
        }
    }

    fn revive(&self) -> Option<Texture> {
        let mut texture = self.template.clone();
        texture.pixels = self.pixels.upgrade()?;
        texture.pages = self.pages.iter().map(Weak::upgrade).collect::<Option<_>>()?;
        if let Some(video) = &self.video {
            texture.video = Some(Video::Bytes(video.upgrade()?));
        }
        texture.stream = match &self.stream {
            Some(stream) => Some(stream.upgrade()?),
            None => None,
        };
        Some(texture)
    }
}

impl TextureCache {
    pub(crate) fn get(&self, key: &Key) -> Option<Texture> {
        self.decoded.get(key)?.revive()
    }

    pub(crate) fn insert(&mut self, key: Key, texture: &Texture) {
        self.charged.insert(address(&texture.pixels), (Arc::downgrade(&texture.pixels), false));
        self.decoded.insert(key, Entry::of(texture));
    }

    pub(crate) fn charge(&mut self, texture: &Texture) -> usize {
        match self.charged.get_mut(&address(&texture.pixels)) {
            Some((_, true)) => 0,
            Some((_, charged)) => {
                *charged = true;
                texture.payload_bytes()
            }
            None => texture.payload_bytes(),
        }
    }
}

#[cfg(test)]
mod tests;
