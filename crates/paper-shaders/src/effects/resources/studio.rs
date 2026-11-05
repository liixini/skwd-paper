pub const STUDIO_ENVIRONMENT_SIZE: [u32; 2] = [1536, 1024];
pub const STUDIO_ENVIRONMENT_RGBA: &[u8] = include_bytes!("studio/environment.rgba");

pub(in crate::effects) const PHOTO_CUTOUT_FRAG: &str =
    concat!(include_str!("studio/common.glsl"), include_str!("studio/photo_cutout.glsl"));
pub(in crate::effects) const CROP_RELAY_FRAG: &str =
    concat!(include_str!("studio/common.glsl"), include_str!("studio/crop_relay.glsl"));
pub(in crate::effects) const PAPER_HEART_FRAG: &str =
    concat!(include_str!("studio/common.glsl"), include_str!("studio/paper_heart.glsl"));
pub(in crate::effects) const SLAT_TURN_FRAG: &str = concat!(
    include_str!("studio/common.glsl"),
    include_str!("studio/environment.glsl"),
    include_str!("studio/world.glsl"),
    include_str!("studio/slat_turn.glsl")
);
pub(in crate::effects) const SUNLIT_ARCADE_FRAG: &str = concat!(
    include_str!("studio/common.glsl"),
    include_str!("studio/environment.glsl"),
    include_str!("studio/world.glsl"),
    include_str!("studio/sunlit_arcade.glsl")
);
