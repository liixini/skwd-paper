use crate::scene::SceneFeatures;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeGap {
    EventDrivenSounds(usize),
    LightObjects(usize),
    TextObjects(usize),
    OtherObjects(usize),
    Puppet,
    AnimatedImageTextures(usize),
    AudioReactive,
    VideoTextures(usize),
    UnknownTextureFormats(Vec<String>),
    TextureParseFailures(usize),
    JsonParseFailures(usize),
    ReferenceScanTruncated,
}

impl NativeGap {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EventDrivenSounds(_) => "event-driven-sounds",
            Self::LightObjects(_) => "light-objects",
            Self::TextObjects(_) => "text-objects",
            Self::OtherObjects(_) => "other-objects",
            Self::Puppet => "puppet",
            Self::AnimatedImageTextures(_) => "animated-image-textures",
            Self::AudioReactive => "audio-reactive",
            Self::VideoTextures(_) => "video-textures",
            Self::UnknownTextureFormats(_) => "unknown-texture-formats",
            Self::TextureParseFailures(_) => "texture-parse-failures",
            Self::JsonParseFailures(_) => "json-parse-failures",
            Self::ReferenceScanTruncated => "reference-scan-truncated",
        }
    }

    pub fn instances(&self) -> usize {
        match self {
            Self::EventDrivenSounds(count)
            | Self::LightObjects(count)
            | Self::TextObjects(count)
            | Self::OtherObjects(count)
            | Self::AnimatedImageTextures(count)
            | Self::VideoTextures(count)
            | Self::TextureParseFailures(count)
            | Self::JsonParseFailures(count) => *count,
            Self::UnknownTextureFormats(formats) => formats.len(),
            Self::Puppet | Self::AudioReactive | Self::ReferenceScanTruncated => 1,
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::EventDrivenSounds(count) => {
                format!("{count} sound object(s) that only a scene event can start")
            }
            Self::LightObjects(count) => format!("{count} light object(s)"),
            Self::TextObjects(count) => format!("{count} text object(s)"),
            Self::OtherObjects(count) => format!("{count} unrecognised object(s)"),
            Self::Puppet => "unsupported puppet deformation format".to_string(),
            Self::AnimatedImageTextures(count) => {
                format!("{count} animated image texture layer(s)")
            }
            Self::AudioReactive => "audio-reactive scene data".to_string(),
            Self::VideoTextures(count) => format!("{count} video texture(s)"),
            Self::UnknownTextureFormats(formats) => {
                format!("unknown texture format(s): {}", formats.join(", "))
            }
            Self::TextureParseFailures(count) => {
                format!("{count} texture metadata parse failure(s)")
            }
            Self::JsonParseFailures(count) => format!("{count} JSON parse failure(s)"),
            Self::ReferenceScanTruncated => "scene reference scan exceeded its safety limit".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeCompatibility {
    pub gaps: Vec<NativeGap>,
}

impl NativeCompatibility {
    pub fn full_fidelity(&self) -> bool {
        self.gaps.is_empty()
    }

    pub fn reason_codes(&self) -> Vec<&'static str> {
        self.gaps.iter().map(NativeGap::code).collect()
    }
}

/// Merely suggestive incidence, notably the ubiquitous `parallax` key, is not a gap.
pub fn assess_native(features: &SceneFeatures) -> NativeCompatibility {
    let mut gaps = Vec::new();
    if features.objects_sound_event > 0 {
        gaps.push(NativeGap::EventDrivenSounds(features.objects_sound_event));
    }
    if features.objects_light > 0 {
        gaps.push(NativeGap::LightObjects(features.objects_light));
    }
    if features.objects_text > 0 {
        gaps.push(NativeGap::TextObjects(features.objects_text));
    }
    if features.objects_other > 0 {
        gaps.push(NativeGap::OtherObjects(features.objects_other));
    }
    if features.puppet_unsupported {
        gaps.push(NativeGap::Puppet);
    }
    if !features.animated_image_textures.is_empty() {
        gaps.push(NativeGap::AnimatedImageTextures(features.animated_image_textures.len()));
    }
    if features.audio {
        gaps.push(NativeGap::AudioReactive);
    }
    if features.tex_video > 0 {
        gaps.push(NativeGap::VideoTextures(features.tex_video));
    }
    if !features.tex_other_format.is_empty() {
        gaps.push(NativeGap::UnknownTextureFormats(
            features.tex_other_format.iter().cloned().collect(),
        ));
    }
    if features.tex_failures > 0 {
        gaps.push(NativeGap::TextureParseFailures(features.tex_failures));
    }
    if features.json_failures > 0 {
        gaps.push(NativeGap::JsonParseFailures(features.json_failures));
    }
    if features.refs_truncated {
        gaps.push(NativeGap::ReferenceScanTruncated);
    }
    NativeCompatibility { gaps }
}

#[cfg(test)]
mod tests;
