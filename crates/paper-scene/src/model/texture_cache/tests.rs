use super::{Key, TextureCache};
use crate::model::solid_texture;
use std::path::PathBuf;
use std::sync::Arc;

fn package(name: &str) -> Key {
    Key::Package(name.to_string(), None)
}

#[test]
fn shared_charged_once() {
    let mut cache = TextureCache::default();
    let texture = solid_texture();
    cache.insert(package("solid"), &texture);
    let copy = cache.get(&package("solid")).unwrap();
    assert_eq!(cache.charge(&texture), 4);
    assert_eq!(cache.charge(&copy), 0);
}

#[test]
fn uncached_charged_fully() {
    let mut cache = TextureCache::default();
    let texture = solid_texture();
    assert_eq!(cache.charge(&texture), 4);
    assert_eq!(cache.charge(&texture), 4);
}

#[test]
fn shared_pages_charged_once() {
    let mut cache = TextureCache::default();
    let mut texture = solid_texture();
    texture.pages.push(Arc::new(crate::tex::Pixels::rgba(2, 1, vec![0; 8])));
    cache.insert(package("sheet"), &texture);
    let copy = cache.get(&package("sheet")).unwrap();
    assert!(Arc::ptr_eq(&texture.pages[0], &copy.pages[0]));
    assert_eq!(cache.charge(&texture), 12);
    assert_eq!(cache.charge(&copy), 0);
}

#[test]
fn shared_video_revived() {
    let mut cache = TextureCache::default();
    let mut texture = solid_texture();
    let payload: Arc<[u8]> = Arc::from(vec![1_u8; 16]);
    texture.video = Some(crate::model::Video::Bytes(payload.clone()));
    cache.insert(package("clip"), &texture);
    let copy = cache.get(&package("clip")).unwrap();
    let Some(crate::model::Video::Bytes(revived)) = copy.video else { panic!("revived video") };
    assert!(Arc::ptr_eq(&payload, &revived));
}

#[test]
fn package_range_revived() {
    let mut cache = TextureCache::default();
    let mut texture = solid_texture();
    let file = Arc::new(tempfile::tempfile().unwrap());
    let range = crate::model::VideoRange { file: file.clone(), offset: 4, length: 8 };
    texture.video = Some(crate::model::Video::Range(range));
    cache.insert(package("clip"), &texture);
    let copy = cache.get(&package("clip")).unwrap();
    let Some(crate::model::Video::Range(revived)) = copy.video else { panic!("revived range") };
    assert!(Arc::ptr_eq(&file, &revived.file));
    assert_eq!((revived.offset, revived.length), (4, 8));
}

#[test]
fn dropped_not_retained() {
    let mut cache = TextureCache::default();
    let texture = solid_texture();
    cache.insert(package("solid"), &texture);
    drop(texture);
    assert!(cache.get(&package("solid")).is_none());
}

#[test]
fn dropped_page_misses() {
    let mut cache = TextureCache::default();
    let mut texture = solid_texture();
    texture.pages.push(Arc::new(crate::tex::Pixels::rgba(1, 1, vec![0; 4])));
    cache.insert(package("sheet"), &texture);
    let _pixels = texture.pixels.clone();
    drop(texture);
    assert!(cache.get(&package("sheet")).is_none());
}

#[test]
fn keyed_by_source() {
    let mut cache = TextureCache::default();
    let texture = solid_texture();
    cache.insert(Key::Asset(PathBuf::from("/assets/materials/noise.tex")), &texture);
    assert!(cache.get(&package("materials/noise.tex")).is_none());
    assert!(cache.get(&Key::Asset(PathBuf::from("/assets/materials/noise.tex"))).is_some());
}
