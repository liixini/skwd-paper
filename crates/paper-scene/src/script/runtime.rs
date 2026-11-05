use super::budget::{Budget, Limits};
use super::source;
use crate::model::MESH_FIELD;
use anyhow::{Result, anyhow};
use rquickjs::{Context, Ctx, Exception, Function, Module, Runtime};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::Instant;

const MATRIX_ATAN2: &str = "__matrixAtan2";
const OBJECTS_FIELD: &str = "objects";
const EFFECT_VISIBLE_SUFFIX: &str = "/visible";
const EFFECTS_SEGMENT: &str = "/effects/";
const OBJECTS_PREFIX: &str = "/objects/";
const VISIBLE_FIELD: &str = "visible";
const ID_FIELD: &str = "id";
const DISCARD_LAYER: &str = "__discardLayer";
const DRAIN_LIFECYCLE: &str = "__drainLifecycle";
const DRAIN_CREATED: &str = "__drainCreated";
const RELEASE_LAYERS: &str = "__releaseLayers";
const VALIDATE_IMAGE: &str = "__validateImage";
const EFFECT_MATERIALS: &str = "__effectMaterials";
const DRAIN_CAMERA: &str = "__drainCamera";
const CREATE_MODEL: &str = "__createModel";
const ATTACHMENT_INFO: &str = "__attachmentInfo";
const ATTACHMENT_MISSING: &str = "null";
const DESTROY_MODEL: &str = "__destroyModel";
const MAX_SCENE_OBJECTS_JS: &str = "__maxSceneObjects";
const MISSING_IMAGE_RESOURCES: &str = "SceneScript dynamic image creation requires resources";
const RETIRE_LAYERS: &str = "__retireLayers";
const PREPARE_MODULE: &str = "__prepareModule";
const PENDING_WORK: &str = "__timers.size > 0 || __pendingLayers.size > 0 || __commands.length > 0 || __hasListeners('update')";
const EMIT_EVENT: &str = "__emit";
const INIT_EVENT: &str = "init";
const UPDATE_EVENT: &str = "update";
const VIDEO_TEXTURE: &str = "__hasVideoTexture";
const VIDEO_STATES: &str = "__setVideos";
const VIDEO_COMMAND: &str = "video";
const SHORTCUT_COMMAND: &str = "shortcut";
const MAX_SHORTCUT_NAME_BYTES: usize = 256;
const SORT_COMMAND: &str = "sort";
const PARTICLE_COMMAND: &str = "particle";
const EFFECT_COMMAND: &str = "effect";
const MATERIAL_COMMAND: &str = "material";
const EFFECT_MATERIAL_COMMAND: &str = "effectMaterial";
const MAX_MATERIAL_VALUES: usize = 16;
const MAX_MATERIAL_VALUES_JS: &str = "__maxMaterialValues";
const MAX_MATERIAL_NAME_BYTES: usize = 128;
const EFFECT_MATERIAL_PARTS: usize = 3;
const PLAY_COMMAND: &str = "play";
const STOP_COMMAND: &str = "stop";
const PAUSE_COMMAND: &str = "pause";
const EMIT_COMMAND: &str = "emit";
const SIZE_COMMAND: &str = "size";
const ALPHA_COMMAND: &str = "alpha";
const COUNT_COMMAND: &str = "count";
const RATE_COMMAND: &str = "rate";
const SPEED_COMMAND: &str = "speed";
const LIFETIME_COMMAND: &str = "lifetime";
const COLORN_COMMAND: &str = "colorn";
const CONTROL_POINT_COMMAND: &str = "controlpoint";
const SET_PARTICLES: &str = "__setParticles";
const SET_SOUNDS: &str = "__setSounds";
const RESTORE_LAYER_ORDER: &str = "__restoreLayerOrder";
const UPDATE_BUDGET_EXHAUSTED: &str = "scripts stopped: update budget exhausted";
const LOAD_BUDGET_EXHAUSTED: &str = "scripts stopped: load budget exhausted";
const HALT_SCRIPTS: &str = "__halt";
const BONE_WORLD_READERS: &str = "__boneWorldReaders";
const BONE_WORLD_METHODS: &str = "BoneTransform";
const SORT_REJECTED: &str = "layer sort rejected: unsupported scene target dependencies";
const JOB_BUDGET_EXHAUSTED: &str = "SceneScript job budget exhausted";
const RUN_JOBS: &str = "__runJobs";
const TIMELINE_CONTROL: &str = "__timelineControl";
const PUPPET_CONTROL: &str = "__puppetControl";
const PUPPET_VISIBILITY: &str = "__puppetVisibility";
const PUPPET_BLEND: &str = "__puppetBlend";
const PUPPET_CREATE: &str = "__puppetCreate";
const PUPPET_DESTROY: &str = "__puppetDestroy";
const PUPPET_BONE_GET: &str = "__puppetBoneGet";
const PUPPET_MORPH: &str = "__puppetMorph";
const PUPPET_BONE_SET: &str = "__puppetBoneSet";
const PUPPET_SPRING_STEP: &str = "__puppetSpringStep";
const PUPPET_ENDED: &str = "__puppetEnded";
const PUPPET_EVENT: &str = "__puppetEvent";
const SET_PUPPETS: &str = "__setPuppets";
const GENERAL_SETTINGS: &str = "__applyGeneralSettings";
const RESIZE_SCREEN: &str = "__resizeScreen";
const DEFAULT_LANGUAGE: &str = "en-us";
const LANGUAGE_SEPARATOR: &str = "-";
const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    ("zh-cn", "zh-chs"),
    ("zh-sg", "zh-chs"),
    ("zh-tw", "zh-cht"),
    ("zh-hk", "zh-cht"),
    ("zh-mo", "zh-cht"),
    ("no", "nb-no"),
    ("no-no", "nb-no"),
];
const LOCALE_ENV: &[&str] = &["LC_ALL", "LC_MESSAGES", "LANG"];
const LANGUAGE_CODES: &[&str] = &[
    "ar-sa", "be-by", "bg-bg", "cs-cz", "da-dk", "de-de", "el-gr", "en-us", "es-es", "eu-es",
    "fa-ir", "fi-fi", "fr-fr", "he-il", "hu-hu", "id-id", "it-it", "ja-jp", "ko-kr", "lt-lt",
    "nb-no", "nl-nl", "pl-pl", "pt-br", "pt-pt", "ro-ro", "ru-ru", "sk-sk", "sl-si", "sv-se",
    "th-th", "tr-tr", "uk-ua", "vi-vn", "zh-chs", "zh-cht",
];

pub(super) const HEAP_LIMIT: usize = 128 * 1024 * 1024;
pub(super) const MAX_SCRIPT_BINDINGS: usize = 4096;
pub(super) const MAX_SCRIPT_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_DELTA_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct Load<'a> {
    pub storage: Option<super::Storage>,
    pub images: Option<crate::model::ImageResources>,
    pub keyframes: Option<&'a crate::keyframe::Bindings>,
    pub screen_resolution: Option<[u32; 2]>,
    pub sprites: Option<&'a [(usize, Vec<f32>)]>,
    pub puppets: Option<crate::puppet::playback::Playback>,
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SoundOp {
    Play,
    Stop,
    Pause,
    Gain(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpriteOp {
    Play,
    Stop,
    Pause,
    Frame(usize),
    Rate(f32),
    Join,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParticleOp {
    Play,
    Stop,
    Pause,
    Emit(u32),
    Size(f32),
    Alpha(f32),
    Count(f32),
    Rate(f32),
    Speed(f32),
    Lifetime(f32),
    Color([f32; 3]),
    ControlPoint(usize, [f32; 3]),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptCommand {
    Video { object: usize, op: super::VideoOp },
    Shortcut { name: String },
    Sort { order: Vec<usize> },
    Sound { id: String, op: SoundOp },
    Sprite { object: usize, op: SpriteOp },
    Particle { id: String, op: ParticleOp },
    EffectFunction { id: String, effect: usize, name: String },
    Material { id: String, effect: Option<(usize, usize)>, name: String, values: Vec<f32> },
}

#[expect(clippy::too_many_lines, reason = "SceneScript command protocol validation")]
fn parse_command(
    kind: &str,
    target: &str,
    op: &str,
    value: &Value,
    object_slots: &[usize],
) -> Option<ScriptCommand> {
    let number = value.as_f64().map(|v| v as f32);
    match kind {
        VIDEO_COMMAND => Some(ScriptCommand::Video {
            object: target.parse().ok()?,
            op: super::VideoOp::parse(op, value)?,
        }),
        SHORTCUT_COMMAND => (!target.is_empty() && target.len() <= MAX_SHORTCUT_NAME_BYTES)
            .then(|| ScriptCommand::Shortcut { name: target.to_owned() }),
        SORT_COMMAND => {
            let order: Vec<usize> = serde_json::from_value(value.clone()).ok()?;
            let mut seen = vec![false; object_slots.iter().max().map_or(0, |slot| slot + 1)];
            for &slot in object_slots {
                seen[slot] = true;
            }
            if order.len() != object_slots.len()
                || order.iter().any(|&slot| {
                    seen.get_mut(slot).is_none_or(|seen| !std::mem::replace(seen, false))
                })
            {
                return None;
            }
            Some(ScriptCommand::Sort { order })
        }
        "sound" => {
            let op = match op {
                PLAY_COMMAND => SoundOp::Play,
                STOP_COMMAND => SoundOp::Stop,
                PAUSE_COMMAND => SoundOp::Pause,
                _ => return None,
            };
            Some(ScriptCommand::Sound { id: target.to_owned(), op })
        }
        "sprite" => {
            let op = match op {
                PLAY_COMMAND => SpriteOp::Play,
                STOP_COMMAND => SpriteOp::Stop,
                PAUSE_COMMAND => SpriteOp::Pause,
                "join" => SpriteOp::Join,
                "frame" => SpriteOp::Frame(number?.max(0.0) as usize),
                "rate" => SpriteOp::Rate(number?),
                _ => return None,
            };
            Some(ScriptCommand::Sprite { object: target.parse().ok()?, op })
        }
        EFFECT_COMMAND => {
            if op.is_empty() || op.len() > crate::effects::MAX_EFFECT_FUNCTION_NAME_BYTES {
                return None;
            }
            Some(ScriptCommand::EffectFunction {
                id: target.to_owned(),
                effect: value.as_u64()?.try_into().ok()?,
                name: op.to_owned(),
            })
        }
        MATERIAL_COMMAND | EFFECT_MATERIAL_COMMAND => {
            if target.is_empty() || op.is_empty() || op.len() > MAX_MATERIAL_NAME_BYTES {
                return None;
            }
            let (effect, values) = if kind == EFFECT_MATERIAL_COMMAND {
                let pair = value.as_array()?;
                if pair.len() != EFFECT_MATERIAL_PARTS {
                    return None;
                }
                (
                    Some((pair[0].as_u64()?.try_into().ok()?, pair[1].as_u64()?.try_into().ok()?)),
                    pair[2].as_array()?,
                )
            } else {
                (None, value.as_array()?)
            };
            if values.is_empty() || values.len() > MAX_MATERIAL_VALUES {
                return None;
            }
            let values: Vec<f32> = values
                .iter()
                .map(|entry| entry.as_f64().map(|number| number as f32))
                .collect::<Option<_>>()?;
            if values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(ScriptCommand::Material {
                id: target.to_owned(),
                effect,
                name: op.to_owned(),
                values,
            })
        }
        PARTICLE_COMMAND => {
            let vector = || {
                let vector: [f32; 3] = serde_json::from_value(value.clone()).ok()?;
                vector.iter().all(|component| component.is_finite()).then_some(vector)
            };
            let op = match op {
                PLAY_COMMAND => ParticleOp::Play,
                STOP_COMMAND => ParticleOp::Stop,
                PAUSE_COMMAND => ParticleOp::Pause,
                EMIT_COMMAND => ParticleOp::Emit(value.as_u64()?.try_into().ok()?),
                SIZE_COMMAND => ParticleOp::Size(number.filter(|value| value.is_finite())?),
                ALPHA_COMMAND => ParticleOp::Alpha(number.filter(|value| value.is_finite())?),
                COUNT_COMMAND => ParticleOp::Count(number.filter(|value| value.is_finite())?),
                RATE_COMMAND => ParticleOp::Rate(number.filter(|value| value.is_finite())?),
                SPEED_COMMAND => ParticleOp::Speed(number.filter(|value| value.is_finite())?),
                LIFETIME_COMMAND => ParticleOp::Lifetime(number.filter(|value| value.is_finite())?),
                COLORN_COMMAND => ParticleOp::Color(vector()?),
                _ => {
                    let suffix = op.strip_prefix(CONTROL_POINT_COMMAND)?;
                    if suffix.len() != 1 {
                        return None;
                    }
                    let index: usize = suffix.parse().ok()?;
                    if index >= crate::particles::CONTROL_POINT_COUNT {
                        return None;
                    }
                    ParticleOp::ControlPoint(index, vector()?)
                }
            };
            Some(ScriptCommand::Particle { id: target.to_owned(), op })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct CameraPose {
    pub eye: [f32; 3],
    pub center: [f32; 3],
    pub up: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct CameraTransforms {
    pub initialized: bool,
    pub oriented: bool,
    pub scripted: bool,
    pub pose: Option<CameraPose>,
    pub zoom: Option<f32>,
}

pub struct SceneScripts {
    limits: Limits,
    property_bindings: super::properties::PropertyBindings,
    properties_changed: bool,
    storage: Option<super::Storage>,
    commands: Vec<ScriptCommand>,
    layer_order: Vec<usize>,
    hidden: Vec<usize>,
    created: Rc<RefCell<Vec<(usize, crate::model::PreparedImage)>>>,
    created_particles: Rc<RefCell<Vec<(usize, crate::model::ParticleLayer)>>>,
    created_models: Rc<RefCell<Vec<(usize, crate::model::PreparedModel)>>>,
    created_slots: Vec<usize>,
    destroyed: Vec<usize>,
    retired_slots: BTreeSet<usize>,
    vacant_slots: BTreeSet<usize>,
    images: Option<Rc<crate::model::ImageResources>>,
    attachment_poses: Rc<RefCell<HashMap<(String, String), crate::puppet::AttachmentPose>>>,
    puppets: Rc<RefCell<crate::puppet::playback::Playback>>,
    general: Vec<(String, Value)>,
    effect_resets: Vec<usize>,
    context: Context,
    runtime: Runtime,
    deadline: Rc<Budget>,
    pub scene: Value,
    pub properties: crate::model::Properties,
    updates: Vec<usize>,
    pub diagnostics: Vec<String>,
    pub bindings: usize,
    disabled: bool,
    audio_registered: bool,
    work_pending: bool,
}

#[derive(serde::Serialize)]
struct AttachmentInfo<'a> {
    name: &'a str,
    index: usize,
    affine: [f32; 6],
    depth: f32,
}

fn js_error(ctx: &Ctx<'_>, error: &rquickjs::Error) -> anyhow::Error {
    let detail = ctx.catch();
    anyhow!("{error}: {detail:?}")
}

fn drain_jobs(ctx: &Ctx<'_>, deadline: &Budget) -> rquickjs::Result<()> {
    while ctx.execute_pending_job() {
        if Instant::now() >= deadline.get() {
            return Err(Exception::throw_message(ctx, JOB_BUDGET_EXHAUSTED));
        }
    }
    Ok(())
}

fn effect_visibility_slot(path: &str) -> Option<usize> {
    let path = path.strip_prefix(OBJECTS_PREFIX)?.strip_suffix(EFFECT_VISIBLE_SUFFIX)?;
    let (slot, effect) = path.split_once(EFFECTS_SEGMENT)?;
    effect.parse::<usize>().ok()?;
    slot.parse().ok()
}

fn language_code(locale: &str) -> &'static str {
    let code = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', LANGUAGE_SEPARATOR)
        .to_ascii_lowercase();
    let code = LANGUAGE_ALIASES
        .iter()
        .find_map(|&(alias, canonical)| (alias == code).then_some(canonical))
        .unwrap_or(&code);
    LANGUAGE_CODES
        .iter()
        .copied()
        .find(|supported| *supported == code)
        .or_else(|| {
            let language = code.split('-').next()?;
            LANGUAGE_CODES.iter().copied().find(|supported| {
                supported.starts_with(language)
                    && supported.as_bytes().get(language.len()) == Some(&b'-')
            })
        })
        .unwrap_or(DEFAULT_LANGUAGE)
}

fn system_language() -> &'static str {
    LOCALE_ENV
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.is_empty())
        .map_or(DEFAULT_LANGUAGE, |value| language_code(&value))
}

#[derive(serde::Serialize)]
struct GeneralSettings {
    language: &'static str,
}

#[cfg(test)]
mod locale_tests {
    use super::language_code;

    #[test]
    fn wallpaper_language_codes() {
        for (locale, expected) in [
            ("C.UTF-8", "en-us"),
            ("sv_SE.UTF-8", "sv-se"),
            ("zh_CN.UTF-8", "zh-chs"),
            ("zh_TW.UTF-8", "zh-cht"),
            ("en_GB.UTF-8", "en-us"),
        ] {
            assert_eq!(language_code(locale), expected);
        }
    }
}

impl SceneScripts {
    pub fn uses_runtime(scene: &Value) -> bool {
        source::present(scene) && source::needs_runtime(scene, "")
    }

    pub fn strip(scene: &mut Value, props: &crate::model::Properties) {
        source::collect(scene, "", &mut Vec::new(), props);
    }

    pub fn load(
        scene: &mut Value,
        props: &crate::model::Properties,
        project: &Value,
    ) -> Result<Option<Self>> {
        Self::load_with_storage(scene, props, project, Load::default())
    }

    pub fn load_with_storage(
        scene: &mut Value,
        props: &crate::model::Properties,
        project: &Value,
        options: Load<'_>,
    ) -> Result<Option<Self>> {
        if !Self::uses_runtime(scene) {
            return Ok(None);
        }
        let mut bindings = Vec::new();
        let original = scene.clone();
        source::collect(scene, "", &mut bindings, props);
        let property_bindings =
            super::properties::PropertyBindings::new(&original, project, &bindings);
        if bindings.len() > MAX_SCRIPT_BINDINGS
            || bindings.iter().map(|b| b.source.len()).sum::<usize>() > MAX_SCRIPT_SOURCE_BYTES
        {
            tracing::warn!("SceneScript: source exceeds the scene budget, scripts not run");
            return Ok(None);
        }
        let runtime = Runtime::new().map_err(|e| anyhow!("SceneScript runtime: {e}"))?;
        runtime.set_memory_limit(HEAP_LIMIT);
        runtime.set_max_stack_size(512 * 1024);
        let limits = options.limits;
        let deadline = Budget::new(limits.load);
        let interrupt = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= interrupt.get())));
        let context = Context::full(&runtime).map_err(|e| anyhow!("SceneScript context: {e}"))?;
        let mut host = Self {
            property_bindings,
            properties_changed: false,
            storage: options.storage,
            commands: Vec::new(),
            layer_order: (0..scene[OBJECTS_FIELD].as_array().map_or(0, Vec::len)).collect(),
            hidden: Vec::new(),
            created: Rc::new(RefCell::new(Vec::new())),
            created_particles: Rc::new(RefCell::new(Vec::new())),
            created_models: Rc::new(RefCell::new(Vec::new())),
            created_slots: Vec::new(),
            destroyed: Vec::new(),
            retired_slots: BTreeSet::new(),
            vacant_slots: BTreeSet::new(),
            images: options.images.map(Rc::new),
            attachment_poses: Rc::new(RefCell::new(HashMap::new())),
            puppets: Rc::new(RefCell::new(options.puppets.unwrap_or_default())),
            general: Vec::new(),
            effect_resets: Vec::new(),
            context,
            runtime,
            deadline,
            limits,
            scene: Value::Null,
            properties: props.clone(),
            updates: Vec::new(),
            diagnostics: Vec::new(),
            bindings: bindings.len(),
            disabled: false,
            audio_registered: false,
            work_pending: false,
        };
        let load = host.deadline.enter();
        let prepared =
            host.setup(scene, &original, props, options.screen_resolution).and_then(|()| {
                if let Some(sprites) = options.sprites {
                    host.set_sprites(sprites);
                }
                if let Some(keyframes) = options.keyframes {
                    host.set_keyframes(keyframes)?;
                }
                host.set_puppets()
            });
        if let Err(error) = prepared {
            let heap = host.heap_bytes();
            tracing::warn!("SceneScript: setup used {heap} heap bytes, scripts not run: {error:#}");
            return Ok(None);
        }
        host.scene = scene.clone();
        for (index, binding) in bindings.iter().enumerate() {
            if let Err(error) = host.compile(index, binding) {
                host.disable_module(index);
                if !host.deadline.exhausted() {
                    host.diagnostics.push(format!("{}: {error:#}", binding.path));
                    continue;
                }
                host.stop(format!("{} stopped scripts: {error:#}", binding.path));
            }
            if host.deadline.exhausted() {
                host.stop(LOAD_BUDGET_EXHAUSTED.to_owned());
                break;
            }
        }
        if !host.disabled {
            let init = host.context.with(|ctx| {
                ctx.globals()
                    .get::<_, Function>(EMIT_EVENT)
                    .and_then(|emit| emit.call::<_, ()>((INIT_EVENT,)))
                    .map_err(|error| js_error(&ctx, &error))
            });
            if let Err(error) = init {
                host.stop(format!("init event stopped: {error:#}"));
            }
        }
        if !host.disabled {
            host.apply_user_properties();
            host.apply_general_settings();
        }
        if let Err(error) = host.drain() {
            host.stop(format!("initial changes stopped: {error:#}"));
        }
        host.frame(0.0, 0.0, [0.5; 2], false);
        drop(load);
        host.deadline.renew(limits.compose);
        for diagnostic in &host.diagnostics {
            tracing::warn!("SceneScript: {diagnostic}");
        }
        tracing::info!(
            bindings = host.bindings,
            heap_bytes = host.heap_bytes(),
            "SceneScript runtime created"
        );
        *scene = host.scene.clone();
        Ok(Some(host))
    }

    #[expect(clippy::too_many_lines, reason = "SceneScript host binding initialization")]
    fn setup(
        &self,
        scene: &Value,
        original: &Value,
        props: &crate::model::Properties,
        screen_resolution: Option<[u32; 2]>,
    ) -> Result<()> {
        let mut resolved = scene.clone();
        source::resolve_wrappers(&mut resolved, props);
        let created = self.created.clone();
        let created_particles = self.created_particles.clone();
        let created_models = self.created_models.clone();
        let images = self.images.clone();
        let deadline = self.deadline.clone();
        let resources_budget = self.deadline.clone();
        let draining = Cell::new(false);
        self.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<()> {
                ctx.globals().set(
                    RUN_JOBS,
                    Function::new(ctx.clone(), move |ctx: Ctx<'_>| {
                        if draining.replace(true) {
                            return Ok(());
                        }
                        let result = drain_jobs(&ctx, &deadline);
                        draining.set(false);
                        result
                    })?,
                )?;
                ctx.globals().set(
                    "__log",
                    Function::new(ctx.clone(), |message: String| {
                        tracing::debug!(
                            "SceneScript: {}",
                            message.chars().take(1024).collect::<String>()
                        );
                    })?,
                )?;
                ctx.globals().set(MAX_SCENE_OBJECTS_JS, crate::model::MAX_SCENE_OBJECTS)?;
                ctx.globals().set(MAX_MATERIAL_VALUES_JS, MAX_MATERIAL_VALUES)?;
                ctx.globals().set(
                    VALIDATE_IMAGE,
                    Function::new(ctx.clone(), move |json: String, slot: usize| -> String {
                        let result = (|| -> std::result::Result<Vec<f32>, String> {
                            let _work = resources_budget.host().map_err(str::to_owned)?;
                            let images = images
                                .as_ref()
                                .ok_or_else(|| MISSING_IMAGE_RESOURCES.to_owned())?;
                            let scene = serde_json::from_str::<Value>(&json)
                                .map_err(|error| error.to_string())?;
                            if scene[OBJECTS_FIELD][slot].get(MESH_FIELD).is_some() {
                                let model = images
                                    .load_model(&scene, slot)
                                    .map_err(|error| error.to_string())?;
                                created_models.borrow_mut().push((slot, model));
                                return Ok(Vec::new());
                            }
                            if scene[OBJECTS_FIELD][slot]
                                .get(crate::model::PARTICLE_FIELD)
                                .is_some()
                            {
                                let particle =
                                    images.load_particle(&scene, slot).map_err(|error| {
                                        format!("SceneScript dynamic particle: {error:#}")
                                    })?;
                                created_particles.borrow_mut().push((slot, particle));
                                return Ok(Vec::new());
                            }
                            let image = images
                                .load(&scene, slot)
                                .map_err(|error| format!("SceneScript dynamic image: {error:#}"))?;
                            let times =
                                image.layer.atlas_frames().map_or_else(Vec::new, |frames| {
                                    frames.iter().map(|frame| frame.time).collect()
                                });
                            created.borrow_mut().push((slot, image));
                            Ok(times)
                        })();
                        serde_json::to_string(&result).expect("image validation")
                    })?,
                )?;
                self.bind_materials(&ctx)?;
                self.bind_models(&ctx)?;
                self.bind_videos(&ctx)?;
                self.bind_attachments(&ctx)?;
                ctx.eval::<(), _>(include_str!("bootstrap.js"))?;
                ctx.globals().set(MATRIX_ATAN2, Function::new(ctx.clone(), libm::atan2)?)?;
                ctx.eval::<(), _>(include_str!("matrix.js"))?;
                ctx.eval::<(), _>(include_str!("storage.js"))?;
                if let Some(storage) = &self.storage {
                    ctx.globals()
                        .get::<_, Function>("__storageLoad")?
                        .call::<_, ()>((storage.data.to_string(),))?;
                }
                let setup: Function = ctx.globals().get("__setup")?;
                let values = self.property_bindings.user_properties(props);
                let layers: Vec<String> =
                    original[OBJECTS_FIELD].as_array().map_or_else(Vec::new, |objects| {
                        objects.iter().map(Value::to_string).collect()
                    });
                setup.call::<_, ()>((
                    layers,
                    resolved.to_string(),
                    values.to_string(),
                    screen_resolution.map(|size| size[0]),
                    screen_resolution.map(|size| size[1]),
                ))?;
                for (name, source) in [
                    ("WEColor", include_str!("color.js")),
                    ("WEMath", include_str!("math.js")),
                    ("WEVector", include_str!("vector.js")),
                ] {
                    let (_, p) = Module::declare(ctx.clone(), name, source)?.eval()?;
                    p.finish::<()>()?;
                }
                Ok(())
            })();
            result.map_err(|e| js_error(&ctx, &e))
        })
    }

    fn bind_materials(&self, ctx: &Ctx<'_>) -> rquickjs::Result<()> {
        let images = self.images.clone();
        let budget = self.deadline.clone();
        ctx.globals().set(
            EFFECT_MATERIALS,
            Function::new(ctx.clone(), move |json: String| -> String {
                let result =
                    (|| -> std::result::Result<crate::model::EffectMaterialInfo, String> {
                        let _work = budget.host().map_err(str::to_owned)?;
                        let images =
                            images.as_ref().ok_or_else(|| MISSING_IMAGE_RESOURCES.to_owned())?;
                        let object = serde_json::from_str::<Value>(&json)
                            .map_err(|error| error.to_string())?;
                        images.effect_materials(&object).map_err(|error| error.to_string())
                    })();
                serde_json::to_string(&result).expect("effect materials")
            })?,
        )
    }

    fn bind_videos(&self, ctx: &Ctx<'_>) -> rquickjs::Result<()> {
        let resources = self.images.clone();
        let budget = self.deadline.clone();
        ctx.globals().set(
            VIDEO_TEXTURE,
            Function::new(ctx.clone(), move |json: String| -> bool {
                let Ok(_work) = budget.host() else { return false };
                let Some(resources) = &resources else { return false };
                serde_json::from_str::<Value>(&json)
                    .is_ok_and(|object| resources.video_texture(&object))
            })?,
        )
    }

    pub fn set_videos(&mut self, states: &[super::VideoState], committed: bool) {
        if self.disabled {
            return;
        }
        let _entry = self.deadline.enter();
        let json = serde_json::to_string(states).expect("video states");
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(VIDEO_STATES)
                .and_then(|function| function.call::<_, ()>((json, committed)))
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = result {
            self.stop(format!("video states stopped: {error:#}"));
            return;
        }
        self.refresh_activity();
    }

    fn bind_models(&self, ctx: &Ctx<'_>) -> rquickjs::Result<()> {
        let resources = self.images.clone();
        let budget = self.deadline.clone();
        ctx.globals().set(
            CREATE_MODEL,
            Function::new(ctx.clone(), move |id: usize, json: String| -> String {
                let _work = match budget.host() {
                    Ok(work) => work,
                    Err(error) => return error.to_owned(),
                };
                let Some(resources) = &resources else {
                    return MISSING_IMAGE_RESOURCES.into();
                };
                serde_json::from_str::<crate::model::mesh::ModelData>(&json)
                    .map_err(anyhow::Error::from)
                    .and_then(|data| resources.create_model(id, data))
                    .err()
                    .map_or_else(String::new, |error| error.to_string())
            })?,
        )?;
        let resources = self.images.clone();
        ctx.globals().set(
            DESTROY_MODEL,
            Function::new(ctx.clone(), move |id: usize| {
                if let Some(resources) = &resources {
                    resources.destroy_model(id);
                }
            })?,
        )?;
        Ok(())
    }

    fn bind_attachments(&self, ctx: &Ctx<'_>) -> rquickjs::Result<()> {
        let images = self.images.clone();
        let current = self.attachment_poses.clone();
        let puppets = self.puppets.clone();
        let budget = self.deadline.clone();
        let cache =
            RefCell::new(HashMap::<String, Vec<(String, crate::puppet::AttachmentPose)>>::new());
        ctx.globals().set(
            ATTACHMENT_INFO,
            Function::new(
                ctx.clone(),
                move |owner: String, path: String, name: String, index: Option<usize>| -> String {
                    let Ok(_work) = budget.host() else { return ATTACHMENT_MISSING.into() };
                    if !cache.borrow().contains_key(&path) {
                        let poses = images
                            .as_ref()
                            .and_then(|images| images.puppet_attachment_poses(&path).ok())
                            .unwrap_or_default();
                        if !poses.is_empty() {
                            cache.borrow_mut().insert(path.clone(), poses);
                        }
                    }
                    let known = cache.borrow();
                    let Some((index, (name, rest))) = known.get(&path).and_then(|poses| {
                        index
                            .filter(|index| *index < poses.len())
                            .or_else(|| poses.iter().position(|(candidate, _)| candidate == &name))
                            .map(|index| (index, &poses[index]))
                    }) else {
                        return ATTACHMENT_MISSING.into();
                    };
                    let pose = puppets
                        .borrow()
                        .attachment_pose(&owner, name)
                        .or_else(|| current.borrow().get(&(owner, name.clone())).copied())
                        .unwrap_or(*rest);
                    serde_json::to_string(&AttachmentInfo {
                        name,
                        index,
                        affine: [
                            pose.affine.a,
                            pose.affine.b,
                            pose.affine.c,
                            pose.affine.d,
                            pose.affine.x,
                            pose.affine.y,
                        ],
                        depth: pose.depth,
                    })
                    .expect("attachment info")
                },
            )?,
        )
    }

    pub fn set_attachment_poses(&mut self, poses: &crate::model::Attachments<'_>) {
        let mut current = self.attachment_poses.borrow_mut();
        current.clear();
        current.extend(
            poses.iter().map(|(&(owner, name), &pose)| ((owner.into(), name.into()), pose)),
        );
    }

    fn compile(&mut self, index: usize, binding: &source::Binding) -> Result<()> {
        let mut properties = binding.properties.clone();
        source::resolve_wrappers(&mut properties, &self.properties);
        let (has_update, init_error) = self.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<(bool, Option<anyhow::Error>)> {
                let layer = binding.layer.map_or("undefined".to_owned(), |i| format!("__layers[{i}]"));
                let source = format!("let thisLayer = {layer}; let thisObject = __scriptObject({}); const createScriptProperties = () => __createScriptProperties({properties}, {index});\n{}", serde_json::to_string(&binding.path).unwrap(), binding.source);
                if binding.source.contains(BONE_WORLD_METHODS) {
                    ctx.globals().set(BONE_WORLD_READERS, true)?;
                }
                ctx.globals().get::<_, Function>(PREPARE_MODULE)?.call::<_, ()>((index, binding.path.as_str()))?;
                let (module, promise) = Module::declare(ctx.clone(), format!("scene-{index}.js"), source)?.eval()?;
                ctx.globals().get::<_, Function>(RUN_JOBS)?.call::<_, ()>(())?;
                promise.finish::<()>()?;
                let namespace = module.namespace()?;
                let has_update = namespace.get::<_, rquickjs::Value>("update")?.is_function();
                let register: Function = ctx.globals().get("__register")?;
                register.call::<_, ()>((index, namespace, binding.path.as_str()))?;
                let invoke: Function = ctx.globals().get("__invoke")?;
                let init_error =
                    invoke.call::<_, ()>((index, "init")).err().map(|error| js_error(&ctx, &error));
                Ok((has_update, init_error))
            })();
            result.map_err(|e| js_error(&ctx,&e))
        })?;
        if let Some(error) = init_error {
            if self.deadline.exhausted() {
                return Err(error);
            }
            self.diagnostics.push(format!("{}: {error:#}", binding.path));
        }
        if has_update {
            self.updates.push(index);
        }
        Ok(())
    }

    fn apply_user_properties(&mut self) {
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__applyUserProperties")
                .and_then(|f| f.call::<_, ()>(()))
                .map_err(|e| js_error(&ctx, &e))
        });
        if let Err(error) = result {
            self.diagnostics.push(format!("applyUserProperties: {error:#}"));
        }
    }

    fn apply_general_settings(&mut self) {
        let _entry = self.deadline.enter();
        let settings = serde_json::to_string(&GeneralSettings { language: system_language() })
            .expect("general settings serialize");
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(GENERAL_SETTINGS)
                .and_then(|function| function.call::<_, ()>((settings,)))
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = result {
            self.diagnostics.push(format!("applyGeneralSettings: {error:#}"));
        }
    }

    pub fn resize_screen(&mut self, resolution: [u32; 2]) -> bool {
        if self.disabled {
            return false;
        }
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(RESIZE_SCREEN)
                .and_then(|function| function.call::<_, ()>((resolution[0], resolution[1])))
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = result {
            self.stop(format!("resizeScreen stopped: {error:#}"));
            return false;
        }
        match self.drain() {
            Ok(changed) => changed,
            Err(error) => {
                self.stop(format!("resizeScreen changes stopped: {error:#}"));
                false
            }
        }
    }

    pub(crate) fn restrict_property_updates(
        &mut self,
        package: &crate::pkg::Package,
        assets: &crate::effects::Assets,
    ) {
        self.property_bindings.restrict_package(package);
        self.property_bindings.restrict_assets(assets.property_dependencies().as_ref());
    }

    pub fn update_properties(&mut self, overrides: &crate::model::Properties) -> Result<bool> {
        if self.disabled {
            return Ok(false);
        }
        let desired = self.property_bindings.desired(overrides);
        let Some(update) = self.property_bindings.update(&self.properties, &desired) else {
            return Ok(false);
        };
        let _entry = self.deadline.enter();
        self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__updateUserProperties")
                .and_then(|function| function.call::<_, ()>((update.to_string(),)))
                .map_err(|error| js_error(&ctx, &error))
        })?;
        self.properties = desired;
        self.properties_changed = true;
        self.refresh_activity();
        Ok(true)
    }

    pub fn restrict_hidden_layer_updates(&mut self, layers: &[String]) {
        if layers.is_empty() {
            return;
        }
        let _entry = self.deadline.enter();
        let callbacks = self.context.with(|ctx| {
            ctx.eval::<bool, _>(
                "__modules.some(m => m && !m.disabled && typeof m.ns.applyUserProperties === 'function')",
            )
            .unwrap_or(true)
        });
        self.property_bindings.restrict_hidden_layers(&self.scene, layers, callbacks);
    }

    fn set_keyframes(&mut self, bindings: &crate::keyframe::Bindings) -> Result<()> {
        let _entry = self.deadline.enter();
        let paths = serde_json::to_string(&bindings.descriptors())?;
        let clocks = bindings.clocks.clone();
        self.context
            .with(|ctx| {
                ctx.globals().set(
                    TIMELINE_CONTROL,
                    Function::new(ctx.clone(), move |index: usize, op: usize, value: f32| {
                        crate::keyframe::Control::ALL
                            .get(op)
                            .and_then(|op| clocks.borrow_mut().control(index, *op, value))
                    })?,
                )?;
                ctx.globals().get::<_, Function>("__setKeyframes")?.call::<_, ()>((paths,))
            })
            .map_err(|error| anyhow!("keyframe bindings: {error}"))
    }

    fn set_puppets(&mut self) -> Result<()> {
        let descriptors = serde_json::to_string(&self.puppets.borrow().descriptors())?;
        let bones = {
            let puppets = self.puppets.borrow();
            serde_json::to_string(
                &puppets
                    .bone_descriptors()
                    .iter()
                    .map(|(slot, bones)| (slot, bones, puppets.has_springs(*slot)))
                    .collect::<Vec<_>>(),
            )?
        };
        let puppets = self.puppets.clone();
        let visibility = self.puppets.clone();
        let blend = self.puppets.clone();
        let create = self.puppets.clone();
        let destroy = self.puppets.clone();
        let bone_get = self.puppets.clone();
        let bone_set = self.puppets.clone();
        let springs = self.puppets.clone();
        let morph = self.puppets.clone();
        let shapes = serde_json::to_string(&self.puppets.borrow().morph_descriptors())?;
        self.context
            .with(|ctx| {
                ctx.globals().set(
                    PUPPET_CONTROL,
                    Function::new(ctx.clone(), move |index: usize, op: usize, value: f32| {
                        crate::keyframe::Control::ALL
                            .get(op)
                            .and_then(|op| puppets.borrow_mut().control(index, *op, value))
                    })?,
                )?;
                ctx.globals().set(
                    PUPPET_VISIBILITY,
                    Function::new(ctx.clone(), move |index: usize, value: Option<bool>| {
                        visibility.borrow_mut().visibility(index, value)
                    })?,
                )?;
                ctx.globals().set(
                    PUPPET_BLEND,
                    Function::new(ctx.clone(), move |index: usize, value: Option<f32>| {
                        blend.borrow_mut().blend(index, value)
                    })?,
                )?;
                ctx.globals().set(
                    PUPPET_CREATE,
                    Function::new(
                        ctx.clone(),
                        move |slot: usize, name: String, single: bool, json: String| {
                            let Ok(transition) = serde_json::from_str(&json) else {
                                return ATTACHMENT_MISSING.to_owned();
                            };
                            let mut playback = create.borrow_mut();
                            let Some(index) = playback.create(slot, &name, single, transition)
                            else {
                                return ATTACHMENT_MISSING.to_owned();
                            };
                            serde_json::to_string(
                                &playback
                                    .descriptors()
                                    .into_iter()
                                    .find(|descriptor| descriptor.clock == index),
                            )
                            .unwrap_or_else(|_| ATTACHMENT_MISSING.to_owned())
                        },
                    )?,
                )?;
                ctx.globals().set(
                    PUPPET_DESTROY,
                    Function::new(ctx.clone(), move |index: usize| {
                        destroy.borrow_mut().destroy(index)
                    })?,
                )?;
                ctx.globals().set(
                    PUPPET_BONE_GET,
                    Function::new(ctx.clone(), move |slot: usize, index: usize, local: bool| {
                        serde_json::to_string(
                            &bone_get.borrow_mut().bone_matrix(slot, index, local),
                        )
                        .unwrap_or_else(|_| ATTACHMENT_MISSING.to_owned())
                    })?,
                )?;
                ctx.globals().set(
                    PUPPET_BONE_SET,
                    Function::new(
                        ctx.clone(),
                        move |slot: usize, index: usize, local: bool, json: String| {
                            let Ok((matrix, owner)) = serde_json::from_str::<(
                                crate::matrix::Mat4,
                                Option<crate::matrix::Mat4>,
                            )>(&json) else {
                                return false;
                            };
                            let mut puppets = bone_set.borrow_mut();
                            if owner.as_ref().is_some_and(|matrix| {
                                !crate::puppet::spring::owner_supported(matrix)
                            }) || !puppets.set_bone_matrix(slot, index, matrix, local)
                            {
                                return false;
                            }
                            if let Some(owner) = owner {
                                puppets.record_springs(slot, index, local, &owner);
                            }
                            true
                        },
                    )?,
                )?;
                ctx.globals().set(
                    PUPPET_SPRING_STEP,
                    Function::new(ctx.clone(), move |slot: usize, json: String, dt: f32| {
                        let Ok(owner) = serde_json::from_str::<crate::matrix::Mat4>(&json) else {
                            return false;
                        };
                        springs.borrow_mut().step_springs(slot, owner, dt)
                    })?,
                )?;
                ctx.globals().set(
                    PUPPET_MORPH,
                    Function::new(
                        ctx.clone(),
                        move |slot: usize, index: usize, value: Option<f32>| {
                            morph.borrow_mut().morph_weight(slot, index, value)
                        },
                    )?,
                )?;
                ctx.globals().get::<_, Function>(SET_PUPPETS)?.call::<_, ()>((
                    descriptors,
                    bones,
                    shapes,
                ))
            })
            .map_err(|error| anyhow!("puppet bindings: {error}"))
    }

    pub fn has_puppets(&self) -> bool {
        !self.puppets.borrow().is_empty()
    }

    pub fn puppet_sample(
        &self,
        object: &str,
        animation: u32,
    ) -> Option<crate::puppet::playback::Sample> {
        self.puppets.borrow().sample(object, animation)
    }

    pub fn puppet_states(&self, object: &str) -> Vec<crate::puppet::playback::State> {
        self.puppets.borrow().states(object)
    }

    pub fn puppet_owner_retired(&self, object: &str) -> bool {
        self.puppets.borrow().owner_retired(object)
    }

    pub fn puppet_bone_palette(&self, object: &str) -> Option<(u64, Vec<crate::matrix::Mat4>)> {
        self.puppets.borrow().bone_palette(object)
    }

    pub fn puppet_bone_world(&self, object: &str) -> Option<(u64, Vec<crate::matrix::Mat4>)> {
        self.puppets.borrow().bone_world(object)
    }

    pub fn puppet_morph_weights(&self, object: &str) -> Option<Vec<f32>> {
        self.puppets.borrow().morph_weights(object)
    }

    pub fn puppet_bone_revision(&self, object: &str) -> u64 {
        self.puppets.borrow().bone_revision(object)
    }

    pub fn sync_keyframes(&mut self, bindings: &crate::keyframe::Bindings) {
        if self.disabled || bindings.is_empty() {
            return;
        }
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__keyframe")
                .and_then(|update| {
                    for (index, values, count) in bindings.values() {
                        update.call::<_, ()>((
                            index, values[0], values[1], values[2], values[3], count,
                        ))?;
                    }
                    Ok(())
                })
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = result {
            self.stop(format!("keyframe values stopped: {error:#}"));
        }
    }

    pub fn set_sprites(&mut self, sprites: &[(usize, Vec<f32>)]) {
        if self.disabled || sprites.is_empty() {
            return;
        }
        let json = serde_json::to_string(sprites).unwrap_or_else(|_| "[]".into());
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__setSprites")
                .and_then(|f| f.call::<_, ()>((json,)))
                .map_err(|e| js_error(&ctx, &e))
        });
        if let Err(error) = result {
            self.stop(format!("sprite table stopped: {error:#}"));
        }
    }

    pub fn set_particle_states(&mut self, states: &[(String, bool)]) {
        if !states.is_empty() {
            self.set_playing(SET_PARTICLES, states);
        }
    }

    pub fn set_sound_states(&mut self, states: &[(String, bool)]) {
        self.set_playing(SET_SOUNDS, states);
    }

    fn set_playing(&mut self, receiver: &str, states: &[(String, bool)]) {
        if self.disabled {
            return;
        }
        let json = serde_json::to_string(states).expect("playing states");
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(receiver)
                .and_then(|function| function.call::<_, ()>((json,)))
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = result {
            self.stop(format!("{receiver} stopped: {error:#}"));
        }
    }

    pub fn take_camera(&mut self) -> Option<CameraTransforms> {
        if self.disabled {
            return None;
        }
        let _entry = self.deadline.enter();
        let drained = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(DRAIN_CAMERA)
                .and_then(|drain| drain.call::<_, String>(()))
                .map_err(|error| js_error(&ctx, &error))
        });
        let parsed = drained.and_then(|json| {
            if json.is_empty() {
                return Ok(None);
            }
            Ok(Some(serde_json::from_str(&json)?))
        });
        match parsed {
            Ok(camera) => camera,
            Err(error) => {
                self.stop(format!("camera stopped: {error:#}"));
                None
            }
        }
    }

    pub fn discard_created(&mut self, slot: usize) {
        if self.disabled {
            if let Some(images) = &self.images {
                images.release(slot);
            }
            return;
        }
        let _entry = self.deadline.enter();
        let result = self
            .context
            .with(|ctx| {
                ctx.globals()
                    .get::<_, Function>(DISCARD_LAYER)
                    .and_then(|discard| discard.call::<_, ()>((slot,)))
                    .map_err(|error| js_error(&ctx, &error))
            })
            .and_then(|()| self.drain_lifecycle());
        if let Err(error) = result {
            self.stop(format!("layer discard stopped: {error:#}"));
        }
    }

    pub fn take_commands(&mut self) -> Vec<ScriptCommand> {
        std::mem::take(&mut self.commands)
    }

    pub fn take_created_images(&mut self) -> Vec<(usize, crate::model::PreparedImage)> {
        let queued = std::mem::take(&mut *self.created.borrow_mut());
        queued
            .into_iter()
            .filter_map(|(slot, image)| {
                if self.created_slots.binary_search(&slot).is_ok()
                    && self.scene[OBJECTS_FIELD].get(slot).is_some()
                {
                    Some((slot, image))
                } else {
                    if let Some(images) = &self.images {
                        images.release(slot);
                    }
                    None
                }
            })
            .collect()
    }

    pub fn take_created_particles(&mut self) -> Vec<(usize, crate::model::ParticleLayer)> {
        std::mem::take(&mut *self.created_particles.borrow_mut())
            .into_iter()
            .filter_map(|(slot, particle)| {
                if self.created_slots.binary_search(&slot).is_ok()
                    && self.scene[OBJECTS_FIELD].get(slot).is_some()
                {
                    Some((slot, particle))
                } else {
                    if let Some(images) = &self.images {
                        images.release(slot);
                    }
                    None
                }
            })
            .collect()
    }

    pub fn visit_created_passes(&self, mut visit: impl FnMut(&crate::effects::EffectPass)) {
        for (_, model) in self.created_models.borrow().iter() {
            model.layer.materials.iter().for_each(|material| visit(&material.pass));
        }
    }

    pub fn has_created_models(&self) -> bool {
        !self.created_models.borrow().is_empty()
    }

    pub fn has_models(&self) -> bool {
        self.images.as_ref().is_some_and(|resources| resources.has_models())
    }

    pub fn take_created_models(&mut self) -> Vec<(usize, crate::model::PreparedModel)> {
        std::mem::take(&mut *self.created_models.borrow_mut())
            .into_iter()
            .filter_map(|(slot, model)| {
                if self.created_slots.binary_search(&slot).is_ok() {
                    return Some((slot, model));
                }
                if let Some(resources) = &self.images {
                    resources.release(slot);
                }
                None
            })
            .collect()
    }

    pub fn created_layer_slots(&self) -> &[usize] {
        &self.created_slots
    }

    pub fn reserve_image_bytes(&self, bytes: usize, slots: &[(usize, usize)]) -> Result<()> {
        if let Some(images) = &self.images {
            images.initialize(bytes, slots)?;
        }
        Ok(())
    }

    pub fn image_resources(&self) -> Option<Rc<crate::model::ImageResources>> {
        self.images.clone()
    }

    pub fn take_destroyed_images(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.destroyed)
    }

    pub fn acknowledge_destroyed(&mut self, slots: &[usize]) {
        if self.disabled {
            return;
        }
        if let Err(error) = self.release_slots(slots) {
            self.stop(format!("layer release stopped: {error:#}"));
        }
    }

    pub(super) fn release_slots(&mut self, slots: &[usize]) -> Result<()> {
        if slots.iter().any(|slot| {
            self.created_slots.binary_search(slot).is_err() || !self.hidden.contains(slot)
        }) {
            return Err(anyhow!("SceneScript retired object slot is invalid"));
        }
        self.retired_slots.extend(slots.iter().copied());
        if self.retired_slots.is_empty() {
            return Ok(());
        }
        let json = serde_json::to_string(&self.retired_slots)?;
        let _entry = self.deadline.enter();
        let released: String = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(RELEASE_LAYERS)
                .and_then(|release| release.call((json,)))
                .map_err(|error| js_error(&ctx, &error))
        })?;
        let released: Vec<usize> = serde_json::from_str(&released)?;
        for slot in released {
            if !self.retired_slots.remove(&slot) {
                return Err(anyhow!("SceneScript released object slot is invalid"));
            }
            let id = self.scene[OBJECTS_FIELD][slot][ID_FIELD].clone();
            self.scene[OBJECTS_FIELD][slot] = serde_json::json!({ID_FIELD:id,VISIBLE_FIELD:false});
            self.hidden.retain(|value| *value != slot);
            self.layer_order.retain(|value| *value != slot);
            for command in &mut self.commands {
                if let ScriptCommand::Sort { order } = command {
                    order.retain(|value| *value != slot);
                }
            }
            self.created_slots.retain(|value| *value != slot);
            self.effect_resets.retain(|value| *value != slot);
            self.vacant_slots.insert(slot);
        }
        Ok(())
    }

    pub fn accept_layer_order(&mut self, order: Vec<usize>) {
        self.layer_order = order;
    }

    pub fn reject_layer_order(&mut self) {
        if self.disabled {
            return;
        }
        let json = serde_json::to_string(&self.layer_order).expect("layer order");
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(RESTORE_LAYER_ORDER)
                .and_then(|restore| restore.call::<_, ()>((json,)))
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = result {
            self.stop(format!("layer order stopped: {error:#}"));
            return;
        }
        if !self.diagnostics.iter().any(|message| message == SORT_REJECTED) {
            tracing::warn!("SceneScript: {SORT_REJECTED}");
            self.diagnostics.push(SORT_REJECTED.into());
        }
    }

    pub fn take_general_changes(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.general)
    }

    pub fn take_effect_resets(&mut self) -> Vec<usize> {
        std::mem::take(&mut self.effect_resets)
    }

    pub fn destroy(&mut self) {
        if self.disabled {
            return;
        }
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__destroyAll")
                .and_then(|f| f.call::<_, ()>(()))
                .map_err(|e| js_error(&ctx, &e))
        });
        match result.and_then(|()| self.drain()) {
            Ok(_) => {}
            Err(error) => self.diagnostics.push(format!("destroy: {error:#}")),
        }
    }

    pub fn needs_audio(&self) -> bool {
        !self.disabled && self.audio_registered
    }

    pub fn audio(&mut self, count: usize, left: &[f32], right: &[f32]) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__audioUpdate")
                .and_then(|f| f.call::<_, ()>((count, left.to_vec(), right.to_vec())))
                .map_err(|e| js_error(&ctx, &e))
        });
        if let Err(error) = result {
            self.stop(format!("audio stopped: {error:#}"));
        }
        Ok(())
    }

    pub fn needs_media(&self) -> bool {
        if self.disabled {
            return false;
        }
        let _entry = self.deadline.enter();
        self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__needsMedia")
                .and_then(|f| f.call::<_, bool>(()))
                .unwrap_or(false)
        })
    }

    pub fn media(&mut self, events: &Value) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__media")
                .and_then(|f| f.call::<_, ()>((events.to_string(),)))
                .map_err(|e| js_error(&ctx, &e))
        });
        if let Err(error) = result {
            self.stop(format!("media event stopped: {error:#}"));
        }
        self.refresh_activity();
        Ok(())
    }

    pub fn pointer(
        &mut self,
        position: [f32; 2],
        buttons: [bool; 3],
        hits: Vec<String>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        let _entry = self.deadline.enter();
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__pointer")
                .and_then(|f| f.call::<_, ()>((position[0], position[1], buttons.to_vec(), hits)))
                .map_err(|e| js_error(&ctx, &e))
        });
        if let Err(error) = result {
            self.stop(format!("cursor event stopped: {error:#}"));
        }
        self.refresh_activity();
        Ok(())
    }

    pub fn animated(&self) -> bool {
        !self.disabled
            && (!self.updates.is_empty() || self.work_pending || self.puppets.borrow().animated())
    }

    pub fn heap_bytes(&self) -> usize {
        self.runtime.memory_usage().memory_used_size.max(0) as usize
    }

    pub fn tick(&mut self, time: f32, dt: f32, pointer: [f32; 2]) -> Result<bool> {
        Ok(self.frame(time, dt, pointer, true))
    }

    fn frame(&mut self, time: f32, dt: f32, pointer: [f32; 2], updates: bool) -> bool {
        if self.disabled {
            return self.hide_layers();
        }
        if updates {
            self.puppets.borrow_mut().bone_frame(dt);
        } else {
            self.puppets.borrow_mut().advance(dt);
        }
        let ended = self.puppets.borrow_mut().take_ended();
        let events = self.puppets.borrow_mut().take_events();
        let started = Instant::now();
        let _entry = self.deadline.enter();
        let expected = if updates { self.updates.len() } else { 0 };
        let mut invoked = 0;
        let mut failed = Vec::new();
        let result = self.context.clone().with(|ctx| {
            let result = (|| -> rquickjs::Result<()> {
                let frame: Function = ctx.globals().get("__frame")?;
                frame.call::<_, ()>((time, dt, pointer[0], pointer[1], updates))?;
                if !events.is_empty() {
                    let callback: Function = ctx.globals().get(PUPPET_EVENT)?;
                    for event in events {
                        callback.call::<_, ()>((event.slot, event.name, event.frame))?;
                    }
                }
                if !ended.is_empty() {
                    let callback: Function = ctx.globals().get(PUPPET_ENDED)?;
                    for index in ended {
                        callback.call::<_, ()>((index,))?;
                    }
                }
                let invoke: Function = ctx.globals().get("__invoke")?;
                for &index in if updates { &self.updates[..] } else { &[] } {
                    if Instant::now() >= self.deadline.get() {
                        break;
                    }
                    invoked += 1;
                    if let Err(error) = invoke.call::<_, ()>((index, "update")) {
                        let error = js_error(&ctx, &error);
                        self.diagnostics.push(format!("script {index} stopped: {error:#}"));
                        tracing::warn!("SceneScript {index} stopped: {error:#}");
                        failed.push(index);
                    }
                }
                self.updates.retain(|index| !failed.contains(index));
                if updates {
                    ctx.globals().get::<_, Function>(EMIT_EVENT)?.call::<_, ()>((UPDATE_EVENT,))?;
                }
                if Instant::now() >= self.deadline.get() {
                    return Ok(());
                }
                let timers: Function = ctx.globals().get("__tickTimers")?;
                timers.call::<_, ()>(())?;
                if Instant::now() >= self.deadline.get() {
                    return Err(Exception::throw_message(&ctx, JOB_BUDGET_EXHAUSTED));
                }
                ctx.globals().get::<_, Function>(RETIRE_LAYERS)?.call::<_, ()>(())
            })();
            result.map_err(|e| js_error(&ctx, &e))
        });
        tracing::trace!(target: "paper_scene::script::frame", invoked, expected, elapsed_us = started.elapsed().as_micros(), "SceneScript updates");
        if let Err(error) = result {
            self.stop(format!("scripts stopped: {error:#}"));
            return true;
        }
        if self.deadline.exhausted() {
            self.stop(UPDATE_BUDGET_EXHAUSTED.to_owned());
            return true;
        }
        match self.drain() {
            Ok(changed) => {
                let properties_changed = std::mem::take(&mut self.properties_changed);
                changed || properties_changed
            }
            Err(error) => {
                self.stop(format!("changes stopped: {error:#}"));
                true
            }
        }
    }

    pub fn finish_compose(&mut self) {
        if !self.disabled && self.deadline.spent() {
            self.stop(UPDATE_BUDGET_EXHAUSTED.to_owned());
        }
        self.deadline.renew(self.limits.compose);
    }

    fn stop(&mut self, diagnostic: String) {
        if std::mem::replace(&mut self.disabled, true) {
            return;
        }
        tracing::warn!("SceneScript: {diagnostic}");
        self.diagnostics.push(diagnostic);
        let _entry = self.deadline.enter();
        self.deadline.cleanup();
        let halted = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(HALT_SCRIPTS)
                .and_then(|halt| halt.call::<_, ()>(()))
                .map_err(|error| js_error(&ctx, &error))
        });
        if let Err(error) = halted.and_then(|()| self.drain().map(|_| ())) {
            self.diagnostics.push(format!("layer retirement stopped: {error:#}"));
        }
    }

    fn disable_module(&self, index: usize) {
        let deadline = self.deadline.get();
        self.deadline.cleanup();
        self.context.with(|ctx| {
            let _ = ctx
                .globals()
                .get::<_, Function>("__disable")
                .and_then(|f| f.call::<_, ()>((index,)));
        });
        self.deadline.restore(deadline);
    }

    fn drain(&mut self) -> Result<bool> {
        let _entry = self.deadline.enter();
        let created: String = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>(DRAIN_CREATED)
                .and_then(|drain| drain.call(()))
                .map_err(|error| js_error(&ctx, &error))
        })?;
        let created: Vec<(usize, Value)> = serde_json::from_str(&created)?;
        for (slot, object) in &created {
            let Err(index) = self.created_slots.binary_search(slot) else {
                return Err(anyhow!("SceneScript created object slot is already occupied"));
            };
            let objects = self.scene[OBJECTS_FIELD]
                .as_array_mut()
                .ok_or_else(|| anyhow!("SceneScript objects are not an array"))?;
            if self.vacant_slots.remove(slot) {
                objects[*slot] = object.clone();
            } else if *slot == objects.len() && objects.len() < crate::model::MAX_SCENE_OBJECTS {
                objects.push(object.clone());
            } else {
                return Err(anyhow!("SceneScript created object slot is invalid"));
            }
            self.layer_order.push(*slot);
            self.created_slots.insert(index, *slot);
        }
        let json: String = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__drain")
                .and_then(|f| f.call(()))
                .map_err(|e| js_error(&ctx, &e))
        })?;
        if json.len() > MAX_DELTA_BYTES {
            return Err(anyhow!("SceneScript changes exceed 1 MiB per frame"));
        }
        self.audio_registered =
            self.context.with(|ctx| ctx.eval::<bool, _>("__audio.size > 0").unwrap_or(false));
        if let Some(storage) = &mut self.storage {
            let data: String = self
                .context
                .with(|ctx| ctx.globals().get::<_, Function>("__storageDrain")?.call(()))?;
            if !data.is_empty() {
                storage.save(&data)?;
            }
        }
        let changes: Vec<(String, Value)> = serde_json::from_str(&json)?;
        for (path, value) in &changes {
            let reset = effect_visibility_slot(path).filter(|_| {
                self.scene
                    .pointer(path)
                    .and_then(Value::as_bool)
                    .zip(value.as_bool())
                    .is_some_and(|(old, new)| old != new)
            });
            if let Some(target) = self.scene.pointer_mut(path) {
                *target = value.clone();
            } else if let Some((parent, key)) = path.rsplit_once('/')
                && let Some(Value::Object(map)) = self.scene.pointer_mut(parent)
            {
                map.insert(key.replace("~1", "/").replace("~0", "~"), value.clone());
            }
            if let Some(slot) = reset
                && !self.effect_resets.contains(&slot)
            {
                self.effect_resets.push(slot);
            }
            if let Some(command) = self.volume_command(path, value) {
                self.commands.push(command);
            }
            if let Some(key) = path.strip_prefix("/general/")
                && !key.contains('/')
            {
                self.general.push((key.to_owned(), value.clone()));
            }
        }
        let queued: String = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__drainCommands")
                .and_then(|f| f.call(()))
                .map_err(|e| js_error(&ctx, &e))
        })?;
        let queued: Vec<(String, String, String, Value)> = serde_json::from_str(&queued)?;
        self.commands.extend(queued.iter().filter_map(|(kind, target, op, value)| {
            parse_command(kind, target, op, value, &self.layer_order)
        }));
        self.drain_lifecycle()?;
        self.refresh_activity();
        let hidden_changed = self.hide_layers();
        Ok(!created.is_empty() || !changes.is_empty() || hidden_changed)
    }

    fn drain_lifecycle(&mut self) -> Result<()> {
        let lifecycle: String =
            self.context.with(|ctx| ctx.globals().get::<_, Function>(DRAIN_LIFECYCLE)?.call(()))?;
        if !lifecycle.is_empty() {
            let (hidden, retired, destroyed): (Vec<usize>, Vec<usize>, Vec<usize>) =
                serde_json::from_str(&lifecycle)?;
            for &slot in &hidden {
                self.puppets.borrow_mut().retire(slot);
            }
            self.hidden.extend(hidden);
            if let Some(images) = &self.images {
                for &slot in &destroyed {
                    images.release(slot);
                }
            }
            self.destroyed.extend(destroyed);
            self.updates.retain(|index| !retired.contains(index));
        }
        Ok(())
    }

    fn refresh_activity(&mut self) {
        self.work_pending =
            self.context.with(|ctx| ctx.eval::<bool, _>(PENDING_WORK).unwrap_or(false));
    }

    fn hide_layers(&mut self) -> bool {
        let mut changed = false;
        for &slot in &self.hidden {
            if let Some(layer) = self.scene[OBJECTS_FIELD][slot].as_object_mut()
                && layer.get(VISIBLE_FIELD) != Some(&Value::Bool(false))
            {
                layer.insert(VISIBLE_FIELD.into(), Value::Bool(false));
                changed = true;
            }
        }
        changed
    }

    fn volume_command(&self, path: &str, value: &Value) -> Option<ScriptCommand> {
        let (parent, key) = path.rsplit_once('/')?;
        if key != "volume" {
            return None;
        }
        let object = self.scene.pointer(parent)?;
        object.get("sound")?;
        let id = object.get("id").map(|id| id.to_string().trim_matches('"').to_owned())?;
        let gain = crate::effects::json_numbers(value)?.first().copied()?;
        Some(ScriptCommand::Sound { id, op: SoundOp::Gain(gain.max(0.0)) })
    }
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod budget_tests;

#[cfg(test)]
#[path = "failure_tests.rs"]
mod failure_tests;
