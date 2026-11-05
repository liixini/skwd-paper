mod drawing;
pub use drawing::{
    DRAWING_DURATION_MAX, DRAWING_EFFECT, DRAWING_ENV, DRAWING_INK, DRAWING_PAPER,
    DRAWING_SOURCE_ENV, DRAWING_VERSION, DrawingColours, DrawingInk, DrawingManifest,
    DrawingSource, DrawingTransition, PREPARE_HIDDEN_ENV,
};
mod strokes;
pub use strokes::DrawingStroke;
mod background;
mod media;

pub use background::worker_background;
pub use paper_geom::Background;
mod multi_video;
mod output_target;
mod paper_command;
mod playback;
mod protocol;
pub use playback::{PLAYBACK_ENV, PlaybackState};
mod scene_thumbnail;
mod socket;
mod stdin_reader;
mod still_command;
pub mod transitions;
pub mod we_project;
mod widget;

pub use media::{VIDEO_EXTS, is_video_path};
pub use multi_video::MultiVideoEntry;
pub use output_target::OutputTarget;
pub use paper_command::{
    CommandClass, FrameSnapshot, PaperCommand, PointerState, SceneCapture, classify_command,
};
pub use protocol::{
    ALL_OUTPUT, ApplyRequest, ApplyResponse, ApplyResult, Assignment, AssignmentStatus,
    AudioSetRequest, AudioSetResponse, AudioSetResult, CapabilitiesRequest, CapabilitiesResponse,
    CapabilitiesResult, ControlCapabilities, DecodeCapabilities, FillMode, Layer,
    NativeSceneCapabilities, NdjsonError, PROTOCOL_NAME, PROTOCOL_VERSION, PauseRequest,
    PauseResponse, PauseResult, PlaybackControl, RendererCapability, RendererDiscovery,
    RendererFailed, RendererPolicy, RendererPolicyCapabilities, RendererReady, Request,
    RequestParams, Response, ResponseBody, ResponseError, RuntimeDependencyStatus, SandPolicy,
    SandQuality, SandScope, SceneClamp, ScenePolicy, Source, SourceKind, StatusRequest,
    StatusResponse, StatusResult, StopRequest, StopResponse, StopResult, SurfaceOverridesRequest,
    SurfacePolicy, TransitionCapabilities, TransitionPolicy, ValidationError,
    WallpaperEngineCapabilities, decode_ndjson, encode_ndjson,
};
pub use scene_thumbnail::{SceneThumbnailRequest, SceneThumbnailResponse};
pub use socket::{
    signal_paper_failed, signal_paper_failed_generation_to, signal_paper_ready,
    signal_paper_ready_generation_to, signal_paper_ready_to, socket_path,
};
pub use stdin_reader::spawn_stdin_line_reader;
pub use still_command::{MirrorTarget, StillCommand};
pub use widget::{
    ClockStyle, Depth, Face, Labels, MAX_OUTLINES, MAX_WIDGETS, MONTHS, Palette, Pin, WEEKDAYS,
    WIDGET_HEIGHTS, Widget, WidgetScene,
};
