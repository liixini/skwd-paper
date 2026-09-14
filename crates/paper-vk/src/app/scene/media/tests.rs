use super::*;
use dbus::arg::Variant;

#[test]
fn metadata_uses_album_artist_fallback_and_seconds() {
    let mut metadata = PropMap::new();
    metadata.insert("xesam:title".into(), Variant(Box::new("Test track".to_owned())));
    metadata.insert("xesam:artist".into(), Variant(Box::new(vec!["Artist".to_owned()])));
    metadata.insert("mpris:length".into(), Variant(Box::new(120_000_000i64)));
    let mut properties = PropMap::new();
    properties.insert("PlaybackStatus".into(), Variant(Box::new("Playing".to_owned())));
    properties.insert("Position".into(), Variant(Box::new(3_000_000i64)));
    properties.insert("Metadata".into(), Variant(Box::new(metadata)));
    let value = events(&properties);
    assert_eq!(value["mediaPropertiesChanged"]["albumArtist"], "Artist");
    assert_eq!(value["mediaPropertiesChanged"]["title"], "Test track");
    assert_eq!(value["mediaPlaybackChanged"]["state"], 1);
    assert_eq!(value["mediaTimelineChanged"]["position"], 3.0);
    assert_eq!(value["mediaTimelineChanged"]["duration"], 120.0);
    assert_eq!(events(&PropMap::new())["mediaPlaybackChanged"]["state"], 0);
}
