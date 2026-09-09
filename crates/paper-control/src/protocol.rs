use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use paper_geom::FillMode;

pub const PROTOCOL_NAME: &str = "skwd-paper";
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Static,
    Video,
    #[serde(rename = "we")]
    WallpaperEngine,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoEngine {
    #[default]
    #[serde(alias = "regular", alias = "tiny")]
    Default,
    Tinier,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    #[default]
    Background,
    Bottom,
    Top,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandQuality {
    Auto,
    Full,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandScope {
    All,
    Primary,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl TransitionPolicy {
    pub fn effect(&self) -> &str {
        self.effect.as_deref().unwrap_or("fade")
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms.unwrap_or(600)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Some(from) = &self.from {
            if from.trim().is_empty() {
                return Err(ValidationError::EmptyTransitionFrom);
            }
            if from.starts_with('-') {
                return Err(ValidationError::TransitionFromStartsWithDash);
            }
        }
        if let Some(effect) = &self.effect
            && (effect.is_empty()
                || effect.starts_with('-')
                || !effect
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
        {
            return Err(ValidationError::InvalidTransitionEffect(effect.clone()));
        }
        let duration_ms = self.duration_ms();
        if !(50..=10_000).contains(&duration_ms) {
            return Err(ValidationError::TransitionDurationOutOfRange(duration_ms));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<SandQuality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<SandScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_particles: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_dimension: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_effect_chains: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_effect_passes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfacePolicy {
    pub namespace: String,
    #[serde(default)]
    pub blur: u32,
    #[serde(default)]
    pub dim: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Box<SurfacePolicy>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transitions_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sand: Option<SandPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<ScenePolicy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub output_fps: BTreeMap<String, u32>,
}

impl RendererPolicy {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.surface.as_ref().is_some_and(|surface| {
            surface.namespace.trim().is_empty()
                || surface.namespace.len() > 128
                || surface.namespace.chars().any(char::is_control)
                || surface.blur > 100
                || surface.dim > 100
        }) {
            return Err(ValidationError::InvalidSurfacePolicy);
        }
        if let Some(fps) = self.sand.as_ref().and_then(|sand| sand.fps)
            && !(1..=1000).contains(&fps)
        {
            return Err(ValidationError::SandFpsOutOfRange(fps));
        }
        if let Some(scene) = &self.scene {
            if scene.fps == Some(0) || scene.fps.is_some_and(|fps| fps > 240) {
                return Err(ValidationError::SceneFpsOutOfRange(scene.fps.unwrap_or_default()));
            }
            if scene.assets_dir.as_ref().is_some_and(|path| path.trim().is_empty()) {
                return Err(ValidationError::EmptySceneAssetsDirectory);
            }
            if let Some(dimension) = scene.max_dimension
                && !(256..=16_384).contains(&dimension)
            {
                return Err(ValidationError::SceneDimensionOutOfRange(dimension));
            }
            if let Some(chains) = scene.max_effect_chains
                && !(1..=64).contains(&chains)
            {
                return Err(ValidationError::SceneEffectChainsOutOfRange(chains));
            }
            if let Some(passes) = scene.max_effect_passes
                && !(1..=64).contains(&passes)
            {
                return Err(ValidationError::SceneEffectPassesOutOfRange(passes));
            }
        }
        for (output, fps) in &self.output_fps {
            if output.trim().is_empty() || output.starts_with('-') || output.contains([';', '=']) {
                return Err(ValidationError::InvalidPolicyOutput(output.clone()));
            }
            if *fps > 1000 {
                return Err(ValidationError::OutputFpsOutOfRange(output.clone(), *fps));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub kind: SourceKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<VideoEngine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_rate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

pub const MAX_SCENE_PROPERTIES: usize = 512;
pub const MAX_SCENE_PROPERTY_NAME: usize = 128;

impl Source {
    pub fn static_file(path: impl Into<String>) -> Self {
        Self {
            kind: SourceKind::Static,
            path: path.into(),
            engine: None,
            frame_rate: None,
            properties: None,
        }
    }

    pub fn video(path: impl Into<String>, engine: Option<VideoEngine>) -> Self {
        Self {
            kind: SourceKind::Video,
            path: path.into(),
            engine,
            frame_rate: None,
            properties: None,
        }
    }

    pub fn tinier_video(path: impl Into<String>, frame_rate: impl Into<String>) -> Self {
        Self {
            kind: SourceKind::Video,
            path: path.into(),
            engine: Some(VideoEngine::Tinier),
            frame_rate: Some(frame_rate.into()),
            properties: None,
        }
    }

    pub fn wallpaper_engine(path: impl Into<String>) -> Self {
        Self {
            kind: SourceKind::WallpaperEngine,
            path: path.into(),
            engine: None,
            frame_rate: None,
            properties: None,
        }
    }

    #[must_use]
    pub fn with_properties(
        mut self,
        properties: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        self.properties = Some(properties);
        self
    }

    pub fn effective_video_engine(&self) -> Option<VideoEngine> {
        (self.kind == SourceKind::Video).then_some(self.engine.unwrap_or_default())
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.path.trim().is_empty() {
            return Err(ValidationError::EmptySourcePath);
        }
        if self.path.starts_with('-') {
            return Err(ValidationError::SourcePathStartsWithDash);
        }
        if self.kind != SourceKind::Video && self.engine.is_some() {
            return Err(ValidationError::EngineNotAllowed(self.kind));
        }
        if self.kind != SourceKind::Video && self.frame_rate.is_some() {
            return Err(ValidationError::FrameRateNotAllowed(self.kind));
        }
        if let Some(properties) = &self.properties {
            if self.kind != SourceKind::WallpaperEngine {
                return Err(ValidationError::PropertiesNotAllowed(self.kind));
            }
            if properties.len() > MAX_SCENE_PROPERTIES {
                return Err(ValidationError::TooManySceneProperties(properties.len()));
            }
            for name in properties.keys() {
                if name.trim().is_empty() || name.len() > MAX_SCENE_PROPERTY_NAME {
                    return Err(ValidationError::InvalidScenePropertyName(name.clone()));
                }
            }
        }
        if let Some(frame_rate) = &self.frame_rate {
            validate_frame_rate(frame_rate)?;
        }
        if self.effective_video_engine() == Some(VideoEngine::Tinier) && self.frame_rate.is_none() {
            return Err(ValidationError::TinierFrameRateRequired);
        }
        if self.effective_video_engine() != Some(VideoEngine::Tinier) && self.frame_rate.is_some() {
            return Err(ValidationError::FrameRateRequiresTinier);
        }
        Ok(())
    }
}

fn validate_frame_rate(value: &str) -> Result<(), ValidationError> {
    let (numerator, denominator) = match value.split_once('/') {
        Some((left, right)) if !right.contains('/') => (left, right),
        Some(_) => return Err(ValidationError::InvalidFrameRate(value.to_string())),
        None => (value, "1"),
    };
    let parse = |part: &str| {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        part.parse::<u32>().ok().filter(|part| *part > 0)
    };
    let Some(numerator) = parse(numerator) else {
        return Err(ValidationError::InvalidFrameRate(value.to_string()));
    };
    let Some(denominator) = parse(denominator) else {
        return Err(ValidationError::InvalidFrameRate(value.to_string()));
    };
    if u64::from(numerator) > u64::from(denominator) * 240 {
        return Err(ValidationError::InvalidFrameRate(value.to_string()));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub outputs: Vec<String>,
    pub source: Source,
    #[serde(default, skip_serializing_if = "is_assignment_default")]
    pub fill_mode: FillMode,
    #[serde(default = "default_mute", skip_serializing_if = "is_assignment_default")]
    pub mute: bool,
    #[serde(default = "default_volume", skip_serializing_if = "is_assignment_default")]
    pub volume: u32,
    #[serde(default, skip_serializing_if = "is_assignment_default")]
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<TransitionPolicy>,
}

impl Assignment {
    pub fn new(outputs: Vec<String>, source: Source) -> Self {
        Self {
            outputs,
            source,
            fill_mode: FillMode::default(),
            mute: default_mute(),
            volume: default_volume(),
            layer: Layer::default(),
            transition: None,
        }
    }

    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.volume = self.volume.min(100);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyRequest {
    pub assignments: Vec<Assignment>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub replace_all: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RendererPolicy>,
}

impl ApplyRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.assignments.is_empty() {
            return Err(ValidationError::EmptyAssignments);
        }

        let mut explicit_outputs = BTreeSet::new();
        let mut has_wildcard = false;
        for assignment in &self.assignments {
            if assignment.outputs.is_empty() {
                return Err(ValidationError::AssignmentWithoutOutputs);
            }
            assignment.source.validate()?;
            if assignment.source.effective_video_engine() == Some(VideoEngine::Tinier) {
                if assignment.outputs.iter().any(|output| output == "*") {
                    return Err(ValidationError::TinierWildcardNotSupported);
                }
                if assignment.fill_mode != FillMode::Fill {
                    return Err(ValidationError::TinierFillModeNotSupported(assignment.fill_mode));
                }
                if assignment.layer != Layer::Background {
                    return Err(ValidationError::TinierLayerNotSupported(assignment.layer));
                }
                if assignment.transition.is_some() {
                    return Err(ValidationError::TinierTransitionNotSupported);
                }
                if !assignment.mute {
                    return Err(ValidationError::TinierAudioNotSupported);
                }
            }
            if assignment.volume > 100 {
                return Err(ValidationError::VolumeOutOfRange(assignment.volume));
            }
            if assignment.source.kind == SourceKind::Static && assignment.layer != Layer::Background
            {
                return Err(ValidationError::LayerNotAllowed(assignment.layer));
            }
            if let Some(transition) = &assignment.transition {
                transition.validate()?;
            }
            for output in &assignment.outputs {
                if output.trim().is_empty() {
                    return Err(ValidationError::EmptyOutput);
                }
                if output == "*" {
                    has_wildcard = true;
                } else {
                    if output.contains(',') {
                        return Err(ValidationError::OutputContainsComma(output.clone()));
                    }
                    if output.starts_with('-') {
                        return Err(ValidationError::OutputStartsWithDash(output.clone()));
                    }
                    if !explicit_outputs.insert(output.clone()) {
                        return Err(ValidationError::DuplicateOutput(output.clone()));
                    }
                }
            }
        }

        if has_wildcard
            && (self.assignments.len() != 1 || self.assignments[0].outputs.as_slice() != ["*"])
        {
            return Err(ValidationError::WildcardMixed);
        }
        if let Some(policy) = &self.policy {
            policy.validate()?;
            if policy.surface.is_some()
                && self.assignments.iter().any(|assignment| {
                    assignment.source.effective_video_engine() == Some(VideoEngine::Tinier)
                        || assignment.transition.is_some()
                })
            {
                return Err(ValidationError::InvalidSurfacePolicy);
            }
            if policy.transitions_enabled == Some(false)
                && self.assignments.iter().any(|assignment| assignment.transition.is_some())
            {
                return Err(ValidationError::TransitionsDisabled);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
}

impl StopRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut outputs = BTreeSet::new();
        for output in &self.outputs {
            if output.trim().is_empty() {
                return Err(ValidationError::EmptyOutput);
            }
            if output != "*" && output.contains(',') {
                return Err(ValidationError::OutputContainsComma(output.clone()));
            }
            if output.starts_with('-') {
                return Err(ValidationError::OutputStartsWithDash(output.clone()));
            }
            if !outputs.insert(output.clone()) {
                return Err(ValidationError::DuplicateOutput(output.clone()));
            }
        }
        if self.outputs.iter().any(|output| output == "*") && self.outputs.as_slice() != ["*"] {
            return Err(ValidationError::WildcardMixed);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusRequest {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitiesRequest {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reset_decode_cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseRequest {
    pub paused: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSetRequest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<u32>,
}

impl AudioSetRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        StopRequest { outputs: self.outputs.clone() }.validate()?;
        if self.mute.is_none() && self.volume.is_none() {
            return Err(ValidationError::MissingAudioChange);
        }
        if let Some(volume) = self.volume
            && volume > 100
        {
            return Err(ValidationError::VolumeOutOfRange(volume));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererReady {
    pub pid: u32,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererFailed {
    pub pid: u32,
    pub generation: u64,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: u64,
    #[serde(flatten)]
    pub params: RequestParams,
}

impl Request {
    pub fn new(id: u64, params: RequestParams) -> Self {
        Self { id, params }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        match &self.params {
            RequestParams::Apply(request) => request.validate(),
            RequestParams::Stop(request) => request.validate(),
            RequestParams::AudioSet(request) => request.validate(),
            RequestParams::Pause(_)
            | RequestParams::Status(_)
            | RequestParams::Capabilities(_)
            | RequestParams::Ready(_)
            | RequestParams::Failed(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum RequestParams {
    #[serde(rename = "paper.apply")]
    Apply(ApplyRequest),
    #[serde(rename = "paper.stop")]
    Stop(StopRequest),
    #[serde(rename = "paper.pause")]
    Pause(PauseRequest),
    #[serde(rename = "paper.audio.set")]
    AudioSet(AudioSetRequest),
    #[serde(rename = "paper.status")]
    Status(StatusRequest),
    #[serde(rename = "paper.capabilities")]
    Capabilities(CapabilitiesRequest),
    #[serde(rename = "paper.ready")]
    Ready(RendererReady),
    #[serde(rename = "paper.failed")]
    Failed(RendererFailed),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentStatus {
    pub outputs: Vec<String>,
    pub source: Source,
    pub fill_mode: FillMode,
    pub mute: bool,
    pub volume: u32,
    pub layer: Layer,
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub ready: bool,
}

impl AssignmentStatus {
    pub fn from_assignment(
        assignment: &Assignment,
        generation: u64,
        pid: Option<u32>,
        ready: bool,
    ) -> Self {
        Self {
            outputs: assignment.outputs.clone(),
            source: assignment.source.clone(),
            fill_mode: assignment.fill_mode,
            mute: assignment.mute,
            volume: assignment.volume,
            layer: assignment.layer,
            generation,
            pid,
            ready,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyResult {
    pub generation: u64,
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RendererPolicy>,
    pub assignments: Vec<AssignmentStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopResult {
    pub stopped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResult {
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RendererPolicy>,
    pub assignments: Vec<AssignmentStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renderers: Vec<RendererCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseResult {
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSetResult {
    pub updated: usize,
    pub assignments: Vec<AssignmentStatus>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlCapabilities {
    pub pause: bool,
    pub audio: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionCapabilities {
    pub startup_source_kinds: Vec<SourceKind>,
    pub static_overlay: bool,
    pub default_effect: String,
    pub default_duration_ms: u64,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererPolicyCapabilities {
    #[serde(default)]
    pub surface: bool,
    pub idle: bool,
    pub sand: bool,
    pub scene: bool,
    pub output_fps: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallpaperEngineCapabilities {
    pub project_types: Vec<String>,
    pub rejected_project_types: Vec<String>,
    pub scene: NativeSceneCapabilities,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSceneCapabilities {
    pub renderer: String,
    pub supported: Vec<String>,
    pub partial: Vec<String>,
    pub unsupported: Vec<String>,
    pub strict_gap_rejection: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodeCapabilities {
    pub reset_supported: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererDiscovery {
    Configured,
    Sibling,
    PrivateSibling,
    Path,
    Injected,
    #[default]
    Unresolved,
}

impl RendererDiscovery {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Sibling => "sibling",
            Self::PrivateSibling => "private_sibling",
            Self::Path => "path",
            Self::Injected => "injected",
            Self::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeDependencyStatus {
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererCapability {
    pub executable: String,
    pub source_kinds: Vec<SourceKind>,
    #[serde(default)]
    pub video_engines: Vec<VideoEngine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub discovery: RendererDiscovery,
    pub present: bool,
    pub executable_file: bool,
    #[serde(default)]
    pub dependencies: Vec<RuntimeDependencyStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

impl RendererCapability {
    pub fn available(&self) -> bool {
        self.executable_file && self.dependencies.iter().all(|dependency| dependency.available)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitiesResult {
    pub protocol: String,
    pub version: u32,
    pub source_kinds: Vec<SourceKind>,
    pub video_engines: Vec<VideoEngine>,
    pub fill_modes: Vec<FillMode>,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub controls: ControlCapabilities,
    #[serde(default)]
    pub transitions: TransitionCapabilities,
    #[serde(default)]
    pub renderer_policy: RendererPolicyCapabilities,
    #[serde(default)]
    pub wallpaper_engine: WallpaperEngineCapabilities,
    #[serde(default)]
    pub decode: DecodeCapabilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renderers: Vec<RendererCapability>,
}

impl CapabilitiesResult {
    pub fn current() -> Self {
        Self {
            protocol: PROTOCOL_NAME.to_string(),
            version: PROTOCOL_VERSION,
            source_kinds: vec![SourceKind::Static, SourceKind::Video, SourceKind::WallpaperEngine],
            video_engines: vec![VideoEngine::Default, VideoEngine::Tinier],
            fill_modes: FillMode::ALL.to_vec(),
            layers: vec![Layer::Background, Layer::Bottom, Layer::Top, Layer::Overlay],
            controls: ControlCapabilities { pause: true, audio: true },
            transitions: TransitionCapabilities {
                startup_source_kinds: vec![SourceKind::Video, SourceKind::WallpaperEngine],
                static_overlay: true,
                default_effect: "fade".to_string(),
                default_duration_ms: 600,
                min_duration_ms: 50,
                max_duration_ms: 10_000,
            },
            renderer_policy: RendererPolicyCapabilities {
                surface: true,
                idle: true,
                sand: true,
                scene: true,
                output_fps: true,
            },
            wallpaper_engine: WallpaperEngineCapabilities {
                project_types: vec!["scene".into(), "video".into()],
                rejected_project_types: vec!["web".into(), "application".into()],
                scene: NativeSceneCapabilities {
                    renderer: "native-vulkan".into(),
                    supported: [
                        "image-layers",
                        "effect-chains",
                        "cross-layer-render-targets",
                        "particles",
                        "puppet-skeletons",
                        "scene-audio-mixing",
                        "user-properties",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                    partial: [
                        "shader-dialect",
                        "particle-timing-density-trails",
                        "particle-refraction",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                    unsupported: [
                        "embedded-video-textures",
                        "animated-image-textures",
                        "audio-reactivity",
                        "event-driven-sounds",
                        "light-objects",
                        "text-objects",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                    strict_gap_rejection: true,
                },
            },
            decode: DecodeCapabilities { reset_supported: false },
            renderers: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_renderers(mut self, renderers: Vec<RendererCapability>) -> Self {
        self.renderers = renderers;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response<T> {
    pub id: u64,
    #[serde(flatten)]
    pub body: ResponseBody<T>,
}

impl<T> Response<T> {
    pub fn success(id: u64, result: T) -> Self {
        Self { id, body: ResponseBody::Success { result } }
    }

    pub fn failure(id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id,
            body: ResponseBody::Failure {
                error: ResponseError { code: code.into(), message: message.into() },
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseBody<T> {
    Success { result: T },
    Failure { error: ResponseError },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: String,
    pub message: String,
}

pub type ApplyResponse = Response<ApplyResult>;
pub type StopResponse = Response<StopResult>;
pub type PauseResponse = Response<PauseResult>;
pub type AudioSetResponse = Response<AudioSetResult>;
pub type StatusResponse = Response<StatusResult>;
pub type CapabilitiesResponse = Response<CapabilitiesResult>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidSurfacePolicy,
    EmptyAssignments,
    AssignmentWithoutOutputs,
    EmptyOutput,
    EmptySourcePath,
    SourcePathStartsWithDash,
    EngineNotAllowed(SourceKind),
    FrameRateNotAllowed(SourceKind),
    FrameRateRequiresTinier,
    TinierFrameRateRequired,
    InvalidFrameRate(String),
    TinierFillModeNotSupported(FillMode),
    TinierLayerNotSupported(Layer),
    TinierTransitionNotSupported,
    TinierAudioNotSupported,
    TinierWildcardNotSupported,
    TransitionNotAllowed(SourceKind),
    EmptyTransitionFrom,
    TransitionFromStartsWithDash,
    InvalidTransitionEffect(String),
    TransitionDurationOutOfRange(u64),
    TransitionsDisabled,
    VolumeOutOfRange(u32),
    LayerNotAllowed(Layer),
    OutputContainsComma(String),
    OutputStartsWithDash(String),
    DuplicateOutput(String),
    WildcardMixed,
    MissingAudioChange,
    SandFpsOutOfRange(u32),
    SceneFpsOutOfRange(u32),
    EmptySceneAssetsDirectory,
    SceneDimensionOutOfRange(u32),
    SceneEffectChainsOutOfRange(u32),
    SceneEffectPassesOutOfRange(u32),
    InvalidPolicyOutput(String),
    OutputFpsOutOfRange(String, u32),
    PropertiesNotAllowed(SourceKind),
    TooManySceneProperties(usize),
    InvalidScenePropertyName(String),
}

impl Display for ValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSurfacePolicy => {
                formatter.write_str("invalid or unsupported surface policy")
            }
            Self::EmptyAssignments => formatter.write_str("apply requires at least one assignment"),
            Self::AssignmentWithoutOutputs => {
                formatter.write_str("each assignment requires at least one output")
            }
            Self::EmptyOutput => formatter.write_str("output names must not be empty"),
            Self::EmptySourcePath => formatter.write_str("source path must not be empty"),
            Self::SourcePathStartsWithDash => {
                formatter.write_str("source path must not start with a hyphen")
            }
            Self::EngineNotAllowed(kind) => {
                write!(formatter, "video engine is not valid for {kind:?} sources")
            }
            Self::FrameRateNotAllowed(kind) => {
                write!(formatter, "frame rate is not valid for {kind:?} sources")
            }
            Self::FrameRateRequiresTinier => {
                formatter.write_str("frame rate is only valid for the tinier video engine")
            }
            Self::TinierFrameRateRequired => {
                formatter.write_str("the tinier video engine requires a frame rate")
            }
            Self::InvalidFrameRate(value) => {
                write!(formatter, "invalid frame rate '{value}'")
            }
            Self::TinierFillModeNotSupported(mode) => {
                write!(formatter, "the tinier video engine does not support {mode:?} placement")
            }
            Self::TinierLayerNotSupported(layer) => {
                write!(formatter, "the tinier video engine does not support the {layer:?} layer")
            }
            Self::TinierTransitionNotSupported => {
                formatter.write_str("the tinier video engine does not support transitions")
            }
            Self::TinierAudioNotSupported => {
                formatter.write_str("the tinier video engine does not support audio")
            }
            Self::TinierWildcardNotSupported => {
                formatter.write_str("the tinier video engine requires named outputs")
            }
            Self::TransitionNotAllowed(kind) => {
                write!(formatter, "transitions are not supported for {kind:?} sources")
            }
            Self::EmptyTransitionFrom => {
                formatter.write_str("transition source path must not be empty")
            }
            Self::TransitionFromStartsWithDash => {
                formatter.write_str("transition source path must not start with a hyphen")
            }
            Self::InvalidTransitionEffect(effect) => {
                write!(formatter, "transition effect {effect} is not a safe token")
            }
            Self::TransitionDurationOutOfRange(duration_ms) => {
                write!(formatter, "transition duration {duration_ms} is outside 50..=10000 ms")
            }
            Self::TransitionsDisabled => formatter
                .write_str("assignment transitions conflict with disabled renderer transitions"),
            Self::VolumeOutOfRange(volume) => {
                write!(formatter, "volume {volume} is outside the range 0..=100")
            }
            Self::LayerNotAllowed(layer) => {
                write!(formatter, "layer {layer:?} is not valid for static sources")
            }
            Self::OutputContainsComma(output) => {
                write!(formatter, "output {output} must not contain a comma")
            }
            Self::OutputStartsWithDash(output) => {
                write!(formatter, "output {output} must not start with a hyphen")
            }
            Self::DuplicateOutput(output) => {
                write!(formatter, "output {output} appears in more than one assignment")
            }
            Self::WildcardMixed => {
                formatter.write_str("wildcard output cannot be combined with other outputs")
            }
            Self::MissingAudioChange => formatter.write_str("audio update requires mute or volume"),
            Self::SandFpsOutOfRange(fps) => {
                write!(formatter, "sand FPS {fps} is outside the range 1..=1000")
            }
            Self::SceneFpsOutOfRange(fps) => {
                write!(formatter, "scene FPS {fps} is outside the range 1..=240")
            }
            Self::EmptySceneAssetsDirectory => {
                formatter.write_str("scene assets directory must not be empty")
            }
            Self::SceneDimensionOutOfRange(dimension) => write!(
                formatter,
                "scene maximum dimension {dimension} is outside the range 256..=16384"
            ),
            Self::SceneEffectChainsOutOfRange(chains) => {
                write!(formatter, "scene effect chain limit {chains} is outside the range 1..=64")
            }
            Self::SceneEffectPassesOutOfRange(passes) => {
                write!(formatter, "scene effect pass limit {passes} is outside the range 1..=64")
            }
            Self::InvalidPolicyOutput(output) => {
                write!(formatter, "renderer policy output {output} is invalid")
            }
            Self::OutputFpsOutOfRange(output, fps) => {
                write!(formatter, "output FPS {fps} for {output} is outside the range 0..=1000")
            }
            Self::PropertiesNotAllowed(kind) => {
                write!(formatter, "scene properties are not valid for {kind:?} sources")
            }
            Self::TooManySceneProperties(count) => {
                write!(
                    formatter,
                    "{count} scene properties exceeds the limit of {MAX_SCENE_PROPERTIES}"
                )
            }
            Self::InvalidScenePropertyName(name) => {
                write!(formatter, "scene property name {name:?} is empty or too long")
            }
        }
    }
}

impl Error for ValidationError {}

#[derive(Debug)]
pub enum NdjsonError {
    MissingTerminator,
    EmptyRecord,
    MultipleRecords,
    Json(serde_json::Error),
}

impl Display for NdjsonError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTerminator => formatter.write_str("NDJSON record is missing a newline"),
            Self::EmptyRecord => formatter.write_str("NDJSON record is empty"),
            Self::MultipleRecords => formatter.write_str("expected exactly one NDJSON record"),
            Self::Json(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for NdjsonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::MissingTerminator | Self::EmptyRecord | Self::MultipleRecords => None,
        }
    }
}

pub fn encode_ndjson<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(value).map(|mut record| {
        record.push('\n');
        record
    })
}

pub fn decode_ndjson<T: DeserializeOwned>(line: &str) -> Result<T, NdjsonError> {
    let record = line.strip_suffix('\n').ok_or(NdjsonError::MissingTerminator)?;
    let record = record.strip_suffix('\r').unwrap_or(record);
    if record.trim().is_empty() {
        return Err(NdjsonError::EmptyRecord);
    }
    if record.contains(['\r', '\n']) {
        return Err(NdjsonError::MultipleRecords);
    }
    serde_json::from_str(record).map_err(NdjsonError::Json)
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

trait AssignmentDefault {
    fn is_assignment_default(&self) -> bool;
}

impl AssignmentDefault for FillMode {
    fn is_assignment_default(&self) -> bool {
        *self == Self::Fill
    }
}

impl AssignmentDefault for Layer {
    fn is_assignment_default(&self) -> bool {
        *self == Self::Background
    }
}

impl AssignmentDefault for bool {
    fn is_assignment_default(&self) -> bool {
        *self
    }
}

impl AssignmentDefault for u32 {
    fn is_assignment_default(&self) -> bool {
        *self == default_volume()
    }
}

fn is_assignment_default<T: AssignmentDefault>(value: &T) -> bool {
    value.is_assignment_default()
}

const fn default_mute() -> bool {
    true
}

const fn default_volume() -> u32 {
    80
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
