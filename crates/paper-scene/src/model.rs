use crate::affine::Affine;
use crate::pkg::Package;
use crate::tex;
use anyhow::{Result, anyhow};
use serde_json::Value;

pub mod bloom;
mod camera_object;
mod hidden;
pub use camera_object::{CameraSource, CameraView, Placement, active_camera_view, placement};
pub mod fog;
pub(crate) mod hdr;
pub mod lighting;
pub mod mesh;
mod models;
mod objects;
pub use models::PreparedModel;
pub mod script;
pub(crate) mod shape;
pub(crate) mod texture_cache;

pub const MAX_SCENE_TEXTURE_BYTES: usize = 1536 * 1024 * 1024;
pub const MAX_SCENE_OBJECTS: usize = 16_384;
pub const IMAGE_UV_INSET: f32 = 0.15;
const NORMAL_COLOR_BLEND: u32 = 0;
const MAX_USER_TEXTURE_FILE_BYTES: usize = 16 << 20;
const MS_PER_SECOND: f32 = 1000.0;
const MIN_GIF_FRAME_SECONDS: f32 = 0.01;
pub(crate) const GENERAL_FIELD: &str = "general";
pub(crate) const PROJECTION_FIELD: &str = "orthogonalprojection";
pub(crate) const PROJECTION_AUTO: &str = "auto";
pub(crate) const PROJECTION_WIDTH: &str = "width";
pub(crate) const PROJECTION_HEIGHT: &str = "height";
const CAMERA_FIELD: &str = "camera";
const CAMERA_IMAGE_GRAPH_GAP: &str = "authored camera image graph unsupported";
const DEPTH_COMPOSITE_GAP: &str = "depth-tested effect composite unsupported";
const MESH_CAMERA_UNAVAILABLE: &str = "mesh camera unavailable";
const REFLECTION_SOURCE_UNSUPPORTED: &str = "non-mesh reflection source unsupported";
const REFLECTED_MOTION_GAP: &str = "animated mesh missing from reflection";
const CAMERA_EYE: &str = "eye";
const CAMERA_CENTER: &str = "center";
const CAMERA_UP: &str = "up";
const CAMERA_NEAR: &str = "nearz";
const CAMERA_FAR: &str = "farz";
const CAMERA_FOV: &str = "fov";
const PERSPECTIVE_FOV_FIELD: &str = "perspectiveoverridefov";
const DEFAULT_CAMERA_FOV: f32 = 50.0;
const DEFAULT_PERSPECTIVE_FOV: f32 = 95.0;
pub const FOV_RANGE: (f32, f32) = (1.0, 179.0);
const CAMERA_DEFAULTS: [(&str, f32); 3] =
    [(CAMERA_NEAR, 0.1), (CAMERA_FAR, 10_000.0), (CAMERA_FOV, DEFAULT_CAMERA_FOV)];
const CAMERA_FOV_MIN: f32 = 0.1;
const CAMERA_FOV_MAX: f32 = 179.9;
const OBJECTS_FIELD: &str = "objects";
const SCENE_VERSION_FIELD: &str = "version";
pub(crate) const PARTICLE_FIELD: &str = "particle";
const SOUND_FIELD: &str = "sound";
const CAMERA_OTHER_OBJECT_FIELDS: [&str; 7] =
    ["image", "text", "light", "sound", "model", "camera", shape::FIELD];
pub(crate) const ID_FIELD: &str = "id";
const NAME_FIELD: &str = "name";
const VISIBLE_FIELD: &str = "visible";
const COLOR_FIELD: &str = "color";
const COLOR_BLEND_FIELD: &str = "colorBlendMode";
const IMAGE_FIELD: &str = "image";
const ALIGNMENT_FIELD: &str = "alignment";
const CENTERED: &str = "center";
pub(crate) const MESH_FIELD: &str = "model";
const SKIN_FIELD: &str = "skin";
const DEPENDENCIES_FIELD: &str = "dependencies";
pub(crate) const MESH_MORPH_GAP: &str = "skeletal mesh morph section unsupported";
const RESOLVED_FRAME_GAP: &str = "mesh resolved frame buffer unsupported";
const DEGENERATE_MESH_GAP: &str = "degenerate mesh pose unsupported";
const VARIANT_CONDITION_FIELD: &str = "condition";
const MOUNT_GAP: &str = "mesh attachment owner unavailable";
const ORIGIN_FIELD: &str = "origin";
const MESH_AXES: usize = 3;
const VISUAL_FIELDS: [&str; 6] =
    [IMAGE_FIELD, MESH_FIELD, PARTICLE_FIELD, "text", "light", shape::FIELD];
const DEFAULT_LAYER_NAME: &str = "object";
const DIAGNOSTIC_SEPARATOR: &str = "; ";
const MODEL_AUTOSIZE_FIELD: &str = "autosize";
const MODEL_FULLSCREEN_FIELD: &str = "fullscreen";
const MODEL_NO_PADDING_FIELD: &str = "nopadding";
const MODEL_PASSTHROUGH_FIELD: &str = "passthrough";
const MODEL_SOLID_LAYER_FIELD: &str = "solidlayer";
const MODEL_PROJECT_LAYER_FIELD: &str = "projectlayer";
const MODEL_FULL_UV_FIELDS: [&str; 4] = [
    MODEL_FULLSCREEN_FIELD,
    MODEL_NO_PADDING_FIELD,
    MODEL_PASSTHROUGH_FIELD,
    MODEL_SOLID_LAYER_FIELD,
];

pub struct SceneModel {
    pub reflection: ReflectionUse,
    pub unreflected: std::collections::BTreeSet<String>,
    pub custom_shaders: bool,
    pub cameras_move: bool,
    pub camera_source: CameraSource,
    pub skin_viewport: bool,
    pub image_sources: std::collections::BTreeSet<String>,
    pub dependencies: std::collections::BTreeSet<String>,
    pub scripts: Option<crate::script::SceneScripts>,
    pub keyframes: crate::keyframe::Bindings,
    pub animated_scene: Option<(Value, Properties)>,
    pub canvas: (f32, f32),
    pub mouse: crate::mouse::Parallax,
    pub shake: Option<crate::mouse::Shake>,
    pub camera_fov: f32,
    pub perspective_fov: f32,
    pub camera_view: CameraView,
    pub camera: Option<Camera>,
    pub projection: ProjectionMode,
    pub clear: [f32; 3],
    pub clear_enabled: bool,
    pub ambient: [f32; 3],
    pub skylight: [f32; 3],
    pub lights: Vec<lighting::Light>,
    pub sun_view: lighting::SunView,
    pub fog: fog::Volume,
    pub cookies: Option<lighting::Cookies>,
    pub layers: Vec<Layer>,
    pub meshes: Vec<MeshLayer>,
    pub particles: Vec<ParticleLayer>,
    pub skipped: Vec<String>,
    pub background_only: bool,
    pub hdr: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionMode {
    Orthographic,
    Automatic,
    Other,
}

impl ProjectionMode {
    #[must_use]
    pub fn of(scene: &Value) -> Self {
        let projection = scene.get(GENERAL_FIELD).and_then(|general| general.get(PROJECTION_FIELD));
        let auto = projection.and_then(|value| value.get(PROJECTION_AUTO)).and_then(Value::as_bool);
        let dimensions = projection.is_some_and(|value| {
            [PROJECTION_WIDTH, PROJECTION_HEIGHT]
                .iter()
                .all(|key| projection_dimension(value.get(*key)))
        });
        if auto == Some(true) {
            Self::Automatic
        } else if dimensions {
            Self::Orthographic
        } else {
            Self::Other
        }
    }
}

fn projection_dimension(value: Option<&Value>) -> bool {
    let Some(Value::Number(number)) = value else {
        return false;
    };
    if let Some(signed) = number.as_i64() {
        return signed & i64::from(u32::MAX) != 0;
    }
    if let Some(unsigned) = number.as_u64() {
        return unsigned & u64::from(u32::MAX) != 0;
    }
    number.as_f64().is_some_and(|float| float.trunc() != 0.0)
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: [f32; 3],
    pub center: [f32; 3],
    pub up: [f32; 3],
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct CameraBasis {
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
}

impl Camera {
    fn of(scene: &Value, props: &Properties, projection: ProjectionMode) -> Option<Self> {
        if projection != ProjectionMode::Other {
            return None;
        }
        let authored = scene.get(CAMERA_FIELD)?;
        let general = scene.get(GENERAL_FIELD)?;
        let eye = vec3(authored.get(CAMERA_EYE), props)?;
        let center = vec3(authored.get(CAMERA_CENTER), props)?;
        let up = vec3(authored.get(CAMERA_UP), props)?;
        let [near, far, fov] = CAMERA_DEFAULTS.map(|(field, default)| {
            general.get(field).map_or(default, |value| number(Some(value), props, f32::NAN))
        });
        if !near.is_finite() || !far.is_finite() || !fov.is_finite() || near <= 0.0 || near >= far {
            return None;
        }
        let eye = [eye.0, eye.1, eye.2];
        let center = [center.0, center.1, center.2];
        let up = [up.0, up.1, up.2];
        if eye.into_iter().chain(center).chain(up).any(|value| !value.is_finite()) {
            return None;
        }
        let camera =
            Self { eye, center, up, fov: fov.clamp(CAMERA_FOV_MIN, CAMERA_FOV_MAX), near, far };
        camera.basis()?;
        Some(camera)
    }

    pub fn basis(self) -> Option<CameraBasis> {
        CameraBasis::of(self.eye, self.center, self.up)
    }
}

impl CameraBasis {
    pub fn of(eye: [f32; 3], center: [f32; 3], up: [f32; 3]) -> Option<Self> {
        let normalize = |axis: [f32; 3]| {
            let length_squared = axis.iter().map(|value| value * value).sum::<f32>();
            length_squared.is_normal().then(|| axis.map(|value| value / length_squared.sqrt()))
        };
        let cross = |first: [f32; 3], second: [f32; 3]| {
            [
                first[1] * second[2] - first[2] * second[1],
                first[2] * second[0] - first[0] * second[2],
                first[0] * second[1] - first[1] * second[0],
            ]
        };
        let forward = normalize([center[0] - eye[0], center[1] - eye[1], center[2] - eye[2]])?;
        let right = normalize(cross(forward, up))?;
        let up = cross(right, forward);
        Some(CameraBasis { right, up, forward })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    Image,
    Text,
    Quad,
    Viewport { hdr: bool },
}

pub struct Layer {
    pub texture_name: Option<String>,
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub texture: Texture,
    pub geometry: Option<Geometry>,
    pub puppet: Option<Puppet>,
    pub center: (f32, f32),
    pub size: (f32, f32),
    pub composition_size: Option<(f32, f32)>,
    pub scale: (f32, f32),
    pub depth: f32,
    pub scene_order: usize,
    pub alpha: f32,
    pub angle: f32,
    pub color: [f32; 3],
    pub color_blend: u32,
    pub pad_uv: bool,
    pub spritesheet: bool,
    pub passthrough: bool,
    pub fullscreen: bool,
    pub copy_background: bool,
    pub compose_depth: bool,
    pub solid: bool,
    pub base_scene_input: bool,
    pub live_text: Option<crate::text::Prepared>,
    pub script_text: Option<crate::text::script::ScriptText>,
    pub kind: LayerKind,
    pub mouse: crate::mouse::LayerMouse,
    pub effects: Vec<crate::effects::Effect>,
    pub material: Option<crate::effects::material::Material>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub spatial: Option<crate::matrix::Mat4>,
    pub depth_scale: f32,
    pub transform: Affine,
    pub size: [f32; 2],
    pub offset: [f32; 2],
}

impl Geometry {
    fn new(transform: Transform, size: (f32, f32), offset: [f32; 2], canvas: (f32, f32)) -> Self {
        let depth_scale = transform.depth_scale;
        let spatial = transform.spatial.map(|mut matrix| {
            let [xx, xy, xz, _, yx, yy, yz, _, _, _, _, _, x, y, z, _] = &mut matrix;
            *x += *xx * offset[0] - *yx * offset[1];
            *y += *xy * offset[0] - *yy * offset[1];
            *z += *xz * offset[0] - *yz * offset[1];
            matrix
        });
        let affine = transform.affine;
        let mut transform = Affine {
            a: affine.a,
            b: -affine.b,
            c: -affine.c,
            d: affine.d,
            x: affine.x,
            y: canvas.1 - affine.y,
        };
        let center = transform.point([offset[0], offset[1], 0.0]);
        transform.x = center[0];
        transform.y = center[1];
        Self { spatial, transform, depth_scale, size: [size.0, size.1], offset }
    }

    pub fn projection(self, canvas: (f32, f32)) -> [[f32; 4]; 3] {
        let t = self.transform;
        [
            [
                t.a * self.size[0] * 2.0 / canvas.0,
                t.c * self.size[1] * 2.0 / canvas.0,
                0.0,
                t.x * 2.0 / canvas.0 - 1.0,
            ],
            [
                t.b * self.size[0] * 2.0 / canvas.1,
                t.d * self.size[1] * 2.0 / canvas.1,
                0.0,
                t.y * 2.0 / canvas.1 - 1.0,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }
}

pub struct ParticleLayer {
    pub parallax: [f32; 2],
    pub id: String,
    pub visible: bool,
    pub system: crate::particles::ParticleSystem,
    pub depth: f32,
    pub scene_order: usize,
}

const REFLECTED_FIELD: &str = "reflected";
const PERSPECTIVE_FIELD: &str = "perspective";
const REFLECTION_SOURCE_FIELDS: [&str; 4] = [IMAGE_FIELD, PARTICLE_FIELD, "sprite", "text"];

pub struct MeshLayer {
    pub visible: bool,
    pub reflected: bool,
    pub cast_shadow: bool,
    pub perspective: bool,
    pub id: String,
    pub name: String,
    pub mesh: mesh::Mesh,
    pub animations: Vec<crate::puppet::AnimationLayer>,
    pub materials: Vec<crate::effects::material::MeshMaterial>,
    pub pose: MeshPose,
    pub parents: Vec<MeshPose>,
    pub mount: Option<MeshMount>,
    pub scene_order: usize,
}

pub fn mesh_perspective(object: &Value, props: &Properties) -> bool {
    truthy(object.get(PERSPECTIVE_FIELD), props, false)
}

impl MeshLayer {
    pub fn animated(&self) -> bool {
        self.mesh.rig.as_ref().is_some_and(|rig| {
            !rig.springs.is_empty()
                || self.animations.iter().any(|layer| {
                    layer.animated()
                        && rig.animations.iter().any(|animation| {
                            animation.id == layer.id
                                && animation.tracks.iter().any(|track| {
                                    track.windows(2).any(|samples| samples[0] != samples[1])
                                })
                        })
                })
        })
    }
}

pub fn pass_reflects(pass: &crate::effects::EffectPass) -> bool {
    pass.binds.iter().any(|(_, bind)| *bind == crate::effects::EffectBind::Reflection)
}

fn mirrorable(mesh: &MeshLayer, meshes: &[MeshLayer]) -> bool {
    !mesh.animated()
        && mesh.mount.as_ref().is_none_or(|mount| {
            !meshes.iter().any(|owner| owner.id == mount.owner && owner.animated())
        })
        && mesh.materials.iter().all(|material| {
            !material.pass.frame_dependent()
                && material.pass.textures.iter().flatten().all(Texture::is_static)
        })
}

fn demote_reflected(meshes: &mut [MeshLayer], skipped: &mut Vec<String>) {
    let blocked: Vec<bool> =
        meshes.iter().map(|mesh| mesh.reflected && !mirrorable(mesh, meshes)).collect();
    for (mesh, blocked) in meshes.iter_mut().zip(blocked) {
        if blocked {
            mesh.reflected = false;
            skipped.push(format!("{}: {REFLECTED_MOTION_GAP}", mesh.name));
        }
    }
}

fn scene_passes<'a>(
    layers: &'a [Layer],
    meshes: &'a [MeshLayer],
    particles: &'a [ParticleLayer],
) -> impl Iterator<Item = &'a crate::effects::EffectPass> {
    particles
        .iter()
        .flat_map(|particle| {
            particle.system.pass.iter().chain(particle.system.extra_passes.iter().flatten())
        })
        .chain(
            meshes
                .iter()
                .flat_map(|layer| &layer.materials)
                .map(|material| &material.pass)
                .chain(
                    layers
                        .iter()
                        .filter_map(|layer| layer.material.as_ref())
                        .map(|material| &material.pass),
                )
                .chain(
                    layers
                        .iter()
                        .flat_map(|layer| &layer.effects)
                        .flat_map(|effect| &effect.passes),
                ),
        )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReflectionUse {
    pub bound: bool,
    pub sampled: bool,
}

impl ReflectionUse {
    pub fn of(
        layers: &[Layer],
        meshes: &[MeshLayer],
        particles: &[ParticleLayer],
        scripts: Option<&crate::script::SceneScripts>,
    ) -> Self {
        let mut usage = Self::default();
        let mut sampled = false;
        let mut model = |pass: &crate::effects::EffectPass| {
            usage.bound |= pass_reflects(pass);
            sampled |= pass.samples_reflection();
        };
        scene_passes(&[], meshes, &[]).for_each(&mut model);
        if let Some(scripts) = scripts {
            scripts.visit_created_passes(&mut model);
        }
        sampled |= scene_passes(layers, &[], particles)
            .any(crate::effects::EffectPass::samples_reflection);
        usage.sampled = usage.bound && sampled;
        usage
    }
}

fn reflection_fallback(layers: &mut [Layer], particles: &mut [ParticleLayer]) {
    let passes = layers
        .iter_mut()
        .flat_map(|layer| {
            layer
                .material
                .iter_mut()
                .map(|material| &mut material.pass)
                .chain(layer.effects.iter_mut().flat_map(|effect| &mut effect.passes))
        })
        .chain(particles.iter_mut().flat_map(|particle| {
            particle.system.pass.iter_mut().chain(particle.system.extra_passes.iter_mut().flatten())
        }));
    for pass in passes {
        pass.bind_texture(&crate::effects::EffectBind::Reflection, error_texture);
    }
}

pub fn reflection_rerun(layer: &Layer) -> bool {
    !layer.effects.is_empty()
        && ((layer.fullscreen && layer.passthrough) || (layer.is_text() && samples_scene(layer)))
}

fn samples_scene(layer: &Layer) -> bool {
    layer.passthrough
        || layer.copy_background
        || layer.base_scene_input
        || layer.effects.iter().flat_map(|effect| &effect.passes).any(|pass| {
            pass.binds.iter().any(|(_, bind)| {
                matches!(
                    bind,
                    crate::effects::EffectBind::SceneSoFar
                        | crate::effects::EffectBind::SceneUnderLayer
                )
            })
        })
}

pub fn reflection_gap(layer: &Layer) -> bool {
    let reruns = reflection_rerun(layer);
    (!reruns && (layer.passthrough || layer.copy_background || layer.base_scene_input))
        || layer.puppet.is_some()
        || scene_passes(std::slice::from_ref(layer), &[], &[]).any(pass_reflects)
}

fn reflection_source_gap(object: &Value, layers: &[Layer]) -> bool {
    object
        .get(ID_FIELD)
        .and_then(id_of)
        .is_some_and(|id| layers.iter().any(|layer| layer.id == id && reflection_gap(layer)))
}

#[derive(Clone, Copy)]
pub struct MeshPose {
    pub origin: [f32; 3],
    pub scale: [f32; 3],
    pub angles: [f32; 3],
}

impl MeshPose {
    const IDENTITY: Self = Self { origin: [0.0; 3], scale: [1.0; 3], angles: [0.0; 3] };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshMount {
    pub owner: String,
    pub attachment: String,
    pub depth: usize,
}

pub struct MeshParents {
    pub poses: Vec<MeshPose>,
    pub mount: Option<MeshMount>,
}

enum OrderedItem {
    Layer(usize),
    Particle(usize),
    Mesh(usize),
}

struct MeshObject<'a> {
    pkg: &'a Package,
    assets: &'a crate::effects::Assets,
    object: &'a Value,
    by_id: &'a std::collections::HashMap<String, &'a Value>,
    id: String,
    name: String,
    scene_order: usize,
}

fn mesh_layer(source: MeshObject<'_>) -> std::result::Result<MeshLayer, String> {
    let path = source.object[MESH_FIELD].as_str().ok_or("mesh path missing")?;
    let bytes = source.pkg.find(path).ok_or_else(|| format!("mesh {path} missing"))?;
    let mesh = mesh::parse(bytes).map_err(|err| format!("mesh {path}: {err}"))?;
    let label = path.to_owned();
    build_mesh_layer(source, mesh, &label)
}

fn build_mesh_layer(
    source: MeshObject<'_>,
    mesh: mesh::Mesh,
    path: &str,
) -> std::result::Result<MeshLayer, String> {
    let MeshObject { pkg, assets, object, by_id, id, name, scene_order } = source;
    let props = &assets.properties;
    let skin = number(object.get(SKIN_FIELD), props, 0.0);
    if !(skin >= 0.0 && skin.fract() == 0.0 && (skin as usize) < mesh.skins) {
        return Err(format!("mesh {path} skin {skin} unavailable"));
    }
    let skin = skin as usize;
    let posed = mesh_pose(object, props, path)
        .and_then(|pose| Ok((pose, mesh_parents(object, by_id, props, path)?)));
    let (pose, MeshParents { poses: parents, mount }, visible) = match posed {
        Ok((pose, parents)) => (pose, parents, true),
        Err(PoseError::Degenerate(_)) => {
            (MeshPose::IDENTITY, MeshParents { poses: Vec::new(), mount: None }, false)
        }
        Err(error) => return Err(error.into()),
    };
    let mut materials = mesh
        .parts
        .iter()
        .map(|part| {
            crate::effects::material::load_mesh(
                pkg,
                assets,
                &part.materials[skin],
                mesh.rig.as_ref().map(|rig| rig.bones.len()),
            )
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let declared: Vec<String> = dependencies(object).collect();
    if materials.iter().flat_map(|material| &material.pass.binds).any(|(_, bind)| {
        matches!(bind, crate::effects::EffectBind::LayerComposite { layer, .. } if !declared.contains(layer))
    }) {
        return Err(format!("mesh {path} undeclared layer composite unsupported"));
    }
    let animations =
        if mesh.rig.is_some() { model_animation_layers(object, props)? } else { Vec::new() };
    let palette = mesh
        .rig
        .as_ref()
        .map(|rig| {
            let samples = animations.iter().map(|layer| (*layer, 0.0)).collect::<Vec<_>>();
            crate::puppet::palette_layers(rig, &samples).map_err(|err| err.to_string())
        })
        .transpose()?;
    for material in &mut materials {
        if let Some(palette) = &palette {
            let uniform = crate::effects::material::bone_uniform(&material.pass, palette.len())?;
            let mut matrices = Vec::with_capacity(
                palette.len() * size_of::<crate::matrix::Mat4>() / size_of::<f32>(),
            );
            for bone in palette.iter() {
                let Some(crate::puppet::BonePose::Spatial(matrix)) = bone else {
                    return Err("skeletal model palette unavailable".into());
                };
                matrices.extend_from_slice(matrix);
            }
            material.pass.values.insert(uniform.name.clone(), matrices);
        }
    }
    if crate::dynamic_text::media_scripted(object.get(REFLECTED_FIELD)) {
        return Err(format!("mesh {path} dynamic reflection membership unsupported"));
    }
    let reflected = object.get(REFLECTED_FIELD).and_then(Value::as_bool).unwrap_or(true)
        && !materials.iter().any(|material| pass_reflects(&material.pass));
    let cast_shadow = lighting::casts_shadow(object, props);
    Ok(MeshLayer {
        visible,
        reflected,
        cast_shadow,
        perspective: mesh_perspective(object, props),
        id,
        name,
        mesh,
        animations,
        materials,
        pose,
        parents,
        mount,
        scene_order,
    })
}

fn mounted(mesh: &MeshLayer, meshes: &[MeshLayer]) -> bool {
    mesh.mount.as_ref().is_none_or(|mount| {
        meshes.iter().any(|owner| {
            owner.id == mount.owner
                && owner.mesh.rig.as_ref().is_some_and(|rig| {
                    rig.attachments.iter().any(|attachment| attachment.name == mount.attachment)
                })
        })
    })
}

#[derive(Debug)]
pub enum PoseError {
    Degenerate(String),
    Unsupported(String),
}

impl std::fmt::Display for PoseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::Degenerate(reason) | Self::Unsupported(reason)) = self;
        formatter.write_str(reason)
    }
}

impl From<PoseError> for String {
    fn from(error: PoseError) -> Self {
        error.to_string()
    }
}

pub fn mesh_pose(
    object: &Value,
    props: &Properties,
    path: &str,
) -> std::result::Result<MeshPose, PoseError> {
    let values = |field: &str,
                  default: [f32; MESH_AXES]|
     -> std::result::Result<[f32; MESH_AXES], PoseError> {
        let Some(value) = object.get(field) else {
            return Ok(default);
        };
        let reason = || format!("mesh {path} {field} invalid");
        let (x, y, z) = vec3(Some(value), props).ok_or_else(|| PoseError::Unsupported(reason()))?;
        let values = [x, y, z];
        if values.iter().all(|value| value.is_finite()) {
            Ok(values)
        } else {
            Err(PoseError::Degenerate(reason()))
        }
    };
    let origin = values(ORIGIN_FIELD, [0.0; MESH_AXES])?;
    let scale = values(SCALE_FIELD, [1.0; MESH_AXES])?;
    if scale.iter().any(|value| value.abs() < f32::EPSILON) {
        return Err(PoseError::Degenerate(format!("mesh {path} zero scale unsupported")));
    }
    let angles = values(ANGLES_FIELD, [0.0; MESH_AXES])?;
    Ok(MeshPose { origin, scale, angles })
}

pub fn mesh_parents<Hasher: std::hash::BuildHasher>(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value, Hasher>,
    props: &Properties,
    path: &str,
) -> std::result::Result<MeshParents, PoseError> {
    let mut parents = Vec::new();
    let mut mount = None;
    let mut seen = std::collections::HashSet::from([std::ptr::from_ref(object)]);
    let mut cursor = object;
    loop {
        let attachment = cursor.get(ATTACHMENT_FIELD).filter(|value| !value.is_null());
        let unmounted =
            || PoseError::Unsupported(format!("mesh {path} parent attachment unsupported"));
        let Some(value) = cursor.get(PARENT_FIELD) else {
            if attachment.is_some() {
                return Err(unmounted());
            }
            break;
        };
        let id = id_of(value)
            .ok_or_else(|| PoseError::Unsupported(format!("mesh {path} parent id invalid")))?;
        if let Some(attachment) = attachment {
            let (Some(name), None) = (attachment.as_str(), &mount) else {
                return Err(unmounted());
            };
            mount = Some(MeshMount {
                owner: id.clone(),
                attachment: name.into(),
                depth: parents.len(),
            });
        }
        let parent = by_id
            .get(&id)
            .copied()
            .ok_or_else(|| PoseError::Unsupported(format!("mesh {path} parent {id} missing")))?;
        if !seen.insert(std::ptr::from_ref(parent)) {
            return Err(PoseError::Unsupported(format!("mesh {path} parent cycle unsupported")));
        }
        if VISUAL_FIELDS
            .iter()
            .filter(|field| **field != MESH_FIELD)
            .any(|field| parent.get(field).is_some())
        {
            return Err(PoseError::Unsupported(format!("mesh {path} visual parent unsupported")));
        }
        parents.push(mesh_pose(parent, props, path)?);
        cursor = parent;
    }
    Ok(MeshParents { poses: parents, mount })
}

#[must_use]
pub fn scene_scripted(scripts: bool, keyframes: &crate::keyframe::Bindings) -> bool {
    scripts || !keyframes.is_empty()
}

fn effected(effects: &[crate::effects::Effect]) -> bool {
    effects.iter().any(|effect| effect.state.visible && !effect.passes.is_empty())
}

impl Layer {
    pub fn composite_depth_test(&self) -> bool {
        self.compose_depth
            || self.material.as_ref().is_some_and(|material| {
                material.state.test == crate::effects::material::DepthMode::Enabled
            })
    }

    #[must_use]
    pub fn presents_pass(&self, pass: &crate::effects::EffectPass, puppeted: bool) -> bool {
        !puppeted
            && (self.compose_depth
                || (!self.passthrough && !self.copy_background && self.composition_size.is_none()))
            && self.mouse.clock.is_none()
            && self
                .material
                .as_ref()
                .is_none_or(|material| material.blend != crate::effects::PassBlend::Screen)
            && pass.target.is_none()
            && !pass.binds.iter().any(|(_, bind)| {
                matches!(
                    bind,
                    crate::effects::EffectBind::SceneSoFar
                        | crate::effects::EffectBind::SceneUnderLayer
                )
            })
    }

    fn depth_composite_dropped(&self) -> bool {
        let last = self
            .effects
            .iter()
            .rev()
            .find(|effect| effect.state.visible)
            .and_then(|effect| effect.passes.last());
        self.composite_depth_test()
            && last.is_some_and(|pass| !self.presents_pass(pass, self.puppet.is_some()))
    }

    #[must_use]
    pub fn additive(&self) -> bool {
        self.color_blend == crate::effects::ADDITIVE_COLOR_BLEND
            && !(self.is_text() && effected(&self.effects))
    }

    #[must_use]
    pub fn additive_text(&self) -> bool {
        self.is_text() && self.color_blend == crate::effects::ADDITIVE_COLOR_BLEND
    }

    #[must_use]
    pub fn scene_input(&self) -> bool {
        let [effect] = self.effects.as_slice() else {
            return false;
        };
        self.base_scene_input && !self.passthrough && effect.scene_capture()
    }

    #[must_use]
    pub fn orthographic_mesh_ready(&self) -> bool {
        let skinned = self.material.as_ref().is_some_and(|material| material.skin.is_some());
        (self.effects.is_empty() || self.scene_input() || skinned)
            && self.material.as_ref().is_none_or(|material| material.skin.is_some())
            && !self.copy_background
            && !matches!(self.kind, LayerKind::Viewport { .. })
    }

    pub fn fit_viewport(&mut self, canvas: (f32, f32), raster: (u32, u32)) {
        let LayerKind::Viewport { hdr } = self.kind else {
            return;
        };
        self.center = (canvas.0 * 0.5, canvas.1 * 0.5);
        self.size = canvas;
        if hdr {
            hdr::resize(&mut self.effects, [raster.0, raster.1]);
        }
    }

    pub fn is_text(&self) -> bool {
        self.kind == LayerKind::Text
    }

    pub fn atlas_frames(&self) -> Option<&[SpriteFrame]> {
        self.spritesheet.then(|| self.texture.atlas_frames()).flatten()
    }

    pub fn uv(&self) -> [f32; 4] {
        let (width, height) = self.texture.uv_scale();
        if !self.pad_uv {
            return [0.0, 0.0, width, height];
        }
        let u = IMAGE_UV_INSET / self.texture.width.max(1) as f32;
        let v = IMAGE_UV_INSET / self.texture.height.max(1) as f32;
        [u, v, width - u * 2.0, height - v * 2.0]
    }

    pub fn object_index(&self, scene: &Value) -> Option<usize> {
        object_index(scene, &self.id)
    }
}

pub struct Puppet {
    pub planar_owner: bool,
    pub cull: bool,
    pub cpu: bool,
    pub world: crate::matrix::Mat4,
    pub mesh: crate::puppet::Mesh,
    pub size: (f32, f32),
    pub layers: Vec<crate::puppet::AnimationLayer>,
    pub mask_textures: Vec<std::sync::Arc<tex::Pixels>>,
}

impl Layer {
    fn skinned(&self) -> bool {
        self.material.as_ref().is_some_and(|material| material.skin.is_some())
    }

    fn load_masks(
        &mut self,
        pkg: &Package,
        assets: &crate::effects::Assets,
        texture_bytes: &mut usize,
        skipped: &mut Vec<String>,
    ) -> Result<()> {
        let drawn = !self.skinned() && !crate::effects::shader_blend(self.color_blend);
        let Some(puppet) =
            self.puppet.as_mut().filter(|puppet| !puppet.mesh.masks.records.is_empty())
        else {
            return Ok(());
        };
        let Some(textures) = drawn.then(|| puppet.read_masks(pkg, assets)).flatten() else {
            let count = puppet.mesh.masks.records.len();
            skipped.push(format!("{}: {count} puppet clipping masks unsupported", self.name));
            return Ok(());
        };
        for texture in &textures {
            add_texture_bytes(texture_bytes, assets.texture_charge(texture))?;
        }
        puppet.mask_textures = textures.into_iter().map(|texture| texture.pixels).collect();
        Ok(())
    }

    fn unskin(&mut self, hosted: bool, skipped: &mut Vec<String>) -> bool {
        let Some(puppet) = self.puppet.as_mut().filter(|puppet| puppet.cpu) else {
            if hosted {
                return true;
            }
            skipped.push(format!("{}: puppet material scene state unsupported", self.name));
            return false;
        };
        puppet.adopt(self.material.take());
        true
    }
}

impl Puppet {
    fn read_masks(&self, pkg: &Package, assets: &crate::effects::Assets) -> Option<Vec<Texture>> {
        let masks = &self.mesh.masks;
        if !masks.supported(self.mesh.indices.len()) {
            return None;
        }
        masks.records.iter().map(|mask| load_texture_or_asset(pkg, assets, &mask.texture)).collect()
    }

    fn adopt(&mut self, material: Option<crate::effects::material::Material>) {
        self.mesh.indices =
            material.and_then(|material| material.skin).expect("skinned layer").indices;
    }
}

pub type Attachments<'a> =
    std::collections::BTreeMap<(&'a str, &'a str), crate::puppet::AttachmentPose>;
const ATTACHMENT_FIELD: &str = "attachment";
const PARENT_FIELD: &str = "parent";
pub const CLEAR_ENABLED_FIELD: &str = "clearenabled";
pub const ZOOM_FIELD: &str = "zoom";
pub const UNIT_ZOOM: f32 = 1.0;
const ERROR_TEXTURE_SIZE: usize = 40;
const ERROR_EDGE_CELL: usize = 6;
const ERROR_INNER_CELL: usize = 7;
const ERROR_COLORS: [[u8; 4]; 2] = [[0, 0, 0, u8::MAX], [u8::MAX, u8::MAX, 0, u8::MAX]];
static NO_COMPOSITES: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
const BUILTIN_MODEL_DIRS: [&str; 2] = ["models/util/", "models/editor/"];
const ANGLES_FIELD: &str = "angles";
const SCALE_FIELD: &str = "scale";

pub type Properties = std::collections::BTreeMap<String, Vec<f32>>;

#[derive(Clone, Copy, Default)]
struct Transform {
    spatial: Option<crate::matrix::Mat4>,
    depth_scale: f32,
    affine: Affine,
    origin: (f32, f32, f32),
    scale: (f32, f32),
    angle: f32,
}

impl Transform {
    fn placed(mut self, object: &Value, spatial: bool) -> Self {
        if !spatial
            && (object.get(PARENT_FIELD).is_some() || object.get(ATTACHMENT_FIELD).is_some())
        {
            self.spatial = None;
        }
        self
    }

    fn matrix(self) -> crate::matrix::Mat4 {
        let t = self.affine;
        self.spatial.unwrap_or([
            t.a,
            t.b,
            0.0,
            0.0,
            t.c,
            t.d,
            0.0,
            0.0,
            0.0,
            0.0,
            self.depth_scale,
            0.0,
            t.x,
            t.y,
            self.origin.2,
            1.0,
        ])
    }
}

fn spatial_scene(scene: &Value, props: &Properties) -> bool {
    Camera::of(scene, props, ProjectionMode::of(scene)).is_some()
}

pub fn object_index(scene: &Value, id: &str) -> Option<usize> {
    scene.get("objects")?.as_array()?.iter().enumerate().find_map(|(index, object)| {
        let candidate =
            object.get("id").and_then(id_of).unwrap_or_else(|| format!("@object-{index}"));
        (candidate == id).then_some(index)
    })
}

fn layer_alpha(object: &Value, props: &Properties) -> f32 {
    if crate::dynamic_text::media_scripted(object.get("alpha")) {
        return 0.0;
    }
    number(object.get("alpha"), props, 1.0).clamp(0.0, 1.0)
}

fn object_transform(object: &Value, props: &Properties) -> Transform {
    let origin = vec3(object.get("origin"), props).unwrap_or((0.0, 0.0, 0.0));
    let scale = vec3(object.get(SCALE_FIELD), props).map_or((1.0, 1.0), |(x, y, _)| (x, y));
    let angles = vec3(object.get(ANGLES_FIELD), props).unwrap_or((0.0, 0.0, 0.0));
    let angle = angles.2;
    let spatial = (angles.0 != 0.0 || angles.1 != 0.0).then(|| {
        crate::matrix::model_matrix(
            [origin.0, origin.1, origin.2],
            [scale.0, scale.1, vec3(object.get(SCALE_FIELD), props).map_or(1.0, |value| value.2)],
            [angles.0, angles.1, angles.2],
        )
    });
    Transform {
        spatial,
        depth_scale: vec3(object.get(SCALE_FIELD), props).map_or(1.0, |scale| scale.2),
        origin,
        scale,
        angle,
        affine: Affine::new([origin.0, origin.1], [scale.0, scale.1], angle),
    }
}

fn aligned_center(
    center: (f32, f32),
    size: (f32, f32),
    angle: f32,
    alignment: Option<&str>,
) -> (f32, f32) {
    let Some(alignment) = alignment else {
        return center;
    };
    let mut shift = (0.0, 0.0);
    if alignment.contains("top") {
        shift.1 += size.1 * 0.5;
    } else if alignment.contains("bottom") {
        shift.1 -= size.1 * 0.5;
    }
    if alignment.contains("left") {
        shift.0 += size.0 * 0.5;
    } else if alignment.contains("right") {
        shift.0 -= size.0 * 0.5;
    }
    let (sin, cos) = angle.sin_cos();
    (center.0 + shift.0 * cos - shift.1 * sin, center.1 + shift.0 * sin + shift.1 * cos)
}

fn auto_background(scene: &Value) -> Option<&Value> {
    scene
        .get("objects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|object| object.get("image").and_then(Value::as_str).is_some())
}

fn auto_background_id(scene: &Value) -> Option<String> {
    auto_background(scene)?.get("id").and_then(id_of)
}

fn auto_canvas(scene: &Value, props: &Properties) -> Option<(f32, f32)> {
    let size = vec2_or(auto_background(scene)?.get("size"), props, (0.0, 0.0));
    (size.0 >= 1.0 && size.1 >= 1.0).then_some((size.0, size.1))
}

fn compose(parent: Transform, child: Transform) -> Transform {
    let affine = parent.affine.then(child.affine);
    let spatial = (parent.spatial.is_some() || child.spatial.is_some())
        .then(|| crate::matrix::mat4_mul(&parent.matrix(), &child.matrix()));
    Transform {
        spatial,
        affine,
        origin: (affine.x, affine.y, parent.origin.2 + child.origin.2 * parent.depth_scale),
        depth_scale: parent.depth_scale * child.depth_scale,
        scale: (parent.scale.0 * child.scale.0, parent.scale.1 * child.scale.1),
        angle: parent.angle + child.angle,
    }
}

struct Parallax {
    amount: f32,
    focus: (f32, f32),
}

impl Parallax {
    fn of(scene: &Value, canvas: (f32, f32), props: &Properties) -> Option<Self> {
        let general = scene.get("general");
        if !truthy(general.and_then(|top| top.get("cameraparallax")), props, false) {
            return None;
        }
        let amount = number(general.and_then(|top| top.get("cameraparallaxamount")), props, 0.5);
        Some(Self { amount, focus: (canvas.0 * 0.5, canvas.1 * 0.5) })
    }

    fn offset(&self, root: &Value, props: &Properties) -> (f32, f32) {
        let depth = parallax_depth(root, props);
        let origin = vec3(root.get("origin"), props).unwrap_or((0.0, 0.0, 0.0));
        (
            (origin.0 - self.focus.0) * self.amount * depth.0,
            (origin.1 - self.focus.1) * self.amount * depth.1,
        )
    }
}

fn parallax_depth(root: &Value, props: &Properties) -> (f32, f32) {
    let default = if root.get("image").is_some()
        || root.get("text").is_some()
        || root.get(PARTICLE_FIELD).is_some()
    {
        (1.0, 1.0)
    } else {
        (0.0, 0.0)
    };
    vec2_or(root.get("parallaxDepth"), props, default)
}

fn ancestor_chain<'a, Hasher: std::hash::BuildHasher>(
    object: &'a Value,
    by_id: &std::collections::HashMap<String, &'a Value, Hasher>,
) -> Vec<&'a Value> {
    let mut chain = vec![object];
    let mut seen = std::collections::HashSet::from([std::ptr::from_ref(object)]);
    let mut cursor = object;
    loop {
        let Some(parent_id) = cursor.get(PARENT_FIELD).and_then(id_of) else {
            break;
        };
        let Some(parent) = by_id.get(&parent_id) else {
            break;
        };
        if !seen.insert(std::ptr::from_ref(*parent)) {
            break;
        }
        chain.push(parent);
        cursor = parent;
    }
    chain
}

fn resolve_transform<'a>(
    object: &'a Value,
    by_id: &std::collections::HashMap<String, &'a Value>,
    props: &Properties,
    parallax: Option<&Parallax>,
    attachments: &Attachments<'_>,
) -> Transform {
    let chain = ancestor_chain(object, by_id);
    let mut out = Transform { scale: (1.0, 1.0), depth_scale: 1.0, ..Transform::default() };
    for node in chain.iter().rev() {
        if let Some(parent) = node.get(PARENT_FIELD).and_then(id_of)
            && let Some(name) = node.get(ATTACHMENT_FIELD).and_then(Value::as_str)
            && let Some(attachment) = attachments.get(&(parent.as_str(), name))
        {
            out = compose(
                out,
                Transform {
                    spatial: None,
                    depth_scale: 1.0,
                    affine: attachment.affine,
                    origin: (attachment.affine.x, attachment.affine.y, attachment.depth),
                    scale: (
                        attachment.affine.a.hypot(attachment.affine.b),
                        attachment.affine.c.hypot(attachment.affine.d),
                    ),
                    angle: attachment.affine.b.atan2(attachment.affine.a),
                },
            );
        }
        out = compose(out, object_transform(node, props));
    }
    if let Some(parallax) = parallax
        && let Some(root) = chain.last()
    {
        let (dx, dy) = parallax.offset(root, props);
        out.origin.0 += dx;
        out.origin.1 += dy;
        out.affine.x += dx;
        out.affine.y += dy;
        if let Some(matrix) = &mut out.spatial {
            let [_, _, _, _, _, _, _, _, _, _, _, _, x, y, _, _] = matrix;
            *x += dx;
            *y += dy;
        }
    }
    out
}

fn layer_mouse(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value>,
    props: &Properties,
    parallax: crate::mouse::Parallax,
    transform: Transform,
    text: bool,
) -> crate::mouse::LayerMouse {
    let chain = ancestor_chain(object, by_id);
    let root = chain.last().copied().unwrap_or(object);
    let depth = parallax_depth(root, props);
    crate::mouse::LayerMouse {
        parallax: if parallax.amount != 0.0 && parallax.influence != 0.0 {
            [depth.0, depth.1]
        } else {
            [0.0; 2]
        },
        clock: text
            .then(|| {
                object.get(TEXT_FIELD).and_then(|value| {
                    crate::mouse::Clock3d::from_text(
                        value,
                        [transform.origin.0, transform.origin.1],
                    )
                })
            })
            .flatten(),
    }
}

pub(crate) fn id_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(num) => Some(num.to_string()),
        _ => None,
    }
}

fn composite_gap(mesh: &MeshLayer, layers: &[Layer]) -> Option<String> {
    mesh.materials.iter().flat_map(|material| &material.pass.binds).find_map(|(_, bind)| {
        let crate::effects::EffectBind::LayerComposite { layer, .. } = bind else {
            return None;
        };
        match layers.iter().find(|loaded| &loaded.id == layer) {
            None => Some(format!("mesh layer composite {layer} unavailable")),
            Some(loaded)
                if loaded.material.as_ref().is_some_and(|material| !material.copyable()) =>
            {
                Some(format!("mesh layer composite {layer} provider unsupported"))
            }
            Some(_) => None,
        }
    })
}

fn reads_frame(bind: &crate::effects::EffectBind) -> bool {
    matches!(
        bind,
        crate::effects::EffectBind::SceneSoFar | crate::effects::EffectBind::SceneUnderLayer
    )
}

fn resolved_frame_gap(
    mesh: &MeshLayer,
    layers: &[Layer],
    particles: &[ParticleLayer],
) -> Option<String> {
    let samples_frame = mesh.materials.iter().any(|material| material.pass.samples(reads_frame));
    let layer_reads =
        layers.iter().filter(|layer| layer.scene_order < mesh.scene_order).any(|layer| {
            layer.copy_background
                || layer.base_scene_input
                || layer
                    .effects
                    .iter()
                    .flat_map(|effect| &effect.passes)
                    .chain(layer.material.iter().map(|material| &material.pass))
                    .flat_map(|pass| &pass.binds)
                    .any(|(_, bind)| reads_frame(bind))
        });
    let particle_reads = particles.iter().any(|particle| {
        particle.scene_order < mesh.scene_order && particle.system.grab_slot.is_some()
    });
    (samples_frame && (layer_reads || particle_reads)).then(|| RESOLVED_FRAME_GAP.to_owned())
}

fn dependencies(object: &Value) -> impl Iterator<Item = String> + '_ {
    object.get(DEPENDENCIES_FIELD).and_then(Value::as_array).into_iter().flatten().filter_map(id_of)
}

fn collect_render_target_layer_ids(
    value: &Value,
    own_id: Option<&str>,
    targets: &mut std::collections::BTreeSet<String>,
) {
    match value {
        Value::String(text) => {
            let Some((layer, suffix)) = text
                .strip_prefix("_rt_imageLayerComposite_")
                .and_then(|rest| rest.rsplit_once('_'))
            else {
                return;
            };
            if !layer.is_empty() && own_id != Some(layer) && matches!(suffix, "a" | "A" | "b" | "B")
            {
                targets.insert(layer.to_string());
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_render_target_layer_ids(value, own_id, targets);
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                collect_render_target_layer_ids(value, own_id, targets);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteFrame {
    pub uv: [f32; 4],
    pub axes: [f32; 4],
    pub rotated: bool,
    pub time: f32,
    pub image: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaTexture {
    Current,
    Previous,
}

impl MediaTexture {
    pub fn of(value: &Value) -> Option<Self> {
        if value.get("type").and_then(Value::as_str) != Some("system") {
            return None;
        }
        match value.get("name").and_then(Value::as_str)? {
            "$mediaThumbnail" => Some(Self::Current),
            "$mediaPreviousThumbnail" => Some(Self::Previous),
            _ => None,
        }
    }

    pub fn texture(self) -> Texture {
        let mut texture = solid_texture();
        texture.pixels = std::sync::Arc::new(tex::Pixels::rgba(1, 1, vec![0; 4]));
        texture.system_texture = Some(self);
        texture
    }
}

#[derive(Clone)]
pub enum Video {
    Bytes(std::sync::Arc<[u8]>),
    Range(VideoRange),
}

#[derive(Clone)]
pub struct VideoRange {
    pub file: std::sync::Arc<std::fs::File>,
    pub offset: u64,
    pub length: u64,
}

impl Video {
    pub fn resident_bytes(&self) -> usize {
        match self {
            Self::Bytes(payload) => payload.len(),
            Self::Range(_) => 0,
        }
    }
}

#[derive(Clone)]
pub struct Texture {
    pub text_batch: Option<crate::text::batch::Batch>,
    pub width: u32,
    pub height: u32,
    pub img_width: u32,
    pub img_height: u32,
    pub pixels: std::sync::Arc<tex::Pixels>,
    pub pages: Vec<std::sync::Arc<tex::Pixels>>,
    pub system_texture: Option<MediaTexture>,
    pub image_count: usize,
    pub video: Option<Video>,
    pub frames: Vec<SpriteFrame>,
    pub stream: Option<std::sync::Arc<[tex::stream::Cell]>>,
    pub clamp: bool,
    pub nearest: bool,
    pub component_mask: u8,
    pub format: tex::TexFormat,
}

fn sprite_frames(parsed: &tex::Tex, width: u32, height: u32) -> Vec<SpriteFrame> {
    let dimensions: Vec<_> = parsed
        .images
        .iter()
        .map(|image| image.first().map(|mip| (mip.width as f32, mip.height as f32)))
        .collect();
    parsed
        .frames
        .iter()
        .filter_map(|frame| {
            if [
                frame.x,
                frame.y,
                frame.width,
                frame.width_y,
                frame.height_x,
                frame.height,
                frame.frame_time,
            ]
            .iter()
            .any(|value| !value.is_finite())
            {
                return None;
            }
            let (aw, ah) = dimensions
                .get(usize::try_from(frame.image_id).ok()?)?
                .unwrap_or((width as f32, height as f32));
            if aw <= 0.0 || ah <= 0.0 {
                return None;
            }
            let (fw, fh) = frame.size();
            if fw <= 0.0 || fh <= 0.0 {
                return None;
            }
            let rotated = frame.rotated();
            let (uw, uh) = if rotated { (fh, fw) } else { (fw, fh) };
            Some(SpriteFrame {
                uv: [frame.x / aw, frame.y / ah, uw / aw, uh / ah],
                axes: [
                    frame.width / aw,
                    frame.width_y / ah,
                    frame.height_x / aw,
                    frame.height / ah,
                ],
                rotated,
                time: frame.frame_time.max(0.0),
                image: frame.image_id,
            })
        })
        .collect()
}

impl Texture {
    fn frame_bytes(&self) -> Result<usize> {
        self.frames
            .len()
            .checked_mul(std::mem::size_of::<SpriteFrame>())
            .ok_or_else(|| anyhow!("sprite metadata size overflow"))
    }

    pub fn is_static(&self) -> bool {
        self.video.is_none()
            && self.system_texture.is_none()
            && self.frames.is_empty()
            && self.pages.is_empty()
            && self.image_count == 1
    }

    pub fn payload_bytes(&self) -> usize {
        self.pixels.bytes()
            + self.pages.iter().map(|page| page.bytes()).sum::<usize>()
            + self
                .stream
                .iter()
                .flat_map(|cells| cells.iter())
                .map(tex::stream::Cell::packed_bytes)
                .sum::<usize>()
            + self.video.as_ref().map_or(0, Video::resident_bytes)
            + self.text_batch.as_ref().map_or(0, |batch| batch.pixels.bytes())
    }

    #[must_use]
    pub fn we_format(&self) -> i64 {
        i64::from(self.format.raw())
    }

    #[must_use]
    pub fn atlas_frames(&self) -> Option<&[SpriteFrame]> {
        let playable = !self.frames.is_empty()
            && self.frames.iter().all(|frame| {
                usize::try_from(frame.image).is_ok_and(|image| image < self.image_count)
            });
        playable.then_some(self.frames.as_slice())
    }

    pub fn uv_scale(&self) -> (f32, f32) {
        if !self.frames.is_empty() {
            return (1.0, 1.0);
        }
        (
            (self.img_width as f32 / self.width.max(1) as f32).clamp(0.0, 1.0),
            (self.img_height as f32 / self.height.max(1) as f32).clamp(0.0, 1.0),
        )
    }
}

fn vec3(value: Option<&Value>, props: &Properties) -> Option<(f32, f32, f32)> {
    let parts = crate::effects::bound_value(value?, props)?;
    if let [single] = parts[..] {
        return Some((single, single, single));
    }
    let mut parts = parts.into_iter();
    Some((parts.next()?, parts.next()?, parts.next().unwrap_or(0.0)))
}

fn vec2_or(value: Option<&Value>, props: &Properties, fallback: (f32, f32)) -> (f32, f32) {
    vec3(value, props).map_or(fallback, |(x, y, _)| (x, y))
}

pub(crate) fn number(value: Option<&Value>, props: &Properties, fallback: f32) -> f32 {
    let Some(value) = value else {
        return fallback;
    };
    crate::effects::bound_value(value, props)
        .and_then(|parts| parts.first().copied())
        .unwrap_or(fallback)
}

fn truthy(value: Option<&Value>, props: &Properties, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(num)) => num.as_f64().unwrap_or(1.0) != 0.0,
        Some(Value::String(text)) => text != "false" && text != "0",
        Some(value @ Value::Object(_)) => crate::effects::bound_value(value, props)
            .and_then(|parts| parts.first().copied())
            .is_none_or(|first| first != 0.0),
        _ => fallback,
    }
}

pub(crate) fn visible_chain(ancestors: &[&Value], props: &Properties) -> bool {
    ancestors.iter().all(|node| truthy(node.get(VISIBLE_FIELD), props, true))
}

fn authored_visible<Hasher: std::hash::BuildHasher>(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value, Hasher>,
    props: &Properties,
) -> bool {
    ancestor_chain(object, by_id).iter().all(|node| {
        truthy(node.get(VISIBLE_FIELD), props, true)
            && !crate::dynamic_text::media_scripted(node.get(VISIBLE_FIELD))
    })
}

pub fn object_visible<Hasher: std::hash::BuildHasher>(
    object: &Value,
    by_id: &std::collections::HashMap<String, &Value, Hasher>,
    props: &Properties,
) -> bool {
    visible_chain(&ancestor_chain(object, by_id), props)
}

pub(crate) fn tex_candidates(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for name in [raw, raw.trim_end_matches(TEXTURE_SUFFIX)] {
        let mut candidates = vec![format!("{name}{TEXTURE_SUFFIX}")];
        if !name.starts_with(MATERIALS_DIR) {
            candidates.push(format!("{MATERIALS_DIR}{name}{TEXTURE_SUFFIX}"));
        }
        for candidate in candidates {
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

const UNDECODABLE_TEXTURE: &str = "undecodable";
const MISSING_TEXTURE: &str = "missing";
const TEXTURE_SUFFIX: &str = ".tex";
pub(crate) const MATERIALS_DIR: &str = "materials/";
const INVALID_TEXTURE: &str = "invalid texture payload";
const INVALID_VIDEO: &str = "invalid video dimensions";

pub fn load_texture_bytes(bytes: &[u8]) -> Result<Texture, &'static str> {
    load_texture(bytes, None, None)
}

pub(crate) fn load_texture(
    bytes: &[u8],
    origin: Option<(std::sync::Arc<std::fs::File>, u64)>,
    variant: Option<u32>,
) -> Result<Texture, &'static str> {
    if let Some(sprite) =
        tex::stream::pack(bytes, tex::MAX_TEX_PAYLOAD_BYTES).map_err(|_| INVALID_TEXTURE)?
    {
        return streamed_texture(sprite);
    }
    if let Some((file, base)) = origin
        && let Some(span) = tex::video_span(bytes).map_err(|_| INVALID_TEXTURE)?
    {
        let video =
            Video::Range(VideoRange { file, offset: base + span.offset, length: span.length });
        return video_texture(&span.meta, video);
    }
    let mut parsed = tex::parse(bytes, variant).map_err(|_| INVALID_TEXTURE)?;
    let (img_width, img_height) =
        parsed.meta.sprite_size.unwrap_or((parsed.meta.img_width, parsed.meta.img_height));
    let img_width = u32::try_from(img_width).unwrap_or(0);
    let img_height = u32::try_from(img_height).unwrap_or(0);
    let video = if parsed.meta.flags & tex::FLAG_IS_VIDEO != 0 {
        let mip = parsed
            .images
            .first_mut()
            .and_then(|image| image.first_mut())
            .ok_or("missing video payload")?;
        if mip.data.is_empty() {
            return Err("empty video payload");
        }
        Some(Video::Bytes(std::sync::Arc::from(std::mem::take(&mut mip.data))))
    } else {
        None
    };
    let pixels = if video.is_some() {
        tex::Pixels::default()
    } else {
        tex::take_pixels(&mut parsed).ok_or(UNDECODABLE_TEXTURE)?
    };
    let (width, height) = if video.is_some() {
        let width = u32::try_from(parsed.meta.tex_width).map_err(|_| INVALID_VIDEO)?;
        let height = u32::try_from(parsed.meta.tex_height).map_err(|_| INVALID_VIDEO)?;
        tex::PixelFormat::Rgba8.level_bytes(width, height).ok_or(INVALID_VIDEO)?;
        (width, height)
    } else {
        (pixels.width(), pixels.height())
    };
    let animated = parsed.meta.flags & tex::FLAG_IS_GIF != 0;
    let img_width = if img_width == 0 {
        width
    } else if animated {
        img_width
    } else {
        img_width.min(width)
    };
    let img_height = if img_height == 0 {
        height
    } else if animated {
        img_height
    } else {
        img_height.min(height)
    };
    let frames = sprite_frames(&parsed, width, height);
    if frames.len() != parsed.meta.frame_count {
        return Err("invalid atlas frame");
    }
    let pages = if frames.is_empty() {
        Vec::new()
    } else {
        (1..parsed.images.len())
            .map(|index| tex::take_image_pixels(&mut parsed, index).map(std::sync::Arc::new))
            .collect::<Option<Vec<_>>>()
            .ok_or("undecodable atlas page")?
    };
    Ok(Texture {
        text_batch: None,
        width,
        height,
        img_width,
        img_height,
        pixels: std::sync::Arc::new(pixels),
        pages,
        system_texture: None,
        image_count: parsed.meta.image_count,
        video,
        frames,
        stream: None,
        clamp: parsed.meta.flags & tex::FLAG_CLAMP_UVS != 0,
        nearest: parsed.meta.flags & tex::FLAG_NO_INTERPOLATION != 0,
        component_mask: parsed.meta.component_mask(),
        format: if parsed.meta.free_image_format.is_some_and(|format| format >= 0) {
            tex::TexFormat::Rgba8888
        } else {
            parsed.meta.format
        },
    })
}

fn video_texture(meta: &tex::TexMeta, video: Video) -> Result<Texture, &'static str> {
    let width = u32::try_from(meta.tex_width).map_err(|_| INVALID_VIDEO)?;
    let height = u32::try_from(meta.tex_height).map_err(|_| INVALID_VIDEO)?;
    tex::PixelFormat::Rgba8.level_bytes(width, height).ok_or(INVALID_VIDEO)?;
    let (img_width, img_height) = meta.sprite_size.unwrap_or((meta.img_width, meta.img_height));
    let visible = |size: i32, full: u32| match u32::try_from(size).unwrap_or(0) {
        0 => full,
        size => size.min(full),
    };
    Ok(Texture {
        text_batch: None,
        width,
        height,
        img_width: visible(img_width, width),
        img_height: visible(img_height, height),
        pixels: std::sync::Arc::new(tex::Pixels::default()),
        pages: Vec::new(),
        system_texture: None,
        image_count: meta.image_count,
        video: Some(video),
        frames: Vec::new(),
        stream: None,
        clamp: meta.flags & tex::FLAG_CLAMP_UVS != 0,
        nearest: meta.flags & tex::FLAG_NO_INTERPOLATION != 0,
        component_mask: meta.component_mask(),
        format: if meta.free_image_format.is_some_and(|format| format >= 0) {
            tex::TexFormat::Rgba8888
        } else {
            meta.format
        },
    })
}

fn streamed_texture(sprite: tex::stream::Sprite) -> Result<Texture, &'static str> {
    let tex::stream::Sprite { tex: parsed, cells } = sprite;
    let (width, height) = (cells[0].width, cells[0].height);
    let mut first = Vec::new();
    cells[0].unpack(&mut first).map_err(|_| UNDECODABLE_TEXTURE)?;
    let frames = parsed
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            Ok(SpriteFrame {
                uv: [0.0, 0.0, 1.0, 1.0],
                axes: [1.0, 0.0, 0.0, 1.0],
                rotated: false,
                time: frame.frame_time.max(0.0),
                image: i32::try_from(index).map_err(|_| UNDECODABLE_TEXTURE)?,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    Ok(Texture {
        text_batch: None,
        width,
        height,
        img_width: width,
        img_height: height,
        pixels: std::sync::Arc::new(tex::Pixels::rgba(width, height, first)),
        pages: Vec::new(),
        system_texture: None,
        image_count: frames.len(),
        video: None,
        frames,
        stream: Some(cells.into()),
        clamp: parsed.meta.flags & tex::FLAG_CLAMP_UVS != 0,
        nearest: parsed.meta.flags & tex::FLAG_NO_INTERPOLATION != 0,
        component_mask: parsed.meta.component_mask(),
        format: parsed.meta.format,
    })
}

fn user_image_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(tex::MAX_TEXTURE_EDGE);
    limits.max_image_height = Some(tex::MAX_TEXTURE_EDGE);
    limits.max_alloc = Some(tex::MAX_RGBA_BYTES as u64);
    limits
}

fn load_user_texture(bytes: &[u8]) -> Option<Texture> {
    use image::{AnimationDecoder, ImageDecoder};
    use std::io::Cursor;

    let format = image::guess_format(bytes).ok()?;
    if format == image::ImageFormat::Gif {
        let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?;
        let (width, height) = decoder.dimensions();
        let frame_bytes = tex::PixelFormat::Rgba8.level_bytes(width, height)?;
        decoder.set_limits(user_image_limits()).ok()?;
        let mut texture = solid_texture();
        texture.width = width;
        texture.height = height;
        texture.img_width = width;
        texture.img_height = height;
        let mut used = 0usize;
        for frame in decoder.into_frames() {
            used = used.checked_add(frame_bytes)?;
            if used > tex::MAX_RGBA_BYTES {
                return None;
            }
            let frame = frame.ok()?;
            let (numer, denom) = frame.delay().numer_denom_ms();
            let pixels = frame.into_buffer();
            if pixels.dimensions() != (width, height) {
                return None;
            }
            let pixels = tex::Pixels::rgba(width, height, pixels.into_raw());
            let image = i32::try_from(texture.frames.len()).ok()?;
            texture.frames.push(SpriteFrame {
                uv: [0.0, 0.0, 1.0, 1.0],
                axes: [1.0, 0.0, 0.0, 1.0],
                rotated: false,
                time: (numer as f32 / denom.max(1) as f32 / MS_PER_SECOND)
                    .max(MIN_GIF_FRAME_SECONDS),
                image,
            });
            if image == 0 {
                texture.pixels = std::sync::Arc::new(pixels);
            } else {
                texture.pages.push(std::sync::Arc::new(pixels));
            }
        }
        if texture.frames.is_empty() {
            return None;
        }
        texture.image_count = texture.frames.len();
        return Some(texture);
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(user_image_limits());
    let image = reader.decode().ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    tex::PixelFormat::Rgba8.level_bytes(width, height)?;
    let mut texture = solid_texture();
    texture.width = width;
    texture.height = height;
    texture.img_width = width;
    texture.img_height = height;
    texture.pixels = std::sync::Arc::new(tex::Pixels::rgba(width, height, image.into_raw()));
    Some(texture)
}

pub(crate) fn load_texture_from(
    pkg: &Package,
    assets: &crate::effects::Assets,
    package: &[String],
    asset: &[String],
) -> Option<Texture> {
    let entry = package.iter().find_map(|name| pkg.entry_path(name));
    entry
        .and_then(|name| {
            let bytes = pkg.find(name)?;
            let variant = texture_variant(bytes, assets);
            assets.cached_texture(texture_cache::Key::Package(name.to_owned(), variant), || {
                load_texture(bytes, pkg.origin(name), variant).ok()
            })
        })
        .or_else(|| {
            asset.iter().filter_map(|name| assets.asset_path(name)).find_map(|path| {
                assets.cached_texture(texture_cache::Key::Asset(path.clone()), || {
                    load_texture_bytes(&assets.read_asset(&path)?).ok()
                })
            })
        })
}

fn texture_variant(bytes: &[u8], assets: &crate::effects::Assets) -> Option<u32> {
    let variants = tex::variants(bytes).ok()?;
    let held: Vec<&tex::TexVariant> =
        variants.iter().filter(|variant| variant_holds(variant, assets)).collect();
    let variant = held.first()?;
    if !variant.complete {
        tracing::warn!(index = variant.index, "tex: partial variant unsupported, using base");
        return None;
    }
    Some(variant.index)
}

fn variant_holds(variant: &tex::TexVariant, assets: &crate::effects::Assets) -> bool {
    let Ok(condition) = crate::json::parse(variant.condition.as_bytes()) else { return false };
    let Some(binding) = condition.get(VARIANT_CONDITION_FIELD) else { return false };
    if let Some(name) = binding.get(NAME_FIELD).and_then(Value::as_str) {
        assets.depend(name);
    }
    crate::effects::bound_value(&serde_json::json!({ (USER_FIELD): binding }), &assets.properties)
        .is_some_and(|values| values.first().is_some_and(|value| *value != 0.0))
}

pub(crate) fn load_texture_or_asset(
    pkg: &Package,
    assets: &crate::effects::Assets,
    raw: &str,
) -> Option<Texture> {
    let candidates = tex_candidates(raw);
    load_texture_from(pkg, assets, &candidates, &candidates)
}

fn texture_animation_times(
    pkg: &Package,
    assets: &crate::effects::Assets,
    raw: &str,
) -> Option<Vec<f32>> {
    let candidates = tex_candidates(raw);
    for candidate in &candidates {
        if let Some(bytes) = pkg.find(candidate) {
            return tex::animation_times(bytes).ok();
        }
    }
    candidates
        .iter()
        .find_map(|candidate| assets.read_bytes(candidate))
        .and_then(|bytes| tex::animation_times(&bytes).ok())
}

fn sprite_metadata(
    scene: &Value,
    pkg: &Package,
    assets: &crate::effects::Assets,
) -> Vec<(usize, Vec<f32>)> {
    scene
        .get(OBJECTS_FIELD)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(slot, object)| {
            let source = model_texture(pkg, assets, object)?;
            if !source.spritesheet || source.user.is_some() {
                return None;
            }
            let times = texture_animation_times(pkg, assets, source.path.as_deref()?)?;
            (!times.is_empty()).then_some((slot, times))
        })
        .collect()
}

fn asset_json(
    pkg: &Package,
    assets: &crate::effects::Assets,
    path: &str,
) -> Option<serde_json::Value> {
    pkg.find_json(path)
        .ok()
        .flatten()
        .or_else(|| assets.read(path).and_then(|text| crate::json::parse(text.as_bytes()).ok()))
}

fn error_cell(index: usize) -> usize {
    if index < ERROR_EDGE_CELL { 0 } else { 1 + (index - ERROR_EDGE_CELL) / ERROR_INNER_CELL }
}

fn error_image_size() -> (f32, f32) {
    let cells = (error_cell(ERROR_TEXTURE_SIZE - 1) + 1) as f32;
    (cells, cells)
}

pub fn error_texture() -> Texture {
    let pixels = (0..ERROR_TEXTURE_SIZE)
        .flat_map(|y| (0..ERROR_TEXTURE_SIZE).map(move |x| (x, y)))
        .flat_map(|(x, y)| ERROR_COLORS[(error_cell(x) + error_cell(y)) % ERROR_COLORS.len()])
        .collect();
    let side = ERROR_TEXTURE_SIZE as u32;
    Texture {
        width: side,
        height: side,
        img_width: side,
        img_height: side,
        pixels: std::sync::Arc::new(tex::Pixels::rgba(side, side, pixels)),
        nearest: true,
        ..solid_texture()
    }
}

pub fn solid_texture() -> Texture {
    Texture {
        text_batch: None,
        width: 1,
        height: 1,
        img_width: 1,
        img_height: 1,
        pixels: std::sync::Arc::new(tex::Pixels::rgba(1, 1, vec![255, 255, 255, 255])),
        video: None,
        pages: Vec::new(),
        system_texture: None,
        image_count: 1,
        frames: Vec::new(),
        stream: None,
        clamp: true,
        nearest: false,
        component_mask: 0,
        format: tex::TexFormat::Rgba8888,
    }
}

#[derive(Default)]
struct UtilModel {
    passthrough: bool,
    fullscreen: bool,
    project_sized: bool,
    flat: bool,
    scene_input: bool,
    untextured: bool,
    autosize: bool,
}

fn untextured(material: &Value) -> bool {
    material
        .get(EFFECT_PASSES_FIELD)
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .is_some_and(|pass| {
            pass.get(crate::effects::TEXTURES_FIELD)
                .and_then(Value::as_array)
                .and_then(|textures| textures.first())
                .and_then(crate::effects::texture_name)
                .is_none_or(str::is_empty)
        })
}

fn util_model(pkg: &Package, assets: &crate::effects::Assets, object: &Value) -> UtilModel {
    let Some(model_path) = object.get(IMAGE_FIELD).and_then(Value::as_str) else {
        return UtilModel::default();
    };
    let Some(model) = asset_json(pkg, assets, model_path) else {
        return UtilModel::default();
    };
    let flag = |key: &str| model.get(key) == Some(&Value::Bool(true));
    let material = crate::effects::material::Image::load(pkg, assets, object).ok().flatten();
    let flat = material.as_ref().is_some_and(|image| material_is_flat(&image.material));
    let own_id = object.get(ID_FIELD).and_then(id_of);
    UtilModel {
        passthrough: flag(MODEL_PASSTHROUGH_FIELD),
        fullscreen: flag(MODEL_FULLSCREEN_FIELD),
        project_sized: flag(MODEL_AUTOSIZE_FIELD) && flag(MODEL_PROJECT_LAYER_FIELD),
        flat,
        scene_input: material.as_ref().is_some_and(|image| {
            crate::effects::material::scene_input(&image.material, own_id.as_deref()).is_some()
        }),
        untextured: material.as_ref().is_some_and(|image| untextured(&image.material)),
        autosize: flag(MODEL_AUTOSIZE_FIELD),
    }
}

fn model_uv_padding(pkg: &Package, assets: &crate::effects::Assets, model_path: &str) -> bool {
    let Some(model) = asset_json(pkg, assets, model_path) else {
        return false;
    };
    !MODEL_FULL_UV_FIELDS.iter().any(|key| model.get(key) == Some(&Value::Bool(true)))
}

fn material_is_flat(material: &Value) -> bool {
    material
        .get("passes")
        .and_then(Value::as_array)
        .and_then(|passes| passes.first())
        .and_then(|pass| pass.get("shader"))
        .and_then(Value::as_str)
        .is_some_and(|shader| shader == "flat")
}

struct ImageSource {
    path: Option<String>,
    user: Option<String>,
    media: Option<MediaTexture>,
    spritesheet: bool,
    autosize: bool,
}

fn material_texture(material: &Value) -> Option<ImageSource> {
    let passes = material.get("passes")?.as_array()?;
    let pass = passes.first()?;
    let slot = pass
        .get(crate::effects::USER_TEXTURES_FIELD)
        .and_then(Value::as_array)
        .and_then(|textures| textures.first());
    let media = slot.and_then(MediaTexture::of);
    let path = pass
        .get("textures")
        .and_then(Value::as_array)
        .and_then(|textures| textures.first())
        .and_then(crate::effects::texture_name)
        .filter(|name| {
            !name.is_empty()
                && !name.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("_rt_"))
        })
        .map(str::to_owned);
    if path.is_none() && media.is_none() {
        return None;
    }
    let user = slot.and_then(crate::effects::texture_name).map(str::to_owned);
    let spritesheet = crate::effects::combos_of(pass.get(crate::effects::COMBOS_FIELD))
        .get(crate::effects::SPRITESHEET_COMBO)
        .is_some_and(|flag| *flag != 0);
    Some(ImageSource { path, user, media, spritesheet, autosize: false })
}

fn legacy_material_tint(
    pkg: &Package,
    assets: &crate::effects::Assets,
    object: &Value,
) -> Option<([f32; 3], f32)> {
    let image = crate::effects::material::Image::load(pkg, assets, object).ok()??;
    let pass = image.material.get("passes")?.as_array()?.first()?;
    let shader = pass.get("shader")?.as_str()?;
    let versioned = pass.get("combos").and_then(|combos| combos.get("VERSION")).is_some();
    if !(shader == "genericimage" || (shader == "genericimage2" && !versioned)) {
        return None;
    }
    let constants = pass.get(crate::effects::CONSTANT_SHADER_VALUES);
    let constant = |key: &str| {
        constants
            .and_then(|values| values.get(key))
            .and_then(Value::as_f64)
            .map(|value| value as f32)
    };
    let brightness = constant("Brightness").or_else(|| constant("Bright")).unwrap_or(1.0);
    let alpha = constant("Alpha").unwrap_or(1.0).clamp(0.0, 1.0);
    Some(([brightness; 3], alpha))
}

fn model_texture(
    pkg: &Package,
    assets: &crate::effects::Assets,
    object: &Value,
) -> Option<ImageSource> {
    let image = crate::effects::material::Image::load(pkg, assets, object).ok()??;
    let mut source = material_texture(&image.material)?;
    source.autosize = image.model.get(MODEL_AUTOSIZE_FIELD) == Some(&Value::Bool(true));
    Some(source)
}

fn model_puppet(pkg: &Package, path: &str) -> Result<Option<crate::puppet::Mesh>> {
    let Some(model) = pkg.find_json(path)? else {
        return Ok(None);
    };
    let Some(puppet) = model.get("puppet").and_then(Value::as_str) else {
        return Ok(None);
    };
    let bytes = pkg.find(puppet).ok_or_else(|| anyhow!("missing puppet {puppet}"))?;
    crate::puppet::parse(bytes).map(Some).map_err(|err| anyhow!("parse puppet {puppet}: {err}"))
}

const ANIMATION_ID_FIELD: &str = "animation";
const USER_FIELD: &str = "user";
const ANIMATION_VALUE_FIELD: &str = "value";
const ANIMATION_LAYERS_FIELD: &str = "animationlayers";
const ANIMATION_RATE_FIELD: &str = "rate";
const ANIMATION_BLEND_FIELD: &str = "blend";

fn puppet_animation_layer(
    layer: &Value,
    props: &Properties,
) -> Option<crate::puppet::AnimationLayer> {
    let id = animation_id(layer.get(ANIMATION_ID_FIELD)?, props)?;
    Some(crate::puppet::AnimationLayer {
        visible: truthy(layer.get(VISIBLE_FIELD), props, true),
        id,
        rate: number(layer.get(ANIMATION_RATE_FIELD), props, 1.0).max(0.0),
        blend: number(layer.get(ANIMATION_BLEND_FIELD), props, 1.0).clamp(0.0, 1.0),
        additive: truthy(layer.get("additive"), props, false),
    })
}

fn puppet_animation_layers(
    object: &Value,
    props: &Properties,
) -> Vec<crate::puppet::AnimationLayer> {
    object
        .get(ANIMATION_LAYERS_FIELD)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|layer| puppet_animation_layer(layer, props))
        .collect()
}

fn model_animation_layers(
    object: &Value,
    props: &Properties,
) -> std::result::Result<Vec<crate::puppet::AnimationLayer>, String> {
    let bound =
        object.get(ANIMATION_LAYERS_FIELD).and_then(Value::as_array).is_some_and(|layers| {
            layers.iter().any(|layer| {
                layer.as_object().is_some_and(|fields| fields.values().any(Value::is_object))
            })
        });
    if bound {
        return Err("bound skeletal animation unsupported".to_owned());
    }
    let layers = puppet_animation_layers(object, props);
    if layers.len() > crate::puppet::MAX_ANIMATION_LAYERS {
        return Err("skeletal animation layer budget exceeded".to_owned());
    }
    if layers.iter().any(|layer| !layer.rate.is_finite() || !layer.blend.is_finite()) {
        return Err("skeletal animation layer invalid".to_owned());
    }
    Ok(layers)
}

fn puppet_playback(
    scene: &Value,
    pkg: &Package,
    props: &Properties,
) -> crate::puppet::playback::Playback {
    let mut configured = Vec::new();
    let mut available = Vec::new();
    let mut rigs = Vec::new();
    for (slot, object) in
        scene.get(OBJECTS_FIELD).and_then(Value::as_array).into_iter().flatten().enumerate()
    {
        let Some(path) = object.get(IMAGE_FIELD).and_then(Value::as_str) else { continue };
        let Ok(Some(mesh)) = model_puppet(pkg, path) else { continue };
        let object_id =
            object.get(ID_FIELD).and_then(id_of).unwrap_or_else(|| format!("@object-{slot}"));
        for animation in &mesh.animations {
            if !animation.name.is_empty() {
                available.push(crate::puppet::playback::Clip {
                    slot,
                    object: object_id.clone(),
                    name: animation.name.clone(),
                    animation: animation.id,
                    fps: animation.fps,
                    length: animation.duration as f32,
                    rate: 1.0,
                });
            }
        }
        if let Some(authored) = object.get(ANIMATION_LAYERS_FIELD).and_then(Value::as_array) {
            configured.extend(authored.iter().enumerate().filter_map(|(position, raw)| {
                let layer = puppet_animation_layer(raw, props)?;
                let clip = mesh.animations.iter().find(|clip| clip.id == layer.id)?;
                Some((
                    crate::puppet::playback::Clip {
                        slot,
                        object: object_id.clone(),
                        name: raw.get(NAME_FIELD).and_then(Value::as_str).unwrap_or("").to_owned(),
                        animation: clip.id,
                        fps: clip.fps,
                        length: clip.duration as f32,
                        rate: layer.rate,
                    },
                    number(raw.get(ANIMATION_BLEND_FIELD), props, 1.0),
                    layer.additive,
                    truthy(raw.get(VISIBLE_FIELD), props, true),
                    position,
                ))
            }));
        }
        rigs.push((slot, object_id, mesh));
    }
    let mut playback = crate::puppet::playback::Playback::with_catalog(
        configured.iter().map(|(clip, ..)| clip).cloned().collect(),
        available,
    );
    for (index, (_, blend, additive, visible, position)) in configured.into_iter().enumerate() {
        playback.configure(index, blend, additive, visible);
        playback.set_authored(index, position);
    }
    for (slot, object, mesh) in rigs {
        playback.install_rig(slot, object, mesh);
    }
    playback
}

fn animation_id(raw: &Value, props: &Properties) -> Option<u32> {
    let value = match raw {
        Value::Number(value) => value.as_f64()?,
        Value::String(value) => value.parse::<f64>().ok()?,
        Value::Object(fields) => match fields.get(USER_FIELD) {
            Some(Value::String(key)) if props.contains_key(&key.to_ascii_lowercase()) => {
                let values = crate::effects::bound_value(raw, props)?;
                let [value] = values.as_slice() else {
                    return None;
                };
                f64::from(*value)
            }
            None | Some(Value::String(_)) => {
                return animation_id(fields.get(ANIMATION_VALUE_FIELD)?, props);
            }
            _ => return None,
        },
        _ => return None,
    };
    (value.is_finite() && value.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&value))
        .then_some(value as u32)
}

fn supported_layers(
    object: &Value,
    props: &Properties,
    selected: &[crate::puppet::AnimationLayer],
) -> bool {
    let Some(raw) = object.get(ANIMATION_LAYERS_FIELD) else {
        return selected.is_empty();
    };
    let Some(layers) = raw.as_array() else {
        return false;
    };
    layers.len() == selected.len()
        && layers.iter().zip(selected).all(|(layer, selected)| {
            let blend = number(layer.get(ANIMATION_BLEND_FIELD), props, 1.0);
            let rate = number(layer.get(ANIMATION_RATE_FIELD), props, 1.0);
            layer.get(ANIMATION_ID_FIELD).and_then(|value| animation_id(value, props))
                == Some(selected.id)
                && rate.is_finite()
                && rate >= 0.0
                && blend.is_finite()
                && (0.0..=1.0).contains(&blend)
        })
}

pub fn load(pkg: &Package) -> Result<SceneModel> {
    let configured = std::env::var("SKWD_WE_ASSETS").ok();
    load_with(pkg, &crate::effects::Assets::discover(configured.as_deref()))
}

pub fn load_from_dir(pkg: &Package, dir: &std::path::Path) -> Result<SceneModel> {
    load_from_dir_with(pkg, dir, &Properties::new())
}

pub fn load_from_dir_with(
    pkg: &Package,
    dir: &std::path::Path,
    overrides: &Properties,
) -> Result<SceneModel> {
    load_from_dir_with_storage(pkg, dir, overrides, None, None)
}

pub fn load_from_dir_with_storage(
    pkg: &Package,
    dir: &std::path::Path,
    overrides: &Properties,
    storage: Option<crate::script::Storage>,
    screen_resolution: Option<[u32; 2]>,
) -> Result<SceneModel> {
    let project_value = crate::source::read(&dir.join("project.json"))
        .ok()
        .and_then(|bytes| crate::json::parse(&bytes).ok())
        .unwrap_or(Value::Null);
    let project =
        if project_value.get("dependency").is_some() || project_value.get("preset").is_some() {
            Some(paper_control::we_project::Project::resolve(dir)?)
        } else {
            None
        };
    let document = project.as_ref().map_or(&project_value, |project| &project.document);
    crate::source::project(dir, document);
    let properties = crate::effects::parse_properties(document);
    let configured = std::env::var("SKWD_WE_ASSETS").ok();
    let mut assets = crate::effects::Assets::discover(configured.as_deref());
    let source = project.as_ref().map_or(dir, |project| project.source.as_path());
    let directories = project.as_ref().map_or(&[][..], |project| project.directories.as_slice());
    crate::source::project_files(directories)?;
    assets = assets.with_scene_textures(document, source, directories)?;
    assets = assets.with_properties(properties).with_overrides(overrides.clone());
    assets = assets.with_custom_shaders(document)?;
    load_scene(pkg, &assets, document, storage, screen_resolution)
}

pub fn load_with(pkg: &Package, assets: &crate::effects::Assets) -> Result<SceneModel> {
    load_scene(pkg, assets, &Value::Null, None, None)
}

fn load_scene(
    pkg: &Package,
    assets: &crate::effects::Assets,
    project: &Value,
    storage: Option<crate::script::Storage>,
    screen_resolution: Option<[u32; 2]>,
) -> Result<SceneModel> {
    let _compilation = crate::shader::CompilationSession::new().ok();
    load_pass(pkg, assets, project, storage, screen_resolution, LoadPass::default())
}

fn load_pass(
    pkg: &Package,
    assets: &crate::effects::Assets,
    project: &Value,
    storage: Option<crate::script::Storage>,
    screen_resolution: Option<[u32; 2]>,
    pass: LoadPass,
) -> Result<SceneModel> {
    let result = load_with_project(pkg, assets, project, storage.clone(), screen_resolution, pass);
    let next = match &result {
        Err(error) if error.downcast_ref::<ScriptImageOverrun>().is_none() => return result,
        Err(error) if !pass.defer_hidden && error.downcast_ref::<HiddenLoaded>().is_some() => {
            tracing::info!("scene: deferring hidden layers and reloading: {error:#}");
            LoadPass { defer_hidden: true, ..pass }
        }
        Err(error) if !pass.without_scripts => {
            tracing::warn!("SceneScript: scripts not run: {error:#}");
            LoadPass { without_scripts: true, ..pass }
        }
        _ => return result,
    };
    load_pass(pkg, assets, project, storage, screen_resolution, next)
}

#[derive(Clone, Copy)]
pub(crate) struct LoadPass {
    pub(crate) defer_hidden: bool,
    pub(crate) without_scripts: bool,
    pub(crate) texture_budget: usize,
}

impl Default for LoadPass {
    fn default() -> Self {
        Self {
            defer_hidden: false,
            without_scripts: false,
            texture_budget: MAX_SCENE_TEXTURE_BYTES,
        }
    }
}

struct ImageGeometry {
    canvas: (f32, f32),
    background: Option<String>,
    parallax: Option<Parallax>,
    mouse: crate::mouse::Parallax,
}

impl ImageGeometry {
    fn of(scene: &Value, props: &Properties) -> Self {
        let general = scene.get(GENERAL_FIELD);
        let ortho = general.and_then(|top| top.get(PROJECTION_FIELD));
        let declared = (
            number(ortho.and_then(|proj| proj.get(PROJECTION_WIDTH)), props, 1920.0).max(1.0),
            number(ortho.and_then(|proj| proj.get(PROJECTION_HEIGHT)), props, 1080.0).max(1.0),
        );
        let auto = truthy(ortho.and_then(|proj| proj.get(PROJECTION_AUTO)), props, false);
        let canvas = if auto { auto_canvas(scene, props).unwrap_or(declared) } else { declared };
        let background = auto.then(|| auto_background_id(scene)).flatten();
        let parallax = Parallax::of(scene, canvas, props);
        let mouse = crate::mouse::Parallax {
            amount: parallax.as_ref().map_or(0.0, |p| p.amount),
            influence: if parallax.is_some() {
                number(general.and_then(|top| top.get("cameraparallaxmouseinfluence")), props, 0.5)
            } else {
                0.0
            },
            delay: number(general.and_then(|top| top.get("cameraparallaxdelay")), props, 0.1),
            camera_offset: parallax
                .as_ref()
                .map_or([0.0; 2], |p| [p.focus.0 / canvas.0 - 0.5, p.focus.1 / canvas.1 - 0.5]),
        };
        Self { canvas, background, parallax, mouse }
    }
}

const TEXT_FIELD: &str = "text";

const EFFECTS_FIELD: &str = "effects";
const EFFECT_FILE_FIELD: &str = "file";
const EFFECT_PASSES_FIELD: &str = "passes";
const EFFECT_CONDITIONS_FIELD: &str = "conditions";
const EFFECT_MATERIAL_FIELD: &str = "material";
const FRAGMENT_SHADER_EXT: &str = "frag";

const TEXT_MEDIA_FIELDS: &[&str] = &[PARTICLE_FIELD, "sound", shape::FIELD];

const DYNAMIC_BINDING_FIELDS: &[&str] = &["script", "scriptproperties", "user", "animation"];

fn dynamic_bindings(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(dynamic_bindings),
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            DYNAMIC_BINDING_FIELDS.contains(&key.as_str()) || dynamic_bindings(value)
        }),
        _ => false,
    }
}

pub struct PreparedImage {
    pub layer: Layer,
    pub layout: script::Layout,
}

pub struct ImageResources {
    font_source: std::sync::Arc<crate::text::script::Source>,
    package: Package,
    assets: crate::effects::Assets,
    bytes: std::cell::Cell<usize>,
    revision: std::cell::Cell<u64>,
    images: std::cell::RefCell<std::collections::HashMap<usize, usize>>,
    model_layers: std::cell::RefCell<std::collections::HashMap<usize, usize>>,
    models: std::cell::RefCell<std::collections::HashMap<usize, std::rc::Rc<mesh::Mesh>>>,
    model_bytes: std::cell::RefCell<Vec<(std::rc::Weak<mesh::Mesh>, usize)>>,
}

#[derive(serde::Serialize)]
pub(crate) struct EffectMaterialInfo {
    count: usize,
    materials: Vec<MaterialInfo>,
    clear_functions: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(serde::Serialize)]
struct MaterialInfo {
    source: usize,
    uniforms: std::collections::BTreeMap<String, crate::shader::MaterialBinding>,
}

impl ImageResources {
    pub fn light_cookies(&self, lights: &[lighting::Light]) -> Result<Option<lighting::Cookies>> {
        lighting::cookie::load(&self.package, &self.assets, lights)
    }

    pub(crate) fn puppet_attachment_poses(
        &self,
        path: &str,
    ) -> Result<Vec<(String, crate::puppet::AttachmentPose)>> {
        let Some(mesh) = model_puppet(&self.package, path)? else { return Ok(Vec::new()) };
        let poses = crate::puppet::skin(&mesh, &[], 0.0).attachments;
        Ok(mesh.attachments.into_iter().map(|attachment| attachment.name).zip(poses).collect())
    }
    pub fn new(package: &Package, assets: &crate::effects::Assets) -> Self {
        Self {
            font_source: std::sync::Arc::new(crate::text::script::Source::new(package, assets)),
            package: package.clone(),
            assets: assets.snapshot(),
            bytes: std::cell::Cell::new(0),
            revision: std::cell::Cell::new(0),
            images: std::cell::RefCell::default(),
            model_layers: std::cell::RefCell::default(),
            models: std::cell::RefCell::default(),
            model_bytes: std::cell::RefCell::default(),
        }
    }

    fn effect_json(&self, assets: &crate::effects::Assets, path: &str) -> Result<Value> {
        self.package
            .find_json(path)?
            .or_else(|| assets.read(path).and_then(|text| crate::json::parse(text.as_bytes()).ok()))
            .ok_or_else(|| anyhow!("effect resource unavailable: {path}"))
    }

    pub(crate) fn effect_materials(&self, object: &Value) -> Result<EffectMaterialInfo> {
        let file = object[EFFECTS_FIELD][0][EFFECT_FILE_FIELD]
            .as_str()
            .ok_or_else(|| anyhow!("effect file missing"))?;
        let definition = crate::effects::definition(
            &self.package,
            &self.assets,
            file,
            &object[EFFECTS_FIELD][0],
        )
        .ok_or_else(|| anyhow!("effect resource unavailable: {file}"))?;
        let assets = self.assets.with_effect(file);
        let passes = definition[EFFECT_PASSES_FIELD]
            .as_array()
            .ok_or_else(|| anyhow!("effect passes missing"))?;
        let combos = crate::effects::instance_combos(
            &self.package,
            &assets,
            passes,
            object[EFFECTS_FIELD][0].get(EFFECT_PASSES_FIELD),
        );
        let clear_functions = crate::effects::clear_functions(&definition);
        let mut materials = Vec::new();
        for (source, pass) in passes.iter().enumerate() {
            if !crate::effects::conditions_hold(pass.get(EFFECT_CONDITIONS_FIELD), &combos) {
                continue;
            }
            let Some(path) = pass[EFFECT_MATERIAL_FIELD].as_str() else { continue };
            let material = self.effect_json(&assets, path)?;
            let shader =
                crate::effects::customshader::resolve(&material[EFFECT_PASSES_FIELD][0], &assets)
                    .ok_or_else(|| anyhow!("effect material shader missing"))?;
            let fragment =
                crate::effects::load_shader(&self.package, &assets, shader, FRAGMENT_SHADER_EXT)
                    .ok_or_else(|| anyhow!("effect fragment shader missing"))?;
            let mut uniforms = crate::shader::material_bindings(&fragment);
            let names = uniforms
                .iter()
                .map(|(name, binding)| (binding.uniform.as_str(), Some(name.as_str())));
            let values = crate::effects::material_values(
                &material[EFFECT_PASSES_FIELD][0],
                object[EFFECTS_FIELD][0][EFFECT_PASSES_FIELD].get(source),
                &assets,
                &names,
            );
            for binding in uniforms.values_mut() {
                if let Some(value) = values.get(&binding.uniform) {
                    let mut value = value.clone();
                    value.resize(binding.width, value.last().copied().unwrap_or_default());
                    binding.value = Some(value);
                }
            }
            materials.push(MaterialInfo { source, uniforms });
        }
        Ok(EffectMaterialInfo { count: passes.len(), materials, clear_functions })
    }

    pub(crate) fn video_texture(&self, object: &Value) -> bool {
        let Some(path) =
            model_texture(&self.package, &self.assets, object).and_then(|source| source.path)
        else {
            return false;
        };
        tex_candidates(&path).iter().any(|path| {
            let owned;
            let bytes = if let Some(bytes) = self.package.find(path) {
                bytes
            } else {
                owned = self.assets.read_bytes(path);
                let Some(bytes) = owned.as_deref() else { return false };
                bytes
            };
            tex::parse_meta(bytes).is_ok_and(|meta| meta.flags & tex::FLAG_IS_VIDEO != 0)
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    fn image_context<'a>(
        &'a self,
        scene: &Value,
        assets: &'a crate::effects::Assets,
        by_id: &'a std::collections::HashMap<String, &'a Value>,
        geometry: &'a ImageGeometry,
        force_material: bool,
    ) -> ImageContext<'a> {
        ImageContext {
            scripted: true,
            force_material,
            font_source: Some(&self.font_source),
            hdr: hdr::enabled(scene.get(GENERAL_FIELD), &assets.properties),
            pkg: &self.package,
            assets,
            by_id,
            canvas: geometry.canvas,
            parallax: geometry.parallax.as_ref(),
            mouse: geometry.mouse,
            background: geometry.background.as_deref(),
            created: &[],
            composites: &NO_COMPOSITES,
            spatial: spatial_scene(scene, &assets.properties),
        }
    }

    pub fn bloom_layer(
        &self,
        scene: &Value,
        properties: &Properties,
        view: bloom::View,
    ) -> Result<Option<Layer>> {
        let mut assets = self.assets.snapshot();
        assets.properties.clone_from(properties);
        let (layer, skipped) = bloom::load(bloom::Source {
            package: &self.package,
            assets: &assets,
            general: scene.get(GENERAL_FIELD),
            view,
        });
        anyhow::ensure!(skipped.is_empty(), skipped.join(DIAGNOSTIC_SEPARATOR));
        Ok(layer)
    }

    pub fn base_material_layer(
        &self,
        scene: &Value,
        properties: &Properties,
        slot: usize,
    ) -> Result<Option<Layer>> {
        let objects = scene
            .get(OBJECTS_FIELD)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("material scene has no objects"))?;
        let object = objects.get(slot).ok_or_else(|| anyhow!("material layer missing"))?;
        if object.get(IMAGE_FIELD).is_none() {
            return Ok(None);
        }
        let by_id = objects
            .iter()
            .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
            .collect();
        let mut assets = self.assets.snapshot();
        assets.properties.clone_from(properties);
        let geometry = ImageGeometry::of(scene, properties);
        let context = self.image_context(scene, &assets, &by_id, &geometry, true);
        let mut skipped = Vec::new();
        let mut bytes = 0;
        let layer = context.load(object, slot, &mut skipped, &mut bytes)?;
        anyhow::ensure!(skipped.is_empty(), skipped.join(DIAGNOSTIC_SEPARATOR));
        Ok(layer.filter(|layer| layer.material.is_some()))
    }

    fn changed(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    fn reserve_image_slot(&self, slot: usize, bytes: usize) -> Result<()> {
        let mut total = self.bytes.get();
        add_texture_bytes(&mut total, bytes)?;
        self.check_model_bytes(total, 0)?;
        let mut images = self.images.borrow_mut();
        if images.contains_key(&slot) {
            return Err(anyhow!("dynamic image slot is already reserved"));
        }
        images.insert(slot, bytes);
        self.bytes.set(total);
        self.changed();
        Ok(())
    }

    pub fn load(&self, scene: &Value, slot: usize) -> Result<PreparedImage> {
        let objects = scene
            .get(OBJECTS_FIELD)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("dynamic image scene has no objects"))?;
        let object = objects.get(slot).ok_or_else(|| anyhow!("dynamic image slot is missing"))?;
        if objects.len() > MAX_SCENE_OBJECTS {
            return Err(anyhow!("dynamic image exceeds the scene object budget"));
        }
        let by_id = objects
            .iter()
            .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
            .collect();
        let geometry = ImageGeometry::of(scene, &self.assets.properties);
        let context = self.image_context(scene, &self.assets, &by_id, &geometry, false);
        let mut skipped = Vec::new();
        let mut bytes = 0;
        let layer = if object.get(TEXT_FIELD).is_some() && object.get(IMAGE_FIELD).is_none() {
            if TEXT_MEDIA_FIELDS.iter().any(|field| object.get(field).is_some()) {
                return Err(anyhow!("dynamic text competing media are unsupported"));
            }
            if dynamic_bindings(object) {
                return Err(anyhow!("dynamic text bindings are unsupported"));
            }
            if object
                .get(EFFECTS_FIELD)
                .is_some_and(|effects| effects.as_array().is_none_or(|effects| !effects.is_empty()))
            {
                return Err(anyhow!("dynamic text effects are unsupported"));
            }
            context.text(object, slot, &mut skipped, &mut bytes)?
        } else {
            context.load(object, slot, &mut skipped, &mut bytes)?
        };
        if !skipped.is_empty() {
            return Err(anyhow!("dynamic image: {}", skipped.join(DIAGNOSTIC_SEPARATOR)));
        }
        let layer = layer.ok_or_else(|| anyhow!("dynamic image has no renderable content"))?;
        if layer.material.is_some()
            || !layer.effects.is_empty()
            || layer.puppet.is_some()
            || layer.passthrough
            || layer.texture.system_texture.is_some()
            || !layer.texture.pages.is_empty()
            || layer.texture.stream.is_some()
            || (layer.texture.frames.len() > 1
                && (layer.atlas_frames().is_none()
                    || !layer
                        .texture
                        .frames
                        .iter()
                        .map(|frame| frame.time)
                        .sum::<f32>()
                        .is_finite()))
        {
            return Err(anyhow!(
                "dynamic image requires unsupported effects, puppet, or animated resources"
            ));
        }
        add_texture_bytes(&mut bytes, layer.texture.frame_bytes()?)?;
        self.reserve_image_slot(slot, bytes)?;
        let layout = script::Layout::new(&layer, scene, geometry.canvas, &self.assets.properties);
        Ok(PreparedImage { layer, layout })
    }

    pub fn load_particle(&self, scene: &Value, slot: usize) -> Result<ParticleLayer> {
        let objects = scene
            .get(OBJECTS_FIELD)
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("dynamic particle scene has no objects"))?;
        let object =
            objects.get(slot).ok_or_else(|| anyhow!("dynamic particle slot is missing"))?;
        if objects.len() > MAX_SCENE_OBJECTS || dynamic_bindings(object) {
            return Err(anyhow!("dynamic particle object is invalid"));
        }
        let path = object
            .get(PARTICLE_FIELD)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("dynamic particle path is missing"))?;
        let id = object
            .get(ID_FIELD)
            .and_then(id_of)
            .ok_or_else(|| anyhow!("dynamic particle id missing"))?;
        if object.get(TEXT_FIELD).is_some() {
            return Err(anyhow!("dynamic text competing media are unsupported"));
        }
        if [IMAGE_FIELD, MESH_FIELD, SOUND_FIELD].iter().any(|field| object.get(*field).is_some()) {
            return Err(anyhow!("dynamic particle has competing media"));
        }
        let props = &self.assets.properties;
        let projection = ProjectionMode::of(scene);
        let (lighting, _) = lighting::load(scene, props);
        let assets = self.assets.with_scene(crate::effects::SceneContext {
            projection: Some(projection),
            lighting,
            fog: fog::Config::load(scene, props),
        });
        let context = crate::particles::LoadContext {
            pkg: &self.package,
            assets: &assets,
            defaults: crate::particles::ParticleDefaults::for_projection(projection),
            viewport: crate::particles::BoundsViewport::of(scene),
            bounds_static: false,
            version: scene
                .get(SCENE_VERSION_FIELD)
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0),
        };
        let (system, skipped) = crate::particles::load(&context, object, path);
        if !skipped.is_empty() {
            return Err(anyhow!("dynamic particle: {}", skipped.join(DIAGNOSTIC_SEPARATOR)));
        }
        let mut system = system.ok_or_else(|| anyhow!("dynamic particle unsupported"))?;
        let by_id = objects
            .iter()
            .filter_map(|object| Some((object.get(ID_FIELD).and_then(id_of)?, object)))
            .collect();
        let geometry = ImageGeometry::of(scene, props);
        let state = script::particle_frame(
            object,
            &by_id,
            props,
            geometry.parallax.as_ref(),
            &Attachments::new(),
        );
        state.apply(&mut system);
        if system.validate_bounds(object) {
            return Err(anyhow!("dynamic particle collision bounds transform unsupported"));
        }
        if system.texture.is_none() {
            return Err(anyhow!("dynamic particle texture missing"));
        }
        let mut bytes = 0;
        charge_particle_textures(&self.assets, &system, &mut bytes)?;
        self.reserve_image_slot(slot, bytes)?;
        Ok(ParticleLayer {
            parallax: state.parallax,
            id,
            visible: truthy(object.get(VISIBLE_FIELD), props, true),
            depth: system.origin.2,
            scene_order: slot,
            system,
        })
    }

    pub fn initialize(&self, bytes: usize, slots: &[(usize, usize)]) -> Result<()> {
        let mut total = 0;
        add_texture_bytes(&mut total, bytes)?;
        let mut dynamic = 0;
        for (_, payload) in slots {
            add_texture_bytes(&mut dynamic, *payload)?;
        }
        if dynamic > total {
            return Err(anyhow!("dynamic payload exceeds scene reservation"));
        }
        let mut images: std::collections::HashMap<_, _> = slots.iter().copied().collect();
        for (&slot, &payload) in self.images.borrow().iter() {
            if let std::collections::hash_map::Entry::Vacant(entry) = images.entry(slot) {
                add_texture_bytes(&mut total, payload)?;
                entry.insert(payload);
            }
        }
        self.check_model_bytes(total, 0)?;
        *self.images.borrow_mut() = images;
        self.bytes.set(total);
        self.changed();
        Ok(())
    }

    pub fn payload_bytes(&self) -> usize {
        self.bytes.get()
            + self
                .model_bytes
                .borrow()
                .iter()
                .filter(|(owner, _)| owner.strong_count() != 0)
                .map(|(_, bytes)| bytes)
                .sum::<usize>()
            + self.model_layers.borrow().values().sum::<usize>()
    }

    pub fn replacement(&self, slot: usize, bytes: usize) -> Result<Option<usize>> {
        let images = self.images.borrow();
        let Some(previous) = images.get(&slot) else {
            return Ok(None);
        };
        let mut total = self.bytes.get() - previous;
        add_texture_bytes(&mut total, bytes)?;
        self.check_model_bytes(total, 0)?;
        Ok(Some(total))
    }

    pub fn replace(&self, slot: usize, bytes: usize) -> Result<()> {
        if let Some(total) = self.replacement(slot, bytes)? {
            self.images.borrow_mut().insert(slot, bytes);
            if self.bytes.replace(total) != total {
                self.changed();
            }
        }
        Ok(())
    }

    pub fn release(&self, slot: usize) {
        let model = self.model_layers.borrow_mut().remove(&slot);
        let image = self.images.borrow_mut().remove(&slot);
        if let Some(bytes) = image {
            self.bytes.set(self.bytes.get() - bytes);
        }
        if model.is_some() || image.is_some() {
            self.changed();
        }
    }

    pub fn reserve(&self, bytes: usize) -> Result<()> {
        let mut total = 0;
        add_texture_bytes(&mut total, bytes)?;
        if total < self.bytes.get() {
            return Err(anyhow!("dynamic image payload exceeds the initial scene budget"));
        }
        self.check_model_bytes(total, 0)?;
        self.bytes.set(total);
        self.changed();
        self.images.borrow_mut().clear();
        Ok(())
    }
}

struct ImageContext<'a> {
    font_source: Option<&'a std::sync::Arc<crate::text::script::Source>>,
    scripted: bool,
    force_material: bool,
    hdr: bool,
    pkg: &'a Package,
    assets: &'a crate::effects::Assets,
    by_id: &'a std::collections::HashMap<String, &'a Value>,
    canvas: (f32, f32),
    parallax: Option<&'a Parallax>,
    mouse: crate::mouse::Parallax,
    background: Option<&'a str>,
    created: &'a [usize],
    composites: &'a std::collections::BTreeSet<String>,
    spatial: bool,
}

impl ImageContext<'_> {
    fn identity(&self, object: &Value, object_index: usize) -> (String, String, bool) {
        let props = &self.assets.properties;
        let id = object
            .get(ID_FIELD)
            .and_then(id_of)
            .unwrap_or_else(|| format!("@object-{object_index}"));
        let name =
            object.get(NAME_FIELD).and_then(Value::as_str).unwrap_or(DEFAULT_LAYER_NAME).to_owned();
        (id, name, authored_visible(object, self.by_id, props))
    }

    fn text(
        &self,
        object: &Value,
        object_index: usize,
        skipped: &mut Vec<String>,
        texture_bytes: &mut usize,
    ) -> Result<Option<Layer>> {
        let Self { scripted, font_source, pkg, assets, by_id, canvas, parallax, mouse, .. } = *self;
        let props = &assets.properties;
        let (id, name, visible) = self.identity(object, object_index);
        let (mut effects, effect_skips) =
            crate::effects::load_effects(pkg, assets, object, scripted);
        let mode = if effected(&effects) {
            crate::text::RasterMode::Effect
        } else {
            crate::text::RasterMode::Direct
        };
        let color_blend = number(object.get(COLOR_BLEND_FIELD), props, 0.0).max(0.0) as u32;
        if crate::effects::shader_blend(color_blend) {
            let blend =
                crate::effects::color_blend_effect(pkg, assets, color_blend, Some(&id), false);
            if blend.is_none() {
                skipped.push(format!("{name}: colorBlendMode {color_blend} unavailable"));
            }
            effects.extend(blend);
        }
        for skip in effect_skips {
            skipped.push(format!("{name}: {skip}"));
        }
        let Some((rendered, script_text)) = (if scripted {
            crate::text::script::render(
                pkg,
                assets,
                object,
                props,
                font_source.expect("scripted font source").clone(),
                mode,
            )
            .map(|(rendered, state)| (rendered, Some(state)))
        } else {
            crate::text::render(pkg, assets, object, props, mode).map(|r| (r, None))
        }) else {
            skipped.push(format!("{name}: text unsupported"));
            return Ok(None);
        };
        let transform = resolve_transform(object, by_id, props, parallax, &Attachments::new())
            .placed(object, self.spatial);
        let ([w, h], text_offset) = rendered.layout();
        let color =
            vec3(object.get(COLOR_FIELD), props).map_or([1.0, 1.0, 1.0], |(r, g, b)| [r, g, b]);
        let alpha = layer_alpha(object, props);
        add_texture_bytes(texture_bytes, rendered.texture.payload_bytes())?;
        let (sx, sy) = transform.scale;
        let angle = -transform.angle;
        let geometry = Geometry::new(transform, (w, h), text_offset, canvas);
        let center = (geometry.transform.x, geometry.transform.y);
        Ok(Some(Layer {
            texture_name: None,
            material: None,
            id,
            name,
            visible,
            live_text: rendered.live,
            script_text,
            kind: LayerKind::Text,
            mouse: layer_mouse(object, by_id, props, mouse, transform, true),
            texture: rendered.texture,
            puppet: None,
            geometry: Some(geometry),
            center,
            size: (w * sx, h * sy),
            composition_size: None,
            scale: transform.scale,
            depth: transform.origin.2,
            scene_order: object_index,
            alpha,
            angle,
            color,
            color_blend,
            pad_uv: false,
            spritesheet: false,
            passthrough: false,
            fullscreen: false,
            copy_background: false,
            compose_depth: false,
            solid: false,
            base_scene_input: false,
            effects,
        }))
    }

    #[expect(clippy::too_many_lines, reason = "one image model and material parsing boundary")]
    fn load(
        &self,
        object: &Value,
        object_index: usize,
        skipped: &mut Vec<String>,
        texture_bytes: &mut usize,
    ) -> Result<Option<Layer>> {
        let Self { scripted, hdr, pkg, assets, by_id, canvas, parallax, mouse, background, .. } =
            *self;
        let props = &assets.properties;
        let (id, name, visible) = self.identity(object, object_index);
        let model_path = object.get(IMAGE_FIELD).and_then(Value::as_str);
        let quad = model_path.is_none() && shape::is_quad(object);
        if model_path.is_none() && !quad {
            return Err(anyhow!("dynamic layer is not an image"));
        }
        let resolved = model_path.and_then(|_| model_texture(pkg, assets, object));
        let texture_name = resolved.as_ref().and_then(|source| source.path.clone());
        let has_texture = resolved.is_some();
        let autosize = resolved.as_ref().is_some_and(|source| source.autosize);
        let spritesheet = resolved.as_ref().is_some_and(|source| source.spritesheet);
        let util = if has_texture {
            UtilModel::default()
        } else {
            model_path.map_or_else(UtilModel::default, |_| util_model(pkg, assets, object))
        };
        let solid = resolved.is_none() && util.flat;
        let errored = util.untextured && !(util.passthrough || util.flat || util.scene_input);
        let error_sized = errored && util.autosize;
        let media_texture = resolved.as_ref().and_then(|source| source.media);
        let texture = if quad {
            shape::texture()
        } else if let Some(kind) = media_texture {
            kind.texture()
        } else if let Some(ImageSource { path: Some(path), user, .. }) = resolved {
            let texture = user
                .as_deref()
                .and_then(|name| assets.scene_texture_bytes(name, MAX_USER_TEXTURE_FILE_BYTES))
                .and_then(|bytes| load_user_texture(&bytes))
                .or_else(|| load_texture_or_asset(pkg, assets, &path));
            let Some(texture) = texture else {
                let candidates = tex_candidates(&path);
                let mut found = candidates.iter().filter_map(|path| pkg.find(path)).peekable();
                let reason = if found.peek().is_none() {
                    MISSING_TEXTURE
                } else {
                    found
                        .find_map(|bytes| load_texture_bytes(bytes).err())
                        .unwrap_or(UNDECODABLE_TEXTURE)
                };
                skipped.push(format!("{name}: {reason} texture {path}"));
                return Ok(None);
            };
            texture
        } else if util.passthrough || util.flat || util.scene_input {
            solid_texture()
        } else if errored
            || model_path.is_some_and(|path| {
                asset_json(pkg, assets, path).is_none()
                    && (assets.available()
                        || !BUILTIN_MODEL_DIRS.iter().any(|dir| path.starts_with(dir)))
            })
        {
            error_texture()
        } else {
            skipped
                .push(format!("{name}: no texture in {}", model_path.expect("image model path")));
            return Ok(None);
        };
        let puppet_mesh = match model_path.map_or(Ok(None), |path| model_puppet(pkg, path)) {
            Ok(puppet) => puppet,
            Err(err) => {
                skipped.push(format!("{name}: {err}"));
                return Ok(None);
            }
        };
        if let Some(mesh) = &puppet_mesh
            && mesh.skipped_constraints != 0
        {
            skipped.push(format!(
                "{name}: {} puppet constraint metadata unsupported",
                mesh.skipped_constraints
            ));
        }
        if puppet_mesh.as_ref().is_some_and(|mesh| mesh.skipped_draw_order) {
            skipped.push(format!("{name}: puppet draw sections invalid"));
        }
        let transform = resolve_transform(object, by_id, props, parallax, &Attachments::new())
            .placed(object, self.spatial && puppet_mesh.is_none());
        let fallback = if quad {
            (canvas.1, canvas.1)
        } else if util.passthrough {
            canvas
        } else if autosize {
            (texture.img_width as f32, texture.img_height as f32)
        } else if error_sized {
            error_image_size()
        } else {
            (1.0, 1.0)
        };
        let base = if quad || autosize || error_sized {
            fallback
        } else if util.fullscreen || util.project_sized {
            canvas
        } else {
            vec2_or(object.get("size"), props, fallback)
        };
        let mut puppet = puppet_mesh.map(|mesh| Puppet {
            planar_owner: true,
            cull: false,
            cpu: false,
            world: transform.matrix(),
            mesh,
            size: base,
            layers: puppet_animation_layers(object, props),
            mask_textures: Vec::new(),
        });
        if let Some(puppet) = &mut puppet
            && !puppet.mesh.springs.is_empty()
            && (ancestor_chain(object, by_id).iter().any(|node| {
                vec3(node.get(ANGLES_FIELD), props).is_some_and(|(x, y, _)| x != 0.0 || y != 0.0)
            }) || crate::matrix::mat4_inverse(&puppet.world).is_none())
        {
            skipped.push(format!("{name}: puppet spring owner unsupported"));
            puppet.mesh.springs.clear();
        }
        let (mut color, alpha) = match model_path
            .and_then(|_| legacy_material_tint(pkg, assets, object))
        {
            Some(tint) => tint,
            None => (
                vec3(object.get(COLOR_FIELD), props).map_or([1.0, 1.0, 1.0], |(r, g, b)| [r, g, b]),
                layer_alpha(object, props),
            ),
        };
        if hdr {
            let brightness = number(object.get(hdr::BRIGHTNESS_FIELD), props, 1.0);
            for component in &mut color {
                *component *= brightness;
            }
        }
        let color_blend = number(object.get(COLOR_BLEND_FIELD), props, 0.0).max(0.0) as u32;
        let (mut effects, effect_skips) =
            crate::effects::load_effects(pkg, assets, object, scripted);
        let shader_blend = crate::effects::shader_blend(color_blend);
        if util.passthrough && effects.is_empty() && !shader_blend {
            if !self.composites.contains(&id) {
                return Ok(None);
            }
            effects.extend(crate::effects::passthrough_effect(pkg, assets));
            if effects.is_empty() {
                return Ok(None);
            }
        }
        let blend = if shader_blend {
            let effect = crate::effects::color_blend_effect(
                pkg,
                assets,
                color_blend,
                Some(&id),
                puppet.is_some() && (scripted || authored_visible(object, by_id, props)),
            );
            if effect.is_none() {
                skipped.push(format!("{name}: colorBlendMode {color_blend} unavailable"));
            }
            effect
        } else {
            None
        };
        let mut material = if model_path.is_some() {
            match crate::effects::material::load(
                pkg,
                assets,
                object,
                &texture,
                puppet.as_mut(),
                &crate::effects::material::Options {
                    source_texture: has_texture || errored,
                    force: self.force_material,
                    effects: !effects.is_empty() || blend.is_some(),
                    blend: (color_blend == crate::effects::ADDITIVE_COLOR_BLEND)
                        .then_some(crate::effects::PassBlend::Add),
                },
            ) {
                Ok(material) => material,
                Err(_) if puppet.as_ref().is_some_and(|puppet| puppet.cpu) => None,
                Err(reason) => {
                    skipped.push(format!("{name}: {reason}"));
                    return Ok(None);
                }
            }
        } else {
            None
        };
        effects.extend(blend);
        let mut base_scene_input = false;
        let reads = |input: fn(&crate::effects::EffectBind) -> bool| {
            material
                .as_ref()
                .is_some_and(|material| material.pass.binds.iter().any(|(_, bind)| input(bind)))
        };
        let reads_scene = reads(|bind| matches!(bind, crate::effects::EffectBind::SceneSoFar));
        let reads_composite =
            reads(|bind| matches!(bind, crate::effects::EffectBind::LayerComposite { .. }));
        if !has_texture && (reads_scene || reads_composite) {
            if material.as_ref().is_some_and(|material| match material.blend {
                crate::effects::PassBlend::Copy | crate::effects::PassBlend::AlphaCoverage => false,
                crate::effects::PassBlend::Alpha | crate::effects::PassBlend::Add => {
                    !effects.is_empty() || color_blend != NORMAL_COLOR_BLEND
                }
                crate::effects::PassBlend::Screen => true,
            }) {
                skipped.push(format!("{name}: base material scene input blending unsupported"));
                return Ok(None);
            }
            let material = material.take().expect("scene input material");
            base_scene_input = reads_scene;
            effects.insert(
                0,
                crate::effects::Effect {
                    state: crate::effects::EffectState::default(),
                    name: material.pass.name.clone(),
                    passes: vec![material.pass],
                    fbos: Vec::new(),
                    swaps: Vec::new(),
                    clear_functions: std::collections::BTreeMap::new(),
                },
            );
        }
        let skinned = material.as_ref().is_some_and(|material| material.skin.is_some());
        if skinned
            && (![
                transform.affine.a,
                transform.affine.b,
                transform.affine.c,
                transform.affine.d,
                transform.affine.x,
                transform.affine.y,
                transform.origin.2,
                transform.depth_scale,
            ]
            .iter()
            .all(|value| value.is_finite())
                || puppet
                    .as_ref()
                    .is_none_or(|puppet| !supported_layers(object, props, &puppet.layers))
                || transform
                    .spatial
                    .is_some_and(|matrix| matrix.iter().any(|value| !value.is_finite()))
                || (parallax.is_some() && puppet.as_ref().is_some_and(|puppet| puppet.cpu))
                || (!effects.is_empty()
                    && material.as_ref().is_some_and(|material| {
                        !material
                            .pass
                            .vertex
                            .uniforms
                            .iter()
                            .any(|uniform| uniform.name == crate::effects::material::UV_ROTATION)
                    }))
                || !effect_skips.is_empty()
                || util.passthrough
                || util.fullscreen
                || util.scene_input
                || !has_texture
                || spritesheet
                || color_blend != NORMAL_COLOR_BLEND
                || object.get(ALIGNMENT_FIELD).is_some_and(|alignment| {
                    alignment.as_str() != Some(CENTERED)
                        || puppet.as_ref().is_some_and(|puppet| puppet.cpu)
                })
                || (vec3(object.get(ANGLES_FIELD), props)
                    .is_some_and(|(x, y, _)| x != 0.0 || y != 0.0)
                    && (object.get(PARENT_FIELD).is_some()
                        || object.get(ATTACHMENT_FIELD).is_some()
                        || !effects.is_empty()))
                || transform.depth_scale.abs() < f32::EPSILON)
        {
            let Some(puppet) = puppet.as_mut().filter(|puppet| puppet.cpu) else {
                skipped.push(format!("{name}: puppet material layer state unsupported"));
                return Ok(None);
            };
            puppet.adopt(material.take());
        }
        if let Some(puppet) = &mut puppet {
            puppet.mesh.raw_vertices = None;
        }
        if material.is_some() && ((!skinned && puppet.is_some()) || util.passthrough) {
            skipped.push(format!("{name}: base material puppet or passthrough route unsupported"));
            return Ok(None);
        }
        if let Some(material) = &material {
            for texture in material.pass.textures.iter().flatten() {
                add_texture_bytes(texture_bytes, assets.texture_charge(texture))?;
            }
        }
        let charge = if self.created.binary_search(&object_index).is_ok() {
            texture.payload_bytes()
        } else {
            assets.texture_charge(&texture)
        };
        add_texture_bytes(texture_bytes, charge)?;
        for effect in &effects {
            for pass in &effect.passes {
                for slot in pass.textures.iter().flatten() {
                    add_texture_bytes(texture_bytes, assets.texture_charge(slot))?;
                }
            }
        }
        for skip in effect_skips {
            skipped.push(format!("{name}: {skip}"));
        }
        let (center, size, angle) = if util.fullscreen {
            ((canvas.0 * 0.5, canvas.1 * 0.5), canvas, 0.0)
        } else {
            (
                (transform.origin.0, canvas.1 - transform.origin.1),
                (base.0 * transform.scale.0, base.1 * transform.scale.1),
                -transform.angle,
            )
        };
        let center = if util.fullscreen {
            center
        } else if background == Some(id.as_str()) {
            (canvas.0 * 0.5, canvas.1 * 0.5)
        } else {
            aligned_center(center, size, angle, object.get(ALIGNMENT_FIELD).and_then(Value::as_str))
        };
        let geometry = if util.fullscreen || background == Some(id.as_str()) {
            None
        } else {
            let offset = aligned_center(
                (0.0, 0.0),
                base,
                0.0,
                object.get(ALIGNMENT_FIELD).and_then(Value::as_str),
            );
            Some(Geometry::new(transform, base, [offset.0, offset.1], canvas))
        };
        let center =
            geometry.map_or(center, |geometry| (geometry.transform.x, geometry.transform.y));
        let pad_uv = has_texture
            && puppet.is_none()
            && (effects.is_empty() || material.is_some())
            && texture.video.is_none()
            && texture.system_texture.is_none()
            && model_path.is_some_and(|path| model_uv_padding(pkg, assets, path));
        if texture.stream.is_some() && (!spritesheet || puppet.is_some()) {
            skipped.push(format!("{name}: streamed sprite outside a sprite sheet unsupported"));
            return Ok(None);
        }
        Ok(Some(Layer {
            texture_name,
            id,
            name,
            visible,
            live_text: None,
            script_text: None,
            kind: if quad { LayerKind::Quad } else { LayerKind::Image },
            mouse: layer_mouse(object, by_id, props, mouse, transform, false),
            texture,
            puppet,
            geometry,
            center,
            size,
            composition_size: (util.passthrough && !util.fullscreen).then_some(base),
            scale: transform.scale,
            depth: transform.origin.2,
            scene_order: object_index,
            alpha,
            angle,
            color,
            color_blend,
            pad_uv,
            spritesheet,
            passthrough: util.passthrough,
            fullscreen: util.fullscreen,
            copy_background: util.passthrough && truthy(object.get("copybackground"), props, true),
            compose_depth: util.passthrough
                && !effects.is_empty()
                && crate::effects::material::depth_tested(pkg, assets, object),
            solid,
            base_scene_input,
            material,
            effects,
        }))
    }
}

fn scene_zoom(scene: &Value, props: &Properties) -> f32 {
    Some(number(
        scene.get(GENERAL_FIELD).and_then(|general| general.get(ZOOM_FIELD)),
        props,
        UNIT_ZOOM,
    ))
    .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
    .unwrap_or(UNIT_ZOOM)
}

fn load_with_project(
    pkg: &Package,
    assets: &crate::effects::Assets,
    project: &Value,
    storage: Option<crate::script::Storage>,
    screen_resolution: Option<[u32; 2]>,
    pass: LoadPass,
) -> Result<SceneModel> {
    let props = &assets.properties;
    let mut scene = pkg.scene_json()?;
    crate::text::bind_user_values(&mut scene, project);
    let hidden = hidden::HiddenLayers::of(&scene);
    let perspective = ProjectionMode::of(&scene) == ProjectionMode::Other;
    let mut camera_gaps = Vec::new();
    let cameras_move = perspective && camera_object::cameras_move(&scene);
    if perspective {
        camera_gaps = camera_object::perspective_gaps(&scene, props);
    }
    let mut keyframes = crate::keyframe::Bindings::parse(&mut scene)?;
    let general_zoom = scene_zoom(&scene, props);
    if pass.without_scripts {
        crate::script::SceneScripts::strip(&mut scene, props);
    }
    let script_runtime = crate::script::SceneScripts::uses_runtime(&scene);
    let sprite_info =
        if script_runtime { sprite_metadata(&scene, pkg, assets) } else { Vec::new() };
    let images = script_runtime.then(|| ImageResources::new(pkg, assets));
    let font_source = images.as_ref().map(|images| images.font_source.clone()).or_else(|| {
        (!keyframes.is_empty())
            .then(|| std::sync::Arc::new(crate::text::script::Source::new(pkg, assets)))
    });
    let puppets = script_runtime.then(|| puppet_playback(&scene, pkg, props));
    let mut scripts = crate::script::SceneScripts::load_with_storage(
        &mut scene,
        props,
        project,
        crate::script::Load {
            storage,
            images,
            keyframes: Some(&keyframes),
            screen_resolution,
            sprites: Some(&sprite_info),
            puppets,
            limits: crate::script::Limits::default(),
        },
    )?;
    if scripts.is_none() {
        keyframes.apply(&mut scene, 0.0);
    }
    let camera_source =
        CameraSource::of(&scene, props, perspective, |path| asset_json(pkg, assets, path));
    let camera_paths = camera_source.pathed();
    if perspective {
        camera_object::adopt(&mut scene, props);
    }
    let scripted = scene_scripted(scripts.is_some(), &keyframes);
    let (mut lighting, light_skips) = lighting::load(&scene, props);
    let scene_lights = lighting::lights(&scene, props, &Attachments::new());
    lighting.shadows |= scripted && !scene_lights.is_empty();
    lighting.cookies |= scripted && scene_lights.iter().any(|light| light.cone.is_some());
    let cookie_lights = scene_lights
        .iter()
        .rev()
        .find(|light| light.visible && light.cookie.is_some())
        .map_or(&[][..], std::slice::from_ref);
    let cookies = lighting::cookie::load(pkg, assets, cookie_lights)?;
    if let Some(cookies) = &cookies {
        lighting.values.extend(cookies.layout.uniforms(&scene_lights));
    }
    let projection = ProjectionMode::of(&scene);
    let scene_assets = assets
        .with_scene(crate::effects::SceneContext {
            projection: Some(projection),
            lighting,
            fog: fog::Config::load(&scene, props),
        })
        .with_texture_cache();
    let assets = &scene_assets;
    let general = scene.get(GENERAL_FIELD);
    let hdr = hdr::enabled(general, props);
    let ImageGeometry { canvas, background, parallax, mouse } = ImageGeometry::of(&scene, props);
    let particle_context = crate::particles::LoadContext {
        pkg,
        assets,
        defaults: crate::particles::ParticleDefaults::for_projection(projection),
        viewport: crate::particles::BoundsViewport::of(&scene),
        bounds_static: !scripted && parallax.is_none(),
        version: scene
            .get(SCENE_VERSION_FIELD)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
    };
    let clear = vec3(general.and_then(|top| top.get("clearcolor")), props)
        .map_or([0.0, 0.0, 0.0], |(r, g, b)| [r, g, b]);
    let clear_enabled = truthy(general.and_then(|top| top.get(CLEAR_ENABLED_FIELD)), props, true);
    let ambient = vec3(general.and_then(|top| top.get("ambientcolor")), props)
        .map_or([0.3, 0.3, 0.3], |(r, g, b)| [r, g, b]);
    let skylight = vec3(general.and_then(|top| top.get("skylightcolor")), props)
        .map_or([0.3, 0.3, 0.3], |(r, g, b)| [r, g, b]);

    let objects = scene.get("objects").and_then(Value::as_array).map_or(&[][..], |arr| arr);
    if objects.len() > MAX_SCENE_OBJECTS {
        return Err(anyhow!("scene has {} objects; limit is {MAX_SCENE_OBJECTS}", objects.len()));
    }
    let by_id: std::collections::HashMap<String, &Value> = objects
        .iter()
        .filter_map(|object| Some((object.get("id").and_then(id_of)?, object)))
        .collect();
    let image_sources: std::collections::BTreeSet<String> = objects
        .iter()
        .flat_map(crate::particles::image::ImageDependency::parse)
        .map(|dependency| dependency.source.to_string())
        .collect();
    let dependency_ids: std::collections::BTreeSet<String> =
        objects.iter().flat_map(dependencies).collect();
    let mut render_target_layer_ids = dependency_ids.clone();
    for object in objects {
        collect_render_target_layer_ids(
            object,
            object.get("id").and_then(id_of).as_deref(),
            &mut render_target_layer_ids,
        );
    }
    let parents: std::collections::HashSet<_> =
        objects.iter().filter_map(|object| object.get("parent").and_then(id_of)).collect();
    let created =
        scripts.as_ref().map(|host| host.created_layer_slots().to_vec()).unwrap_or_default();
    let image_context = ImageContext {
        scripted,
        force_material: false,
        font_source: font_source.as_ref(),
        hdr,
        pkg,
        assets,
        by_id: &by_id,
        canvas,
        parallax: parallax.as_ref(),
        mouse,
        background: background.as_deref(),
        created: &created,
        composites: &render_target_layer_ids,
        spatial: spatial_scene(&scene, props),
    };
    let objects_source = objects::SceneObjects {
        images: &image_context,
        particles: &particle_context,
        image_sources: &image_sources,
        parents: &parents,
    };
    let (late, early): (Vec<_>, Vec<_>) =
        objects.iter().enumerate().partition(|(object_index, object)| {
            objects_source.deferrable(&hidden, *object_index, object)
        });
    let mut loaded =
        objects::Loaded::new(light_skips, scene.get(OBJECTS_FIELD).is_some_and(Value::is_array));
    let mut defer_hidden = pass.defer_hidden;
    if !defer_hidden && !late.is_empty() {
        let bytes =
            objects_source.texture_floor(early.iter().chain(&late).map(|(_, object)| *object));
        defer_hidden = bytes > pass.texture_budget;
        if defer_hidden {
            let count = late.len();
            tracing::info!("scene: {count} hidden layers deferred, textures need {bytes} bytes");
        }
    }
    for (object_index, object) in early {
        objects_source.load(&mut loaded, object_index, object)?;
    }
    let mark = loaded.mark();
    let mut over_budget = defer_hidden;
    for (object_index, object) in late.iter().take_while(|_| !defer_hidden) {
        match objects_source.load(&mut loaded, *object_index, object) {
            Ok(()) if loaded.texture_bytes <= pass.texture_budget => {}
            Err(error) if error.downcast_ref::<TextureBudgetExceeded>().is_none() => {
                return Err(error);
            }
            _ => {
                over_budget = true;
                break;
            }
        }
    }
    let deferred: Vec<String> = if over_budget {
        loaded.rollback(mark);
        late.iter()
            .map(|(object_index, object)| objects::object_id(*object_index, object))
            .collect()
    } else {
        Vec::new()
    };
    if over_budget && !defer_hidden {
        let (count, bytes) = (deferred.len(), loaded.texture_bytes);
        tracing::info!("scene: {count} hidden layers deferred at {bytes} texture bytes");
    }
    let hidden_loaded = !over_budget && !late.is_empty();
    let mut skipped = loaded.ordered_skips();
    let objects::Loaded {
        mut layers,
        mut meshes,
        mut particles,
        mut texture_bytes,
        background_only,
        ..
    } = loaded;
    meshes.sort_by_key(|mesh| mesh.scene_order);
    particles.sort_by_key(|particle| particle.scene_order);
    if let Some(host) = scripts.as_mut() {
        host.restrict_hidden_layer_updates(&deferred);
    }
    let (bloom, bloom_skips) = bloom::load(bloom::Source {
        package: pkg,
        assets,
        general,
        view: bloom::View { canvas, hdr, order: objects.len() },
    });
    skipped.extend(bloom_skips);
    if let Some(bloom) = bloom {
        layers.push(bloom);
    }
    layers.sort_by_key(|layer| layer.scene_order);
    let particle_states = script::particle_frames(
        &scene,
        particles.iter().map(|layer| layer.id.as_str()),
        canvas,
        props,
        &Attachments::new(),
    );
    for (layer, state) in particles.iter_mut().zip(particle_states) {
        if let Some(state) = state {
            state.apply(&mut layer.system);
            layer.visible = state.visible;
            layer.depth = layer.system.origin.2;
        }
    }
    let mut unified_order: Vec<(usize, OrderedItem)> = layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.scene_order, OrderedItem::Layer(index)))
        .chain(
            particles
                .iter()
                .enumerate()
                .map(|(index, particle)| (particle.scene_order, OrderedItem::Particle(index))),
        )
        .chain(
            meshes
                .iter()
                .enumerate()
                .map(|(index, mesh)| (mesh.scene_order, OrderedItem::Mesh(index))),
        )
        .collect();
    unified_order.sort_by_key(|entry| entry.0);
    for (scene_order, (_, item)) in unified_order.into_iter().enumerate() {
        match item {
            OrderedItem::Layer(index) => layers[index].scene_order = scene_order,
            OrderedItem::Particle(index) => particles[index].scene_order = scene_order,
            OrderedItem::Mesh(index) => meshes[index].scene_order = scene_order,
        }
    }
    let overrun = |error: anyhow::Error| {
        let error = error.context(ScriptImageOverrun);
        if hidden_loaded { error.context(HiddenLoaded) } else { error }
    };
    if let Some(scripts) = &mut scripts {
        let mut initial = Vec::new();
        for layer in &layers {
            let Some(slot) = layer.object_index(&scripts.scene) else { continue };
            if layer.script_text.is_none() && !scripts.created_layer_slots().contains(&slot) {
                continue;
            }
            let metadata = layer.texture.frame_bytes()?;
            add_texture_bytes(&mut texture_bytes, metadata).map_err(overrun)?;
            let mut payload = layer.texture.payload_bytes();
            add_texture_bytes(&mut payload, metadata).map_err(overrun)?;
            initial.push((slot, payload));
        }
        #[cfg(test)]
        tests::reservation_fault().map_err(overrun)?;
        scripts.reserve_image_bytes(texture_bytes, &initial).map_err(overrun)?;
        drop(scripts.take_created_images());
        scripts.restrict_property_updates(pkg, assets);
        let sprites: Vec<(usize, Vec<f32>)> = layers
            .iter()
            .filter(|layer| layer.atlas_frames().is_some())
            .filter_map(|layer| {
                Some((
                    layer.object_index(&scripts.scene)?,
                    layer.texture.frames.iter().map(|frame| frame.time).collect(),
                ))
            })
            .collect();
        scripts.set_sprites(&sprites);
    }
    let mut fonts: Vec<std::sync::Arc<[u8]>> = Vec::new();
    for layer in &mut layers {
        if let Some(text) = &mut layer.script_text {
            if let Some(font) = fonts.iter().find(|font| font.as_ref() == text.font.as_ref()) {
                text.font = font.clone();
            } else {
                fonts.push(text.font.clone());
            }
        }
    }
    let shake = truthy(general.and_then(|top| top.get("camerashake")), props, false).then(|| {
        crate::mouse::Shake {
            amplitude: number(general.and_then(|top| top.get("camerashakeamplitude")), props, 1.0),
            speed: number(general.and_then(|top| top.get("camerashakespeed")), props, 1.0),
            roughness: number(general.and_then(|top| top.get("camerashakeroughness")), props, 1.0),
        }
    });
    let image_camera = Camera::of(&scene, props, projection);
    let flat = projection != ProjectionMode::Other;
    let camera_view = active_camera_view(&scene, props, general_zoom, flat);
    skipped.extend(camera_gaps.into_iter().map(String::from));
    let camera_images = !layers.is_empty()
        && parallax.is_none()
        && layers.iter().all(|layer| {
            (layer.fullscreen && layer.kind == LayerKind::Image && layer.puppet.is_none())
                || matches!(layer.kind, LayerKind::Viewport { .. })
                || matches!(layer.kind, LayerKind::Image | LayerKind::Text)
                    && layer.geometry.is_some_and(|geometry| {
                        geometry
                            .spatial
                            .is_none_or(|matrix| matrix.iter().all(|value| value.is_finite()))
                    })
                    && layer.puppet.is_none()
                    && !layer.base_scene_input
                    && layer.mouse.clock.is_none()
        });
    if image_camera.is_some() && !layers.is_empty() && !camera_images {
        skipped.push(CAMERA_IMAGE_GRAPH_GAP.to_string());
    }
    skipped.extend(
        layers
            .iter()
            .filter(|layer| layer.depth_composite_dropped())
            .map(|layer| format!("{}: {DEPTH_COMPOSITE_GAP}", layer.name)),
    );
    let camera = (!meshes.is_empty()
        || camera_images
        || (scripts.is_none()
            && keyframes.is_empty()
            && layers.is_empty()
            && parallax.is_none()
            && scene.get(OBJECTS_FIELD).and_then(Value::as_array).is_some_and(|objects| {
                objects.len() == particles.len()
                    && objects.iter().all(|object| {
                        object.get(PARTICLE_FIELD).and_then(Value::as_str).is_some()
                            && object.get(PARENT_FIELD).is_none()
                            && object.get(SCALE_FIELD).is_none_or(|scale| {
                                vec3(Some(scale), props).is_some_and(|(x, y, z)| {
                                    [x, y, z].iter().all(|value| {
                                        value.is_finite() && value.abs() >= f32::EPSILON
                                    })
                                })
                            })
                            && object.get(ANGLES_FIELD).is_none_or(|angles| {
                                vec3(Some(angles), props).is_some_and(|(x, y, _)| {
                                    x.is_finite() && y.is_finite() && x == 0.0 && y == 0.0
                                })
                            })
                            && CAMERA_OTHER_OBJECT_FIELDS
                                .iter()
                                .all(|field| object.get(*field).is_none())
                    })
            })
            && particles.iter().all(|layer| {
                !layer.system.perspective
                    && layer.system.renderer == crate::particles::Renderer::Sprite
                    && layer.system.orientation == crate::particles::Orientation::Screen
                    && layer.system.renderer_flags == 0
                    && !layer.system.world
                    && layer.system.children.is_empty()
                    && !layer.system.follows_mouse()
            })))
    .then(|| Camera::of(&scene, props, projection))
    .flatten();
    if !meshes.is_empty() && projection == ProjectionMode::Other && camera.is_none() {
        meshes.clear();
        skipped.push(MESH_CAMERA_UNAVAILABLE.to_string());
    }
    let reflection = ReflectionUse::of(&layers, &meshes, &particles, scripts.as_ref());
    if !reflection.bound {
        reflection_fallback(&mut layers, &mut particles);
    }
    let unreflected = objects
        .iter()
        .filter(|object| object.get(REFLECTED_FIELD).and_then(Value::as_bool) == Some(false))
        .filter_map(|object| object.get(ID_FIELD).and_then(id_of))
        .collect();
    if reflection.sampled {
        demote_reflected(&mut meshes, &mut skipped);
        for object in objects {
            if object.get(MESH_FIELD).and_then(Value::as_str).is_none()
                && REFLECTION_SOURCE_FIELDS.iter().any(|key| object.get(key).is_some())
                && object.get(REFLECTED_FIELD).and_then(Value::as_bool).unwrap_or(true)
                && object_visible(object, &by_id, props)
                && reflection_source_gap(object, &layers)
            {
                skipped.push(REFLECTION_SOURCE_UNSUPPORTED.to_string());
            }
        }
    }
    crate::particles::model_collision::bind(&mut particles, &meshes, &by_id, &mut skipped);
    layers.retain_mut(|layer| {
        let clock = layer.puppet.as_ref().is_some_and(|puppet| {
            scripts.as_ref().is_some_and(|host| {
                let states = host.puppet_states(&layer.id);
                states.len() == puppet.layers.len()
                    && states
                        .iter()
                        .zip(&puppet.layers)
                        .all(|(state, clip)| state.layer.id == clip.id)
            })
        });
        !(scripts.is_some() && !clock && layer.skinned()) || layer.unskin(false, &mut skipped)
    });
    meshes.retain(|mesh: &MeshLayer| {
        let gap =
            composite_gap(mesh, &layers).or_else(|| resolved_frame_gap(mesh, &layers, &particles));
        if let Some(gap) = &gap {
            skipped.push(format!("{}: {gap}", mesh.name));
        }
        gap.is_none()
    });
    loop {
        let unmounted: Vec<String> = meshes
            .iter()
            .filter(|mesh| !mounted(mesh, &meshes))
            .map(|mesh| mesh.id.clone())
            .collect();
        if unmounted.is_empty() {
            break;
        }
        meshes.retain(|mesh| {
            let keep = !unmounted.contains(&mesh.id);
            if !keep {
                skipped.push(format!("{}: {MOUNT_GAP}", mesh.name));
            }
            keep
        });
    }
    let supported = projection == ProjectionMode::Orthographic
        && shake.is_none()
        && parallax.is_none()
        && camera.is_none()
        && !hdr
        && !reflection.bound
        && particles.is_empty()
        && image_sources.is_empty()
        && layers.iter().all(|layer| layer.atlas_frames().is_none())
        && ((meshes.is_empty() && layers.iter().any(|layer| layer.base_scene_input))
            || layers.iter().all(Layer::orthographic_mesh_ready));
    if !supported {
        let framebuffer = layers.iter().any(|layer| layer.base_scene_input);
        let hosted = projection == ProjectionMode::Orthographic
            && camera.is_none()
            && meshes.is_empty()
            && !framebuffer;
        layers.retain_mut(|layer| {
            !layer.skinned()
                || layer.unskin(hosted && !image_sources.contains(&layer.id), &mut skipped)
        });
    }
    for layer in &mut layers {
        layer.load_masks(pkg, assets, &mut texture_bytes, &mut skipped)?;
    }
    let custom_shaders = assets.has_custom_shaders();
    anyhow::ensure!(
        !custom_shaders || skipped.is_empty(),
        "customshader scene rejected: {}",
        skipped.join(DIAGNOSTIC_SEPARATOR)
    );
    Ok(SceneModel {
        reflection,
        unreflected,
        custom_shaders,
        cameras_move,
        camera_source,
        skin_viewport: supported,
        lights: scene_lights,
        sun_view: lighting::SunView::of(&scene, props),
        fog: fog::Volume::load(&scene, props),
        cookies,
        image_sources,
        dependencies: dependency_ids,
        animated_scene: (scripts.is_none()
            && (!keyframes.is_empty()
                || camera_paths
                || layers.iter().any(|layer| {
                    layer.puppet.as_ref().is_some_and(|puppet| !puppet.mesh.attachments.is_empty())
                })))
        .then(|| (scene.clone(), props.clone())),
        keyframes,
        scripts,
        canvas,
        mouse,
        camera_fov: number(general.and_then(|top| top.get(CAMERA_FOV)), props, DEFAULT_CAMERA_FOV)
            .clamp(FOV_RANGE.0, FOV_RANGE.1),
        perspective_fov: number(
            general.and_then(|top| top.get(PERSPECTIVE_FOV_FIELD)),
            props,
            DEFAULT_PERSPECTIVE_FOV,
        )
        .clamp(FOV_RANGE.0, FOV_RANGE.1),
        camera_view,
        camera,
        projection,
        shake,
        clear,
        clear_enabled,
        ambient,
        skylight,
        layers,
        meshes,
        particles,
        skipped,
        background_only,
        hdr,
    })
}

fn charge_particle_textures(
    assets: &crate::effects::Assets,
    system: &crate::particles::ParticleSystem,
    total: &mut usize,
) -> Result<()> {
    if let Some(texture) = &system.texture {
        add_texture_bytes(total, assets.texture_charge(texture))?;
    }
    for child in &system.children {
        charge_particle_textures(assets, child, total)?;
    }
    Ok(())
}

const TEXTURE_BUDGET_EXCEEDED: &str = "scene texture budget exceeded";

#[derive(Debug)]
pub struct TextureBudgetExceeded;

const SCRIPT_IMAGE_OVERRUN: &str = "script layers overrun the scene texture reservation";

#[derive(Debug)]
struct ScriptImageOverrun;

const HIDDEN_LOADED: &str = "hidden layers loaded";

#[derive(Debug)]
struct HiddenLoaded;

impl std::fmt::Display for HiddenLoaded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(HIDDEN_LOADED)
    }
}

impl std::fmt::Display for ScriptImageOverrun {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(SCRIPT_IMAGE_OVERRUN)
    }
}

impl std::fmt::Display for TextureBudgetExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(TEXTURE_BUDGET_EXCEEDED)
    }
}

fn add_texture_bytes(total: &mut usize, bytes: usize) -> Result<()> {
    *total = total.checked_add(bytes).ok_or_else(|| anyhow!("scene texture size overflow"))?;
    if *total > MAX_SCENE_TEXTURE_BYTES {
        return Err(anyhow!(
            "scene textures use {total} bytes; limit is {MAX_SCENE_TEXTURE_BYTES}"
        )
        .context(TextureBudgetExceeded));
    }
    Ok(())
}

#[cfg(test)]
#[path = "model_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "model/puppet_clock_tests.rs"]
mod puppet_clock_tests;
