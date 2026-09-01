pub(super) struct StartFade {
    pub(super) from: String,
    pub(super) shader: Option<String>,
    pub(super) duration_ms: u64,
    pub(super) overlay: bool,
    pub(super) held: bool,
}
