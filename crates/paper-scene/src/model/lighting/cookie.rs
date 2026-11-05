use super::{CAPACITY, Light};
use crate::{effects::Assets, model::load_texture_or_asset, pkg::Package, tex};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

const FALLBACK: &str = "cookie/flashlight1";
const UNIFORM: &str = "g_LSpot_Cookie";
const MAX_EDGE: u32 = 4096;
const MAX_BYTES: usize = 64 << 20;
const GUTTER: u32 = 1;
const CHANNELS: usize = 4;
const COOKIE_TOO_LARGE: &str = "cookie texture too large";
const ATLAS_TOO_TALL: &str = "cookie atlas too tall";
const ATLAS_OVER_BUDGET: &str = "cookie atlas exceeds texture budget";
const ANIMATED_COOKIE: &str = "animated spot cookie unsupported";
const INVALID_PIXELS: &str = "spot cookie pixels invalid";

pub struct Cookies {
    pub texture: super::super::Texture,
    pub layout: CookieLayout,
}

#[derive(Clone, Default)]
pub struct CookieLayout {
    uv: BTreeMap<String, [f32; CHANNELS]>,
    packed: bool,
}

impl CookieLayout {
    pub fn packed(&self) -> bool {
        self.packed
    }

    pub fn uniforms(&self, lights: &[Light]) -> BTreeMap<String, Vec<f32>> {
        let slots = lights.iter().filter(|light| light.cone.is_some()).take(CAPACITY).count();
        let mut values = vec![0.0; slots * CHANNELS];
        let selected = lights
            .iter()
            .rev()
            .find(|light| light.visible && light.cone.is_some() && light.cookie.is_some())
            .and_then(|light| self.uv.get(&light.id));
        for (index, light) in lights
            .iter()
            .filter(|light| light.cone.is_some() && light.visible)
            .take(CAPACITY)
            .enumerate()
        {
            if light.cookie.is_some()
                && let Some(uv) = selected
            {
                values[index * CHANNELS..(index + 1) * CHANNELS].copy_from_slice(uv);
            }
        }
        BTreeMap::from([(UNIFORM.into(), values)])
    }
}

struct Tile {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    x: u32,
    y: u32,
}

fn pack(tiles: &mut [Tile]) -> Result<(u32, u32)> {
    let mut x = 0_u32;
    let mut y = 0_u32;
    let mut row_height = 0_u32;
    let mut width = 0_u32;
    for tile in tiles {
        let cell_width = tile.width.checked_add(GUTTER * 2).context("cookie width overflow")?;
        let cell_height = tile.height.checked_add(GUTTER * 2).context("cookie height overflow")?;
        ensure!(cell_width <= MAX_EDGE && cell_height <= MAX_EDGE, COOKIE_TOO_LARGE);
        if x + cell_width > MAX_EDGE {
            x = 0;
            y = y.checked_add(row_height).context("cookie atlas height overflow")?;
            row_height = 0;
        }
        ensure!(y + cell_height <= MAX_EDGE, ATLAS_TOO_TALL);
        tile.x = x;
        tile.y = y;
        x += cell_width;
        width = width.max(x);
        row_height = row_height.max(cell_height);
    }
    let height = y + row_height;
    let bytes = tex::PixelFormat::Rgba8
        .level_bytes(width, height)
        .context("cookie atlas dimensions invalid")?;
    ensure!(bytes <= MAX_BYTES, ATLAS_OVER_BUDGET);
    Ok((width, height))
}

fn raster(tiles: &[Tile], width: u32, height: u32) -> Result<Vec<u8>> {
    let bytes = tex::PixelFormat::Rgba8
        .level_bytes(width, height)
        .context("cookie atlas dimensions invalid")?;
    let mut atlas = vec![0; bytes];
    for tile in tiles {
        for row in 0..tile.height + GUTTER * 2 {
            let source_y = row.saturating_sub(GUTTER).min(tile.height - 1);
            for column in 0..tile.width + GUTTER * 2 {
                let source_x = column.saturating_sub(GUTTER).min(tile.width - 1);
                let source = ((source_y * tile.width + source_x) as usize) * CHANNELS;
                let target = (((tile.y + row) * width + tile.x + column) as usize) * CHANNELS;
                atlas[target..target + CHANNELS]
                    .copy_from_slice(&tile.pixels[source..source + CHANNELS]);
            }
        }
    }
    Ok(atlas)
}

pub fn load(pkg: &Package, assets: &Assets, lights: &[Light]) -> Result<Option<Cookies>> {
    let mut textures = Vec::new();
    let mut decoded_bytes = 0_usize;
    let mut by_path = BTreeMap::new();
    let mut by_id = Vec::new();
    for light in lights.iter().filter(|light| light.cone.is_some()).take(super::MAX_LIGHTS) {
        let Some(path) = light.cookie.as_deref() else { continue };
        let (name, texture) = if let Some(index) = by_path.get(path) {
            by_id.push((light.id.clone(), *index));
            continue;
        } else if let Some(texture) =
            load_texture_or_asset(pkg, assets, path).filter(|texture| texture.stream.is_none())
        {
            (path, texture)
        } else if let Some(index) = by_path.get(FALLBACK) {
            by_id.push((light.id.clone(), *index));
            continue;
        } else {
            (
                FALLBACK,
                load_texture_or_asset(pkg, assets, FALLBACK)
                    .context("spot cookie and fallback unavailable")?,
            )
        };
        ensure!(texture.video.is_none() && texture.frames.is_empty(), ANIMATED_COOKIE);
        ensure!(texture.width > 0 && texture.height > 0, INVALID_PIXELS);
        ensure!(texture.width <= MAX_EDGE && texture.height <= MAX_EDGE, COOKIE_TOO_LARGE);
        let bytes = tex::PixelFormat::Rgba8
            .level_bytes(texture.width, texture.height)
            .context("spot cookie dimensions invalid")?;
        decoded_bytes = decoded_bytes.checked_add(bytes).context(ATLAS_OVER_BUDGET)?;
        ensure!(decoded_bytes <= MAX_BYTES, ATLAS_OVER_BUDGET);
        let index = textures.len();
        textures.push(texture);
        by_path.insert(name.to_owned(), index);
        by_id.push((light.id.clone(), index));
    }
    if textures.is_empty() {
        return Ok(None);
    }
    if textures.len() == 1 {
        let texture = textures.pop().unwrap();
        let uv = by_id.into_iter().map(|(id, _)| (id, [0.0, 0.0, 1.0, 1.0])).collect();
        return Ok(Some(Cookies { texture, layout: CookieLayout { uv, packed: false } }));
    }
    let mut tiles = textures
        .into_iter()
        .map(|texture| {
            let pixels = texture.pixels.base_rgba().context("spot cookie pixels unavailable")?;
            let expected = tex::PixelFormat::Rgba8
                .level_bytes(texture.width, texture.height)
                .context("spot cookie dimensions invalid")?;
            ensure!(pixels.len() == expected, INVALID_PIXELS);
            Ok(Tile { width: texture.width, height: texture.height, pixels, x: 0, y: 0 })
        })
        .collect::<Result<Vec<_>>>()?;
    let (width, height) = pack(&mut tiles)?;
    let pixels = raster(&tiles, width, height)?;
    let uv = by_id
        .into_iter()
        .map(|(id, index)| {
            let tile = &tiles[index];
            (
                id,
                [
                    (tile.x + GUTTER) as f32 / width as f32,
                    (tile.y + GUTTER) as f32 / height as f32,
                    tile.width as f32 / width as f32,
                    tile.height as f32 / height as f32,
                ],
            )
        })
        .collect();
    let mut texture = super::super::solid_texture();
    texture.width = width;
    texture.height = height;
    texture.img_width = width;
    texture.img_height = height;
    texture.pixels = std::sync::Arc::new(tex::Pixels::rgba(width, height, pixels));
    Ok(Some(Cookies { texture, layout: CookieLayout { uv, packed: true } }))
}

#[cfg(test)]
#[path = "cookie_tests.rs"]
mod tests;
