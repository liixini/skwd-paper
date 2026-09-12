class Vec2 {
    constructor(x = 0, y) { if (typeof x === 'object') { y = x.y; x = x.x; } this.x = x; this.y = y ?? x; }
    copy() { return new this.constructor(this); }
    add(v) { return this.map((x, k) => x + (typeof v === 'number' ? v : v[k])); }
    subtract(v) { return this.map((x, k) => x - (typeof v === 'number' ? v : v[k])); }
    multiply(v) { return this.map((x, k) => x * (typeof v === 'number' ? v : v[k])); }
    divide(v) { return this.map((x, k) => x / (typeof v === 'number' ? v : v[k])); }
    map(f) { const v = this.copy(); for (const k of ['x', 'y', 'z', 'w']) if (k in v) v[k] = f(this[k], k); return v; }
    length() { return Math.sqrt(this.dot(this)); }
    lengthSqr() { return this.dot(this); }
    dot(v) { return ['x', 'y', 'z', 'w'].reduce((s, k) => s + (this[k] ?? 0) * (v[k] ?? 0), 0); }
    normalize() { const n = this.length(); return n ? this.divide(n) : this.copy(); }
    equals(v) { return ['x', 'y', 'z', 'w'].every(k => this[k] === v[k]); }
    toString() { return ['x', 'y', 'z', 'w'].filter(k => k in this).map(k => this[k]).join(' '); }
}
class Vec3 extends Vec2 {
    constructor(x = 0, y, z) { super(x, y); this.z = typeof x === 'object' ? (x.z ?? z ?? 0) : (z ?? x); }
    cross(v) { return new Vec3(this.y * v.z - this.z * v.y, this.z * v.x - this.x * v.z, this.x * v.y - this.y * v.x); }
}
class Vec4 extends Vec3 {
    constructor(x = 0, y, z, w) { super(x, y, z); this.w = typeof x === 'object' ? (x.w ?? w ?? 0) : (w ?? x); }
}
Object.assign(globalThis, { Vec2, Vec3, Vec4 });
const __changes = new Map();
const __modules = [];
const __timers = new Map();
let __timerId = 0;
let __current = null;
const __vectors = new Set(['origin', 'scale', 'angles', 'color', 'size', 'parallaxDepth']);
function __coerce(value, key) {
    if (__vectors.has(key)) {
        const a = typeof value === 'string' ? value.trim().split(/\s+/).map(Number) : Array.isArray(value) ? value : null;
        if (a) return key === 'size' || key === 'parallaxDepth' ? new Vec2(...a) : new Vec3(...a);
        if (typeof value === 'number') return new Vec3(value);
    }
    return value;
}
function __plain(value, key) {
    if (value instanceof Vec2) {
        let parts = ['x','y','z','w'].filter(k => k in value).map(k => value[k]);
        if (key === 'angles') parts = parts.map(n => n * Math.PI / 180);
        return parts.join(' ');
    }
    return value;
}
function __wrap(value, path, key) {
    value = __coerce(value, key);
    if (value instanceof Vec2) {
        if (key === 'angles') value = value.multiply(180 / Math.PI);
        return new Proxy(value, { set(target, k, v) { if (target[k] !== v) { target[k] = v; __changes.set(path, __plain(target, key)); } return true; } });
    }
    if (!value || typeof value !== 'object') return value;
    for (const k of Object.keys(value)) value[k] = __wrap(value[k], path + '/' + k.replace(/~/g,'~0').replace(/\//g,'~1'), k);
    return new Proxy(value, { set(target, k, v) {
        const p = path + '/' + String(k).replace(/~/g,'~0').replace(/\//g,'~1');
        v = __coerce(v, k);
        if (!(target[k] instanceof Vec2 && v instanceof Vec2 && target[k].equals(v)) && target[k] !== v) {
            __changes.set(p, __plain(v, k));
            target[k] = v instanceof Vec2 ? __wrap(__plain(v,k),p,k) : __wrap(v,p,k);
        }
        return true;
    }});
}
function __setup(json, props) {
    globalThis.__scene = __wrap(JSON.parse(json), '', '');
    globalThis.__layers = __scene.objects || [];
    engine.userProperties = {};
    for (const [name,entry] of Object.entries(JSON.parse(props))) if (entry.value !== undefined) engine.userProperties[name] = entry.type === 'color' ? __coerce(entry.value, 'color') : entry.value;
    engine.canvasSize = new Vec2(__scene.general?.orthogonalprojection?.width ?? 1920, __scene.general?.orthogonalprojection?.height ?? 1080);
    for (const layer of __layers) {
        for (const [key,value] of Object.entries({visible:true, origin:'0 0 0', scale:'1 1 1', angles:'0 0 0', color:'1 1 1', alpha:1}))
            if (layer[key] === undefined) layer[key] = value;
        Object.defineProperty(layer, 'getEffect', {value: key => (layer.effects || []).find(e => e.name === key || e.id === key)});
        Object.defineProperty(layer, 'getAnimation', {value: () => { throw Error('SceneScript timeline control is not implemented'); }});
    }
    __changes.clear();
}
function __owner(path) {
    const parts = path.split('/').slice(1).map(s => s.replace(/~1/g,'/').replace(/~0/g,'~'));
    const key = parts.pop(); let object = __scene;
    for (const p of parts) object = object[p];
    return [object, key];
}
function __register(index, ns, path) { const [object,key] = __owner(path); __modules[index] = {ns, object, key, layer:path.startsWith('/objects/') ? __layers[Number(path.split('/')[2])] : null, disabled:false}; }
function __disable(index) { if (__modules[index]) __modules[index].disabled = true; }
function __invoke(index, name) {
    const m = __modules[index];
    if (!m || m.disabled || typeof m.ns[name] !== 'function') return;
    __current = m;
    const value = m.object[m.key];
    const result = m.ns[name](value instanceof Vec2 ? value.copy() : value);
    if (result !== undefined) m.object[m.key] = result;
    __current = null;
}
function __drain() { const result = JSON.stringify([...__changes]); __changes.clear(); return result; }
function __frame(time, dt, x, y) {
    engine.runtime = time; engine.frametime = dt;
    input.cursorWorldPosition = new Vec3(x * engine.canvasSize.x, (1 - y) * engine.canvasSize.y, 0);
    input.cursorScreenPosition = new Vec2(x * engine.canvasSize.x, y * engine.canvasSize.y);
}
function __createScriptProperties(defaults = {}) {
    const values = {};
    const builder = { finish: () => values };
    for (const kind of ['Slider','Checkbox','Combo','Color','Text','Texture']) builder['add'+kind] = o => { values[o.name] = defaults[o.name] ?? o.value ?? o.options?.[0]?.value; if (kind === 'Color') values[o.name] = __coerce(values[o.name], 'color'); return builder; };
    return builder;
}
const engine = {
    runtime:0, frametime:0, canvasSize:new Vec2(1920,1080), screenResolution:new Vec2(1920,1080), userProperties:{}, isRunningInEditor:()=>false,
    AUDIO_RESOLUTION_16:16,AUDIO_RESOLUTION_32:32,AUDIO_RESOLUTION_64:64,
    isDesktopDevice:()=>true,isMobileDevice:()=>false,isWallpaper:()=>true,isScreensaver:()=>false,
    get timeOfDay(){const d=new Date();return (d.getHours()*3600+d.getMinutes()*60+d.getSeconds())/86400;},
    registerAudioBuffers: size => { if (!Number.isInteger(size) || size < 1 || size > 128) throw Error('Invalid audio buffer size'); const b = {left:new Array(size).fill(0),right:new Array(size).fill(0),average:new Array(size).fill(0)}; __audio.push(b); return b; },
    setTimeout: (fn, delay=0) => __timer(fn, delay, false), setInterval: (fn, delay=0) => __timer(fn, delay, true),
    clearTimeout: id => __timers.delete(id), clearInterval: id => __timers.delete(id)
};
const __audio = [];
function __timer(fn, delay, repeat) {
    if (typeof fn !== 'function' || !Number.isFinite(delay) || __timers.size >= 256) throw Error('Invalid or excessive SceneScript timers');
    const id = ++__timerId; __timers.set(id, {fn, delay:Math.max(1,delay)/1000, due:engine.runtime+Math.max(1,delay)/1000, repeat}); return () => __timers.delete(id);
}
function __tickTimers() {
    for (const [id,timer] of [...__timers]) if (timer.due <= engine.runtime) {
        if (timer.repeat) timer.due = engine.runtime + timer.delay; else __timers.delete(id);
        timer.fn();
    }
}
const input = {cursorWorldPosition:new Vec3(),cursorScreenPosition:new Vec2()};
const thisScene = {
    getLayer: key => __layers.find(l => l.name === key || String(l.id) === String(key)),
    enumerateLayers: () => [...__layers],
    getLayerIndex: layer => __layers.indexOf(layer),
    getLayerByIndex: index => __layers[index],
    createLayer: () => { throw Error('SceneScript dynamic layer creation is not implemented'); }
};
const shared = {};
const console = {log: (...args) => __log(args.map(String).join(' ')), warn: (...args) => __log(args.map(String).join(' ')), error: (...args) => __log(args.map(String).join(' '))};
Object.assign(globalThis, {engine,input,thisScene,shared,console,createScriptProperties:__createScriptProperties});

Object.assign(globalThis, {MediaPlaybackEvent:{PLAYBACK_PLAYING:1,PLAYBACK_PAUSED:2,PLAYBACK_STOPPED:0}});
function __audioUpdate(count,left,right) { for(const b of __audio) if(b.left.length===count) for(let i=0;i<count;i++){b.left[i]=left[i];b.right[i]=right[i];b.average[i]=(left[i]+right[i])*0.5;} }

let __lastPointer = [-1,-1];
let __lastButtons = [false,false,false];
let __hover = new Set();
const __pressed = new Map();
function __pointer(x,y,buttons,hits) {
    const moved=x!==__lastPointer[0] || y!==__lastPointer[1];
    if(!moved && buttons.every((v,i)=>v===__lastButtons[i])) return;
    input.cursorWorldPosition=new Vec3(x*engine.canvasSize.x,(1-y)*engine.canvasSize.y,0);
    input.cursorScreenPosition=new Vec2(x*engine.canvasSize.x,y*engine.canvasSize.y);
    const hovered=new Set(hits);
    for(let i=0;i<__modules.length;i++) {
        const m=__modules[i]; if(!m || m.disabled || !m.layer) continue;
        const id=String(m.layer.id), inside=hovered.has(id), was=__hover.has(id);
        const event={cursorWorldPosition:input.cursorWorldPosition.copy(),cursorScreenPosition:input.cursorScreenPosition.copy()};
        const call=name=>{if(typeof m.ns[name]==='function') m.ns[name](event);};
        try {
            if(inside&&!was) call('cursorEnter');
            if(!inside&&was) call('cursorLeave');
            if(inside&&moved) call('cursorMove');
            if(inside&&buttons[0]&&!__lastButtons[0]) {__pressed.set(i,true);call('cursorDown');}
            if(!buttons[0]&&__lastButtons[0]) {if(inside){call('cursorUp');if(__pressed.has(i)) call('cursorClick');}__pressed.delete(i);}
        } catch(e) {m.disabled=true;__log('cursor script '+i+': '+String(e));}
    }
    __lastPointer=[x,y];__lastButtons=buttons;__hover=hovered;
}
