mod media;
mod multi_video;
mod output_target;
mod paper_command;
mod protocol;
mod scene_thumbnail;
mod socket;
mod stdin_reader;
mod still_command;

pub use media::{VIDEO_EXTS, is_video_path};
pub use multi_video::MultiVideoEntry;
pub use output_target::OutputTarget;
pub use paper_command::{CommandClass, PaperCommand, SceneCapture, classify_command};
pub use protocol::{
    ApplyRequest, ApplyResponse, ApplyResult, Assignment, AssignmentStatus, AudioSetRequest,
    AudioSetResponse, AudioSetResult, CapabilitiesRequest, CapabilitiesResponse,
    CapabilitiesResult, ControlCapabilities, DecodeCapabilities, FillMode, Layer,
    NativeSceneCapabilities, NdjsonError, PROTOCOL_NAME, PROTOCOL_VERSION, PauseRequest,
    PauseResponse, PauseResult, RendererCapability, RendererDiscovery, RendererFailed,
    RendererPolicy, RendererPolicyCapabilities, RendererReady, Request, RequestParams, Response,
    ResponseBody, ResponseError, RuntimeDependencyStatus, SandPolicy, SandQuality, SandScope,
    ScenePolicy, Source, SourceKind, StatusRequest, StatusResponse, StatusResult, StopRequest,
    StopResponse, StopResult, SurfacePolicy, TransitionCapabilities, TransitionPolicy,
    ValidationError, VideoEngine, WallpaperEngineCapabilities, decode_ndjson, encode_ndjson,
};
pub use scene_thumbnail::{SceneThumbnailRequest, SceneThumbnailResponse};
pub use socket::{
    signal_paper_failed, signal_paper_failed_generation_to, signal_paper_ready,
    signal_paper_ready_generation_to, signal_paper_ready_to, socket_path,
};
pub use stdin_reader::spawn_stdin_line_reader;
pub use still_command::StillCommand;
