use super::source;
use anyhow::{Result, anyhow};
use rquickjs::{Context, Ctx, Function, Module, Runtime};
use serde_json::Value;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const HEAP_LIMIT: usize = 32 * 1024 * 1024;
const FRAME_BUDGET: Duration = Duration::from_millis(4);

pub struct SceneScripts {
    context: Context,
    runtime: Runtime,
    deadline: Rc<Cell<Instant>>,
    pub scene: Value,
    pub properties: crate::model::Properties,
    updates: Vec<usize>,
    pub diagnostics: Vec<String>,
    pub bindings: usize,
    disabled: bool,
    audio_registered: bool,
    timers_pending: bool,
}

fn js_error(ctx: &Ctx<'_>, error: &rquickjs::Error) -> anyhow::Error {
    let detail = ctx.catch();
    anyhow!("{error}: {detail:?}")
}

impl SceneScripts {
    pub fn load(
        scene: &mut Value,
        props: &crate::model::Properties,
        project: &Value,
    ) -> Result<Option<Self>> {
        if !source::present(scene) || !source::needs_runtime(scene, "") {
            return Ok(None);
        }
        let mut bindings = Vec::new();
        source::collect(scene, "", &mut bindings, props);
        if bindings.len() > 4096
            || bindings.iter().map(|b| b.source.len()).sum::<usize>() > 4 * 1024 * 1024
        {
            return Err(anyhow!("SceneScript source exceeds the scene budget"));
        }
        let runtime = Runtime::new().map_err(|e| anyhow!("SceneScript runtime: {e}"))?;
        runtime.set_memory_limit(HEAP_LIMIT);
        runtime.set_max_stack_size(512 * 1024);
        let deadline = Rc::new(Cell::new(Instant::now() + Duration::from_secs(2)));
        let interrupt = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= interrupt.get())));
        let context = Context::full(&runtime).map_err(|e| anyhow!("SceneScript context: {e}"))?;
        let mut host = Self {
            context,
            runtime,
            deadline,
            scene: Value::Null,
            properties: props.clone(),
            updates: Vec::new(),
            diagnostics: Vec::new(),
            bindings: bindings.len(),
            disabled: false,
            audio_registered: false,
            timers_pending: false,
        };
        host.setup(scene, props, project)?;
        let initialization_deadline = Instant::now() + Duration::from_secs(2);
        for (index, binding) in bindings.iter().enumerate() {
            if Instant::now() >= initialization_deadline {
                host.diagnostics
                    .push("remaining scripts skipped: initialization budget exhausted".into());
                break;
            }
            host.deadline
                .set(initialization_deadline.min(Instant::now() + Duration::from_millis(100)));
            if let Err(error) = host.compile(index, binding) {
                host.diagnostics.push(format!("{}: {error:#}", binding.path));
                host.disable_module(index);
            }
        }
        host.scene = scene.clone();
        host.drain()?;
        host.tick(0.0, 0.0, [0.5; 2])?;
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

    fn setup(
        &self,
        scene: &Value,
        props: &crate::model::Properties,
        project: &Value,
    ) -> Result<()> {
        let mut resolved = scene.clone();
        source::resolve_wrappers(&mut resolved, props);
        self.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<()> {
                ctx.globals().set("__log", Function::new(ctx.clone(), |message: String| { tracing::debug!("SceneScript: {}", message.chars().take(1024).collect::<String>()); })?)?;
                ctx.eval::<(), _>(include_str!("bootstrap.js"))?;
                let setup: Function = ctx.globals().get("__setup")?;
                let mut values = project.pointer("/general/properties").and_then(Value::as_object).cloned().unwrap_or_default();
                for (key, numbers) in props {
                    let name = values.keys().find(|name| name.eq_ignore_ascii_case(key)).cloned().unwrap_or_else(|| key.clone());
                    let entry = values.entry(name).or_insert_with(|| serde_json::json!({}));
                    let value = if entry["type"] == "bool" { Value::Bool(numbers.first().is_some_and(|n| *n != 0.0)) } else if numbers.len() == 1 { Value::from(numbers[0]) } else { serde_json::json!(numbers) };
                    entry["value"] = value;
                }
                setup.call::<_, ()>((resolved.to_string(), Value::Object(values).to_string()))?;
                for (name, source) in [
                    ("WEMath", "export const mix=(a,b,t)=>a+(b-a)*t; export const clamp=(x,a,b)=>Math.min(b,Math.max(a,x)); export const smoothstep=(a,b,x)=>{const t=clamp((x-a)/(b-a),0,1);return t*t*(3-2*t)}; export const random=(a=0,b=1)=>mix(a,b,Math.random()); export const radians=x=>x*Math.PI/180; export const degrees=x=>x*180/Math.PI; export const deg2rad=radians; export const rad2deg=degrees; export const smoothStep=smoothstep;"),
                    ("WEVector", "export const lerp=(a,b,t)=>a.add(b.subtract(a).multiply(t));"),
                ] { let (_, p) = Module::declare(ctx.clone(), name, source)?.eval()?; p.finish::<()>()?; }
                Ok(())
            })();
            result.map_err(|e| js_error(&ctx,&e))
        })
    }

    fn compile(&mut self, index: usize, binding: &source::Binding) -> Result<()> {
        let has_update = self.context.with(|ctx| {
            let result = (|| -> rquickjs::Result<bool> {
                let layer = binding.layer.map_or("undefined".to_owned(), |i| format!("__layers[{i}]"));
                let source = format!("const thisLayer = {layer}; const thisObject = __owner({})[0]; const createScriptProperties = () => __createScriptProperties({});\n{}", serde_json::to_string(&binding.path).unwrap(), binding.properties, binding.source);
                let (module, promise) = Module::declare(ctx.clone(), format!("scene-{index}.js"), source)?.eval()?;
                promise.finish::<()>()?;
                let namespace = module.namespace()?;
                let has_update = namespace.get::<_, rquickjs::Value>("update")?.is_function();
                let register: Function = ctx.globals().get("__register")?;
                register.call::<_, ()>((index, namespace, binding.path.as_str()))?;
                let invoke: Function = ctx.globals().get("__invoke")?;
                invoke.call::<_, ()>((index, "init"))?;
                Ok(has_update)
            })();
            result.map_err(|e| js_error(&ctx,&e))
        })?;
        if has_update {
            self.updates.push(index);
        }
        Ok(())
    }

    pub fn needs_audio(&self) -> bool {
        !self.disabled && self.audio_registered
    }

    pub fn audio(&mut self, count: usize, left: &[f32], right: &[f32]) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        self.deadline.set(Instant::now() + FRAME_BUDGET);
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

    pub fn pointer(
        &mut self,
        position: [f32; 2],
        buttons: [bool; 3],
        hits: Vec<String>,
    ) -> Result<()> {
        if self.disabled {
            return Ok(());
        }
        self.deadline.set(Instant::now() + FRAME_BUDGET);
        let result = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__pointer")
                .and_then(|f| f.call::<_, ()>((position[0], position[1], buttons.to_vec(), hits)))
                .map_err(|e| js_error(&ctx, &e))
        });
        if let Err(error) = result {
            self.stop(format!("cursor event stopped: {error:#}"));
        }
        Ok(())
    }

    pub fn animated(&self) -> bool {
        !self.disabled && (!self.updates.is_empty() || self.timers_pending)
    }

    pub fn heap_bytes(&self) -> usize {
        self.runtime.memory_usage().memory_used_size.max(0) as usize
    }

    pub fn tick(&mut self, time: f32, dt: f32, pointer: [f32; 2]) -> Result<bool> {
        if self.disabled {
            return Ok(false);
        }
        self.deadline.set(Instant::now() + FRAME_BUDGET);
        let mut failed = Vec::new();
        let result = self.context.clone().with(|ctx| {
            let result = (|| -> rquickjs::Result<()> {
                let frame: Function = ctx.globals().get("__frame")?;
                frame.call::<_, ()>((time, dt, pointer[0], pointer[1]))?;
                let invoke: Function = ctx.globals().get("__invoke")?;
                for &index in &self.updates {
                    if Instant::now() >= self.deadline.get() {
                        break;
                    }
                    if let Err(error) = invoke.call::<_, ()>((index, "update")) {
                        let error = js_error(&ctx, &error);
                        self.diagnostics.push(format!("script {index} stopped: {error:#}"));
                        tracing::warn!("SceneScript {index} stopped: {error:#}");
                        failed.push(index);
                    }
                }
                self.updates.retain(|index| !failed.contains(index));
                if Instant::now() >= self.deadline.get() {
                    return Ok(());
                }
                let timers: Function = ctx.globals().get("__tickTimers")?;
                timers.call::<_, ()>(())
            })();
            result.map_err(|e| js_error(&ctx, &e))
        });
        for index in failed {
            self.disable_module(index);
        }
        if let Err(error) = result {
            self.stop(format!("scripts stopped: {error:#}"));
            return Ok(false);
        }
        match self.drain() {
            Ok(changed) => Ok(changed),
            Err(error) => {
                self.stop(format!("changes stopped: {error:#}"));
                Ok(false)
            }
        }
    }

    fn stop(&mut self, diagnostic: String) {
        self.disabled = true;
        tracing::warn!("SceneScript: {diagnostic}");
        self.diagnostics.push(diagnostic);
    }

    fn disable_module(&self, index: usize) {
        let deadline = self.deadline.get();
        self.deadline.set(Instant::now() + FRAME_BUDGET);
        self.context.with(|ctx| {
            let _ = ctx
                .globals()
                .get::<_, Function>("__disable")
                .and_then(|f| f.call::<_, ()>((index,)));
        });
        self.deadline.set(deadline);
    }

    fn drain(&mut self) -> Result<bool> {
        self.deadline.set(Instant::now() + FRAME_BUDGET);
        let json: String = self.context.with(|ctx| {
            ctx.globals()
                .get::<_, Function>("__drain")
                .and_then(|f| f.call(()))
                .map_err(|e| js_error(&ctx, &e))
        })?;
        if json.len() > 1024 * 1024 {
            return Err(anyhow!("SceneScript changes exceed 1 MiB per frame"));
        }
        self.audio_registered =
            self.context.with(|ctx| ctx.eval::<bool, _>("__audio.length > 0").unwrap_or(false));
        self.timers_pending =
            self.context.with(|ctx| ctx.eval::<bool, _>("__timers.size > 0").unwrap_or(false));
        let changes: Vec<(String, Value)> = serde_json::from_str(&json)?;
        for (path, value) in &changes {
            if let Some(target) = self.scene.pointer_mut(path) {
                *target = value.clone();
            } else if let Some((parent, key)) = path.rsplit_once('/')
                && let Some(Value::Object(map)) = self.scene.pointer_mut(parent)
            {
                map.insert(key.replace("~1", "/").replace("~0", "~"), value.clone());
            }
        }
        Ok(!changes.is_empty())
    }
}
