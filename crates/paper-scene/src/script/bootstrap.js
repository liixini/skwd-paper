const __equalityTolerance = 0.00001;
const __vectorComponents = ['x', 'y', 'z', 'w'];
const __vectorDimensions = {two: 2, three: 3, four: 4};
const __hermite = {quadratic: 3, cubic: -2};
const __degreesPerHalfTurn = 180;
const __cosineBounds = {min: -1, max: 1};
const __angleUnits = {radiansPerDegree: Math.PI / __degreesPerHalfTurn, degreesPerRadian: __degreesPerHalfTurn / Math.PI};
function __operand(vector, v, k, identity) {
    if (typeof v === 'number') return v;
    return vector instanceof Vec3 && v instanceof Vec2 && !(k in v) ? identity : v[k];
}
class __Vector {
    constructor(dimension, x = 0, y, z, w) {
        const source = x && typeof x === 'object' ? x : null;
        const parsed = typeof x === 'string' ? x.trim().split(/\s+/).map(Number) : null;
        const scalar = y === undefined && z === undefined && w === undefined;
        this.x = parsed ? parsed[0] || 0 : source ? source.x ?? 0 : x;
        this.y = parsed ? parsed[1] || 0 : source ? source.y ?? 0 : y ?? (scalar ? x : 0);
        if (dimension >= __vectorDimensions.three) this.z = parsed ? parsed[2] || 0 : source ? source.z ?? 0 : z ?? (scalar ? x : 0);
        if (dimension >= __vectorDimensions.four) this.w = parsed ? parsed[3] || 0 : source ? source.w ?? 0 : w ?? (scalar ? x : 0);
    }
    copy() { return new this.constructor(this); }
    add(v) { return this.map((x, k) => x + __operand(this, v, k, 0)); }
    subtract(v) { return this.map((x, k) => x - __operand(this, v, k, 0)); }
    multiply(v) { return this.map((x, k) => x * __operand(this, v, k, 1)); }
    divide(v) { return this.map((x, k) => x / __operand(this, v, k, 1)); }
    map(f) { const v = this.copy(); for (const k of __vectorComponents) if (k in v) v[k] = f(this[k], k); return v; }
    length() { return Math.sqrt(this.dot(this)); }
    lengthSqr() { return this.dot(this); }
    distance(v) { return Math.sqrt(this.distanceSqr(v)); }
    distanceSqr(v) {
        let squared = 0;
        for (const k of __vectorComponents) if (k in this) {
            const delta = this[k] - v[k];
            squared += delta * delta;
        }
        return squared;
    }
    isFinite() { return __vectorComponents.every(k => !(k in this) || Number.isFinite(this[k])); }
    negate() { return this.map(x => -x); }
    abs() { return this.map(Math.abs); }
    sign() { return this.map(Math.sign); }
    floor() { return this.map(Math.floor); }
    ceil() { return this.map(Math.ceil); }
    round() { return this.map(Math.round); }
    fract() { return this.map(x => x - Math.floor(x)); }
    min(v) { return this.map((x, k) => Math.min(x, v[k])); }
    max(v) { return this.map((x, k) => Math.max(x, v[k])); }
    clamp(lo, hi) { return this.map((x, k) => Math.max(typeof lo === 'number' ? lo : lo[k], Math.min(typeof hi === 'number' ? hi : hi[k], x))); }
    mix(v, amount) { return this.map((x, k) => x + (v[k] - x) * (typeof amount === 'number' ? amount : amount[k])); }
    mod(v) { return this.map((x, k) => { const divisor = typeof v === 'number' ? v : v[k]; return x - divisor * Math.floor(x / divisor); }); }
    step(edge) { return this.map((x, k) => x < (typeof edge === 'number' ? edge : edge[k]) ? 0 : 1); }
    smoothStep(lo, hi) { return this.map((x, k) => {
        const lower = typeof lo === 'number' ? lo : lo[k];
        const upper = typeof hi === 'number' ? hi : hi[k];
        const amount = Math.max(0, Math.min(1, (x - lower) / (upper - lower)));
        return amount * amount * (__hermite.quadratic + __hermite.cubic * amount);
    }); }
    reflect(normal) { const scale = 2 * this.dot(normal); return this.map((x, k) => x - scale * normal[k]); }
    project(axis) { const squared = axis.lengthSqr(); const scale = squared ? this.dot(axis) / squared : 0; return this.map((_component, k) => axis[k] * scale); }
    dot(v) { return __vectorComponents.reduce((s, k) => s + (this[k] ?? 0) * (v[k] ?? 0), 0); }
    normalize() { const n = this.length(); return n ? this.divide(n) : this.copy(); }
    equals(v) {
        const dimension = this instanceof Vec4 ? __vectorDimensions.four : this instanceof Vec3 ? __vectorDimensions.three : __vectorDimensions.two;
        for (let slot = 0; slot < dimension; slot++) {
            const component = __vectorComponents[slot];
            if (!(Math.abs(this[component] - v[component]) < __equalityTolerance)) return false;
        }
        return true;
    }
    toString() { return __vectorComponents.filter(k => k in this).map(k => this[k]).join(' '); }
}
class Vec2 extends __Vector {
    constructor(x = 0, y) { super(__vectorDimensions.two, x, y); }
    static fromString(value) { return new Vec2(String(value)); }
    perpendicular() { return new Vec2(this.y, -this.x); }
    angle() { return Math.atan2(this.y, this.x) * __angleUnits.degreesPerRadian; }
    angleBetween(v) { return Math.atan2(this.x * v.y - this.y * v.x, this.dot(v)) * __angleUnits.degreesPerRadian; }
    rotate(degrees) {
        const radians = degrees * __angleUnits.radiansPerDegree;
        const cosine = Math.cos(radians), sine = Math.sin(radians);
        return new Vec2(this.x * cosine - this.y * sine, this.x * sine + this.y * cosine);
    }
}
class Vec3 extends __Vector {
    constructor(x = 0, y, z) { super(__vectorDimensions.three, x, y, z); }
    static fromString(value) { return new Vec3(String(value)); }
    static fromSpherical(r, theta, phi) {
        const polar = theta * __angleUnits.radiansPerDegree, azimuth = phi * __angleUnits.radiansPerDegree;
        const radius = r * Math.sin(polar);
        return new Vec3(radius * Math.cos(azimuth), r * Math.cos(polar), radius * Math.sin(azimuth));
    }
    cross(v) { return new Vec3(this.y * v.z - this.z * v.y, this.z * v.x - this.x * v.z, this.x * v.y - this.y * v.x); }
    refract(normal, eta) {
        const cosine = this.dot(normal);
        const discriminant = 1 - eta * eta * (1 - cosine * cosine);
        if (discriminant < 0) return this.map(() => 0);
        const scale = eta * cosine + Math.sqrt(discriminant);
        return this.map((x, k) => eta * x - scale * normal[k]);
    }
    angleBetween(v) {
        const lengths = Math.sqrt(this.lengthSqr() * v.lengthSqr());
        return lengths ? Math.acos(Math.max(__cosineBounds.min, Math.min(__cosineBounds.max, this.dot(v) / lengths))) * __angleUnits.degreesPerRadian : 0;
    }
    toSpherical() {
        const radius = this.length();
        if (!radius) return new Vec3(0);
        const polar = Math.acos(Math.max(__cosineBounds.min, Math.min(__cosineBounds.max, this.y / radius))) * __angleUnits.degreesPerRadian;
        return new Vec3(radius, polar, Math.atan2(this.z, this.x) * __angleUnits.degreesPerRadian);
    }
}
class Vec4 extends __Vector {
    constructor(x = 0, y, z, w) { super(__vectorDimensions.four, x, y, z, w); }
    static fromString(value) { return new Vec4(String(value)); }
}
const __nativeVectorOrder = {
    Vec3: ['length', 'lengthSqr', 'distance', 'distanceSqr', 'normalize', 'copy', 'equals', 'isFinite', 'negate', 'add', 'subtract', 'multiply', 'divide', 'cross', 'dot', 'reflect', 'refract', 'project', 'angleBetween', 'toSpherical', 'mix', 'min', 'max', 'clamp', 'abs', 'sign', 'round', 'floor', 'ceil'],
    Vec4: ['length', 'lengthSqr', 'distance', 'distanceSqr', 'normalize', 'copy', 'equals', 'isFinite', 'negate', 'add', 'subtract', 'multiply', 'divide', 'dot', 'reflect', 'project', 'mix', 'min', 'max', 'clamp', 'abs', 'sign', 'round', 'floor', 'ceil', 'fract', 'mod', 'step', 'smoothStep']
};
function __installVectorPrototype(Type, order) {
    const own = new Map(Object.getOwnPropertyNames(Type.prototype).map(name => [name, Object.getOwnPropertyDescriptor(Type.prototype, name)]));
    for (const name of own.keys()) if (name !== 'constructor') delete Type.prototype[name];
    const inherited = Object.getOwnPropertyNames(__Vector.prototype).filter(name => name !== 'constructor' && !order.includes(name));
    for (const name of [...order, ...inherited]) {
        const descriptor = own.get(name) ?? Object.getOwnPropertyDescriptor(__Vector.prototype, name);
        if (descriptor) Object.defineProperty(Type.prototype, name, descriptor);
    }
}
__installVectorPrototype(Vec3, __nativeVectorOrder.Vec3);
__installVectorPrototype(Vec4, __nativeVectorOrder.Vec4);
for (const [Type, aliases] of [[Vec3, ['r','g','b']], [Vec4, ['r','g','b','a']]]) {
    aliases.forEach((alias, index) => Object.defineProperty(Type.prototype, alias, {
        enumerable:true,
        get() { return this[__vectorComponents[index]]; },
        set(value) { this[__vectorComponents[index]] = value; }
    }));
}
Object.assign(globalThis, { Vec2, Vec3, Vec4 });
const __changes = new Map();
function __queueSerializedNative(path, json, visibility = false) {
    if (!visibility) __changes.delete(path);
    __changes.set(visibility ? Symbol(path) : path, json);
}
function __queueNative(path, value) {
    const json = JSON.stringify(value) ?? 'null';
    __queueSerializedNative(path, json);
}
let __nativeWrite = false;
const __modules = [];
const __timers = new Map();
const __timerLimits = {maximum: 256, minimumDelay: 1, millisecondsPerSecond: 1000};
let __timerId = 0;
let __current = null;
const __alive = 'alive', __pending = 'pending', __retired = 'retired';
const __layerStates = new WeakMap();
const __nestedOwners = new WeakMap();
const __ownerState = object => __layerStates.get(object) ?? __nestedOwners.get(object);
function __ownsSlot(owner) { return !owner || __layerStates.get(__layers[owner.slot]) === owner; }
const __particleStates = new Map();
const __soundStates = new Map();
const __particleFactors = {alpha:1, size:10, count:10, rate:10, speed:100, lifetime:100};
const __particleControlPointCount = 8;
function __particleVector(value) {
    const vector = new Vec3(__coerce(value, 'color'));
    return vector.isFinite() ? vector : null;
}
function __particleInstanceVector(instance, id, key, initial, positive) {
    let current = __particleVector(initial) ?? new Vec3(positive ? 1 : 0);
    Object.defineProperty(instance, key, {enumerable:true, get: () => current, set: value => {
        const next = __particleVector(value);
        if (!next || (positive && [next.x, next.y, next.z].some(component => component < 0))) return;
        current = next;
        __command('particle', id, key, [next.x, next.y, next.z]);
    }});
}
const __missingAttachmentIndex = -1;
const __effectHidden = new WeakMap();
const __pendingLayers = new Set();
let __retiringLayers = null;
const __hiddenSlots = [];
const __retiredModules = [];
const __created = [];
const __destroyed = [];
const __dynamicSlots = new Set();
const __freeSlots = new Set();
let __nextLayerId;
const __objectPrefix = '/objects/';
const __instanceOverridePath = /^\/objects\/\d+\/instanceoverride\/[^/]+$/;
const __objectPath = /^\/objects\/\d+$/;
const __scaleProperty = 'scale';
const __layerProperties = new Set(['visible', 'origin', __scaleProperty, 'angles', 'color', 'alpha', 'name', 'parent', 'parallaxDepth',
    'getParent', 'getChildren', 'getRoot', 'isAncestorOf', 'getDepth', 'setParent', 'removeFromParent', 'getTransformMatrix',
    'getAttachmentIndex', 'getAttachmentMatrix', 'getAttachmentOrigin', 'getAttachmentAngles', 'rotateObjectSpace',
    'getEffect', 'getAnimation', 'getTextureAnimation', 'getEffectCount',
    'play', 'stop', 'pause', 'isPlaying']);
const __kindProperties = new Map([['volume', ['sound']], ['size', ['image', 'text']]]);
const __transformProperties = ['solid', 'name', 'parallaxDepth', 'sortorder', __scaleProperty, 'angles', 'origin', 'disablepropagation'];
const __visualProperties = ['colorBlendMode', 'ledsource', 'nointerpolation', 'castshadow', 'perspective', 'visible', 'brightness', 'alpha', 'color', 'copybackground', 'size', 'clampuvs'];
const __layerMethods = ['getChildren', 'getParent', 'getRoot', 'isAncestorOf', 'getDepth', 'setParent', 'removeFromParent', 'getTransformMatrix',
    'getAttachmentIndex', 'getAttachmentMatrix', 'getAttachmentOrigin', 'getAttachmentAngles', 'rotateObjectSpace', 'getAnimation'];
const __visualMethods = ['getEffect', 'getEffectCount'];
const __playbackMethods = ['play', 'isPlaying', 'pause', 'stop'];
const __orderNumbers = {sortorder:{convert:Math.trunc, initial:0}};
const __visualNumbers = {...__orderNumbers, colorBlendMode:{convert:Math.trunc, initial:0}, brightness:{convert:Math.fround}, alpha:{convert:Math.fround}};
const __textNumbers = {...__visualNumbers,
    blursize:{convert:Math.fround, initial:6}, outlinethickness:{convert:Math.fround, initial:4}, dropshadowopacity:{convert:Math.fround, initial:1},
    maxrows:{convert:Math.trunc}, maxwidth:{convert:Math.fround}, dropshadowsize:{convert:Math.fround, initial:6},
    pointsize:{convert:Math.fround}, backgroundbrightness:{convert:Math.fround}};
const __soundNumbers = {...__orderNumbers, mindistance:{convert:Math.fround, initial:1}, maxtime:{convert:Math.fround, initial:5},
    mintime:{convert:Math.fround, initial:1}, attenuation:{convert:Math.fround, initial:1}, volume:{convert:Math.fround}};
const __classPolicies = new Map([
    ['image', {numbers:__visualNumbers, fields:new Set([...__transformProperties, ...__visualProperties, 'alignment']), methods:new Set([...__layerMethods, ...__visualMethods, 'getTextureAnimation'])}],
    ['text', {numbers:__textNumbers, fields:new Set([...__transformProperties, ...__visualProperties, 'anchor', 'verticalalign', 'horizontalalign', 'depthtest', 'dropshadowoffset', 'dropshadowcolor', 'blursize', 'outlinethickness', 'dropshadowopacity', 'dropshadow', 'blur', 'outline', 'msdf', 'maxrows', 'maxwidth', 'spacing', 'text', 'blockalign', 'limituseellipsis', 'limitrows', 'dropshadowsize', 'limitwidth', 'font', 'backgroundcolor', 'outlinecolor', 'pointsize', 'opaquebackground', 'padding', 'backgroundbrightness']), methods:new Set([...__layerMethods, ...__visualMethods])}],
    ['sound', {numbers:__soundNumbers, fields:new Set([...__transformProperties, 'spatialization', 'mindistance', 'maxtime', 'mintime', 'muteineditor', 'startsilent', 'attenuation', 'playbackmode', 'volume']), methods:new Set([...__layerMethods, ...__playbackMethods])}],
    ['particle', {numbers:__orderNumbers, fields:new Set([...__transformProperties, 'instance', 'visible']), methods:new Set([...__layerMethods, ...__playbackMethods, 'emitParticles'])}]
]);
const __privateLayerFields = ['parent', 'attachment', 'image', 'sound', 'particle', 'effects', 'instance', 'instanceoverride'];
function __classPolicy(initial) {
    const kinds = [...__classPolicies.keys()].filter(kind => Object.hasOwn(initial, kind));
    return kinds.length === 1 ? __classPolicies.get(kinds[0]) : null;
}
const __soundHidden = ['visible', 'alpha', 'color'];
const __particleHidden = ['alpha', 'color'];
const __imageHidden = ['effects', 'image', 'instance'];
const __effectPath = /^\/objects\/\d+\/effects\/\d+$/;
const __effectProperties = new Set(['visible', 'name']);
function __nativeProperty(state, key) {
    return typeof key === 'string' && key !== 'id' && !state.hidden?.has(key) && (state.policy ? state.policy.fields.has(key)
        : Object.hasOwn(state.initial, key) || __layerProperties.has(key) || __kindProperties.get(key)?.some(kind => Object.hasOwn(state.initial, kind)) === true);
}
function __customNested(target, key, custom, deleted, effect, nativeLength) {
    if (custom?.has(key)) return true;
    if (Array.isArray(target)) {
        if (key === 'length') return false;
        if (typeof key !== 'string') return true;
        const index = Number(key);
        return !Number.isInteger(index) || index < 0 || index >= nativeLength || String(index) !== key;
    }
    return !Object.hasOwn(target, key) && !deleted?.has(key) && !(effect && __effectProperties.has(key));
}
function __call(module, callback, ...args) {
    const previous = __current;
    __current = module;
    try { return callback(...args); }
    finally {
        try { __runJobs(); } finally { __current = previous; }
    }
}
function __errorText(error) {
    try { return String(error); } catch { return 'unprintable exception'; }
}
function __callback(module, name, ...args) {
    if (module.failed?.has(name) || typeof module.ns[name] !== 'function') return;
    try { __call(module, module.ns[name], ...args); }
    catch (error) {
        (module.failed ??= new Set()).add(name);
        __log(name + ' ' + module.index + ': ' + __errorText(error));
    }
}
const __nativeFields = new Set(['color', 'origin', 'alpha', 'brightness', 'volume', 'size', 'backgroundcolor']);
const __typedNativeFields = new Map([['brightness', ['image']], ['volume', ['sound']], ['size', ['image', 'text']], ['backgroundcolor', ['text']]]);
function __nativeValueField(state, key) {
    if (!state || !__nativeProperty(state, key)) return false;
    if (state.policy && Object.hasOwn(state.policy.numbers, key)) return true;
    if (!__nativeFields.has(key)) return false;
    const kinds = __typedNativeFields.get(key);
    return !kinds || kinds.some(kind => Object.hasOwn(state.initial, kind));
}
const __vectors = new Set(['origin', __scaleProperty, 'angles', 'color', 'size', 'parallaxDepth', 'clearcolor', 'ambientcolor', 'skylightcolor', 'bloomtint', 'backgroundcolor', 'padding']);
function __coerce(value, key) {
    if (__vectors.has(key)) {
        const fields = value && typeof value === 'object' && !(value instanceof __Vector) && 'x' in value ? [value] : null;
        const a = typeof value === 'string' ? value.trim().split(/\s+/).map(Number) : Array.isArray(value) ? value : fields;
        if (a) return key === 'size' || key === 'parallaxDepth' || key === 'padding' ? new Vec2(...a) : new Vec3(...a);
        if (typeof value === 'number') return key === 'padding' ? new Vec2(value) : new Vec3(value);
    }
    return value;
}
function __nativeField(value, key, state) {
    value = __coerce(value, key);
    if (value instanceof __Vector && key === 'size') return new Vec2(Math.fround(value.x), Math.fround(value.y));
    if (value instanceof __Vector) return new Vec3(Math.fround(value.x), Math.fround(value.y), Math.fround(value.z));
    if (typeof value !== 'number') return value;
    const convert = state?.policy?.numbers[key]?.convert ?? Math.fround;
    if (convert === Math.trunc && (!Number.isFinite(value) || value < 0)) return value;
    return convert(value);
}
function __copiesNativeField(state, object, key) {
    if (!__nativeValueField(state, key)) return false;
    const descriptor = Object.getOwnPropertyDescriptor(object, key);
    return descriptor && 'value' in descriptor && (descriptor.configurable || descriptor.writable);
}
const __defineProperty = Object.defineProperty;
const __defineProperties = Object.defineProperties;
const __reflectDefineProperty = Reflect.defineProperty;
function __nativeDescriptorField(object, key) {
    const state = __layerStates.get(object);
    return __nativeValueField(state, key);
}
function __descriptorKey(object, key) {
    return __layerStates.has(object) && typeof key !== 'string' && typeof key !== 'symbol' ? Reflect.ownKeys({[key]:null})[0] : key;
}
function __descriptorFields(descriptor, fields) {
    if (!descriptor || (typeof descriptor !== 'object' && typeof descriptor !== 'function')) return descriptor;
    return new Proxy({}, {
        has(target, key) {
            const present = Reflect.has(descriptor, key);
            if (present) fields.push(key);
            return present;
        },
        get(target, key) { return Reflect.get(descriptor, key, descriptor); }
    });
}
function __collectedDescriptor(scratch, key, fields) {
    const descriptor = Object.getOwnPropertyDescriptor(scratch, key);
    const collected = {__proto__:null};
    for (const field of fields) collected[field] = descriptor[field];
    return collected;
}
function __marshalDescriptor(object, key, descriptor) {
    if (__nativeDescriptorField(object, key) && Object.hasOwn(descriptor, 'value')) descriptor.value = __nativeField(descriptor.value, key, __layerStates.get(object));
    return descriptor;
}
function __nativeDefinition(define) {
    return {defineProperty(object, key, descriptor) {
        key = __descriptorKey(object, key);
        if (!__nativeDescriptorField(object, key)) return define(object, key, descriptor);
        const scratch = {};
        const fields = [];
        __reflectDefineProperty(scratch, key, __descriptorFields(descriptor, fields));
        descriptor = __collectedDescriptor(scratch, key, fields);
        return define(object, key, __marshalDescriptor(object, key, descriptor));
    }}.defineProperty;
}
Object.defineProperty = __nativeDefinition(Object.defineProperty);
Reflect.defineProperty = __nativeDefinition(Reflect.defineProperty);
Object.defineProperties = {defineProperties(object, descriptors) {
    if (!__layerStates.has(object) || !descriptors || (typeof descriptors !== 'object' && typeof descriptors !== 'function')) return __defineProperties(object, descriptors);
    const collected = [];
    const source = new Proxy({}, {
        ownKeys() { return Reflect.ownKeys(descriptors); },
        getOwnPropertyDescriptor(target, key) {
            const descriptor = Reflect.getOwnPropertyDescriptor(descriptors, key);
            return descriptor ? {__proto__:null, ...descriptor, configurable:true} : undefined;
        },
        get(target, key) {
            const descriptor = Reflect.get(descriptors, key, descriptors);
            const fields = [];
            collected.push([key, fields]);
            return __descriptorFields(descriptor, fields);
        }
    });
    const scratch = {};
    __defineProperties(scratch, source);
    for (const [key, fields] of collected) {
        const descriptor = __collectedDescriptor(scratch, key, fields);
        __defineProperty(object, key, __marshalDescriptor(object, key, descriptor));
    }
    return object;
}}.defineProperties;
function __plain(value, key) {
    if (value instanceof __Vector) {
        let parts = __vectorComponents.filter(k => k in value).map(k => value[k]);
        if (key === 'angles') parts = parts.map(n => n * Math.PI / 180);
        return parts.join(' ');
    }
    return value;
}
function __wrap(value, path, key, eager = false, converted = false, owner = null) {
    value = __coerce(value, key);
    if (value instanceof __Vector) {
        if (key === 'angles' && !converted) value = value.multiply(180 / Math.PI);
        const proxy = new Proxy(value, { set(target, k, v, receiver) {
            if (!__vectorComponents.includes(k) || !(k in target)) return Reflect.set(target, k, v, receiver);
            const descriptor = Object.getOwnPropertyDescriptor(target, k);
            if (descriptor && (!('value' in descriptor) || !descriptor.writable)) return Reflect.set(target, k, v, receiver);
            if (target[k] !== v) {
                if (!Reflect.set(target, k, v, target)) return false;
                if (!__nativeWrite && __ownsSlot(owner) && __currentVector(path, proxy)) __queueNative(path, __plain(target, key));
            }
            return true;
        } });
        if (owner) __nestedOwners.set(proxy, owner);
        return proxy;
    }
    if (!value || typeof value !== 'object') return value;
    const state = __objectPath.test(path) ? {phase:__alive, slot:Number(path.slice(__objectPrefix.length)), initial:__initial[Number(path.slice(__objectPrefix.length))]} : null;
    owner = state ?? owner;
    const nested = path.startsWith(__objectPrefix) && !state;
    const effect = __effectPath.test(path);
    const nativeLength = Array.isArray(value) ? value.length : 0;
    let custom;
    let deleted;
    const effectId = effect && Object.hasOwn(value, 'id') ? value.id : undefined;
    if (effect) delete value.id;
    if (state) {
        delete value.id;
        state.policy = __classPolicy(state.initial);
        const hidden = state.policy
            ? [...new Set([...Object.keys(value), ...__privateLayerFields])].filter(key => !state.policy.fields.has(key))
            : Object.hasOwn(state.initial, 'sound') ? __soundHidden
            : Object.hasOwn(state.initial, 'particle') ? __particleHidden
            : Object.hasOwn(state.initial, 'image') ? __imageHidden : [];
        if (hidden.length) {
            state.hidden = new Map(hidden.map(key => [key, __wrap(value[key], path + '/' + key.replace(/~/g,'~0').replace(/\//g,'~1'), key, false, false, owner)]));
            for (const key of hidden) delete value[key];
        }
        for (const [key, number] of Object.entries(state.policy?.numbers ?? {}))
            if (!Object.hasOwn(value, key) && Object.hasOwn(number, 'initial')) value[key] = number.initial;
        for (const key of Object.keys(value)) if (__nativeValueField(state, key)) value[key] = __nativeField(value[key], key, state);
    }
    const wrapped = new Set();
    if (eager) for (const k of Object.keys(value)) {
        if (!__nativeValueField(state, k)) value[k] = __wrap(value[k], path + '/' + k.replace(/~/g,'~0').replace(/\//g,'~1'), k, true, false, owner);
        wrapped.add(k);
    }
    const proxy = new Proxy(value, { get(target, k, receiver) {
        if ((state && !__nativeProperty(state, k)) || (nested && __customNested(target, k, custom, deleted, effect, nativeLength))) return Reflect.get(target, k, receiver);
        if (state?.phase === __retired) {
            const descriptor = Object.getOwnPropertyDescriptor(target, k);
            if (!descriptor || descriptor.configurable || descriptor.writable) return undefined;
        }
        const child = target[k];
        if (__nativeValueField(state, k)) return __copiesNativeField(state, target, k) ? __nativeField(child, k, state) : child;
        if (typeof k !== 'string' || wrapped.has(k) || (!__vectors.has(k) && (!child || typeof child !== 'object'))) return child;
        const descriptor = Object.getOwnPropertyDescriptor(target, k);
        if (!descriptor || (!descriptor.configurable && !descriptor.writable)) return child;
        const result = __wrap(child, path + '/' + k.replace(/~/g,'~0').replace(/\//g,'~1'), k, false, false, owner);
        if (result !== child) {
            if (descriptor.writable === false) __defineProperty(target, k, {value:result});
            else target[k] = result;
            wrapped.add(k);
        }
        return result;
    }, defineProperty(target, k, descriptor) {
        if (state && k === 'name') __layerLookup = undefined;
        if (state && !__nativeProperty(state, k)) return Reflect.defineProperty(target, k, descriptor);
        if (nested && __customNested(target, k, custom, deleted, effect, nativeLength)) {
            const defined = Reflect.defineProperty(target, k, descriptor);
            if (defined) (custom ??= new Set()).add(k);
            return defined;
        }
        if (!('value' in descriptor) && descriptor.writable === false) {
            const child = this.get(target, k);
            if (__nativeValueField(state, k)) target[k] = child;
        }
        const previous = target[k];
        const changed = 'value' in descriptor && descriptor.value !== previous;
        const p = path + '/' + String(k).replace(/~/g,'~0').replace(/\//g,'~1');
        const json = changed && !__nativeWrite ? JSON.stringify(__plain(descriptor.value, k)) ?? 'null' : null;
        const defined = Reflect.defineProperty(target, k, descriptor);
        if (defined && changed) {
            wrapped.delete(k);
            if (!__nativeWrite && __ownsSlot(owner)) __queueSerializedNative(p, json, effect && k === 'visible'
                && typeof previous === 'boolean' && typeof descriptor.value === 'boolean');
        }
        return defined;
    }, deleteProperty(target, k) {
        if (state && k === 'name') __layerLookup = undefined;
        if (state && !__nativeProperty(state, k)) return Reflect.deleteProperty(target, k);
        if (nested && __customNested(target, k, custom, deleted, effect, nativeLength)) return Reflect.deleteProperty(target, k);
        const native = nested && Object.hasOwn(target, k);
        const removed = Reflect.deleteProperty(target, k);
        if (removed) {
            wrapped.delete(k);
            if (native) (deleted ??= new Set()).add(k);
        }
        return removed;
    }, set(target, k, v, receiver) {
        if (state && k === 'name') __layerLookup = undefined;
        if (state && !__nativeProperty(state, k)) return Reflect.set(target, k, v, receiver);
        if (nested && __customNested(target, k, custom, deleted, effect, nativeLength)) {
            (custom ??= new Set()).add(k);
            const written = Reflect.set(target, k, v, receiver);
            if (!written) custom.delete(k);
            return written;
        }
        if (state?.phase === __retired) return true;
        const descriptor = Object.getOwnPropertyDescriptor(target, k);
        if (descriptor && (!('value' in descriptor) || !descriptor.writable)) return Reflect.set(target, k, v, receiver);
        v = __nativeValueField(state, k) ? __nativeField(v, k, state) : __coerce(v, k);
        if (state && k === __scaleProperty && v instanceof __Vector) v = __nativeField(v, k, state);
        const previous = target[k];
        if (!(previous instanceof __Vector && v instanceof __Vector && __vectorComponents.every(component => previous[component] === v[component])) && previous !== v) {
            const p = path + '/' + String(k).replace(/~/g,'~0').replace(/\//g,'~1');
            const json = !__nativeWrite ? JSON.stringify(__plain(v, k)) ?? 'null' : null;
            const numericVector = v instanceof __Vector && __vectors.has(k)
                && typeof v.x === 'number' && typeof v.y === 'number'
                && (typeof v.z === 'number' || (!('z' in v) && !('w' in v)));
            const stored = __nativeValueField(state, k) ? v : v instanceof __Vector
                ? numericVector
                    ? __wrap(k === 'size' || k === 'parallaxDepth' || k === 'padding' ? new Vec2(v.x,v.y) : new Vec3(v.x,v.y,v.z),p,k,true,true,owner)
                    : __wrap(__plain(v,k),p,k,true,false,owner)
                : __wrap(v,p,k,true,false,owner);
            if (!Reflect.set(target, k, stored, target)) return false;
            if (!__nativeWrite && __ownsSlot(owner)) __queueSerializedNative(p, json, effect && k === 'visible'
                && typeof previous === 'boolean' && typeof v === 'boolean');
            wrapped.add(k);
        }
        return true;
    }});
    if (state) __layerStates.set(proxy, state);
    else if (owner) __nestedOwners.set(proxy, owner);
    if (effectId !== undefined) __effectHidden.set(proxy, {path, fields:new Map([['id', effectId]])});
    return proxy;
}
const __commands = [];
const __sortCommand = 'sort';
const __effectCommand = 'effect';
const __effectMaterialCommand = 'effectMaterial';
let __sortPending = false;
const __sprites = new Map();
function __command(kind, target, op, value) {
    if (__commands.length < 4096) __commands.push([kind, String(target), op, value === undefined ? null : value]);
}
function __drainCommands() {
    if (__sortPending) {
        __commands.push([__sortCommand, '', '', __nativeOrder.map(layer => __layerSlots.get(layer))]);
        __sortPending = false;
    }
    const out = JSON.stringify(__commands); __commands.length = 0; return out;
}
function __restoreLayerOrder(json) {
    __nativeOrder = JSON.parse(json).map(slot => __layers[slot]);
    __layerOrder = __nativeOrder.filter(layer => __layerStates.get(layer).phase !== __retired);
    __layerLookup = undefined;
    __sortPending = false;
}
function __setSprite(index, times) {
    const starts = [];
    let duration = 0;
    for (const time of times) { starts.push(duration + time * 0.5); duration += time; }
    __sprites.set(index, {times, starts, duration});
}
function __setSprites(json) {
    __sprites.clear();
    for (const [index, times] of JSON.parse(json)) __setSprite(index, times);
}
function __setParticles(json) {
    for (const [id, playing] of JSON.parse(json)) {
        const state = __particleStates.get(id);
        if (state) state.playing = playing;
    }
}
function __setSounds(json) {
    const states = new Map(JSON.parse(json));
    for (const [id, state] of __soundStates) state.playing = states.get(id) === true;
}
function __textureAnimation(index) {
    const owner = __layerStates.get(__layers[index]);
    const info = () => (__ownsSlot(owner) && __sprites.get(index)) || {times: [0], starts: [0], duration: 0};
    const state = {frame: 0, playing: true, rate: 1, since: 0, joined: true};
    const push = (op, value) => {
        if (__ownsSlot(owner) && owner?.phase !== __retired) __command('sprite', index, op, value);
    };
    const estimate = () => {
        const {times, starts, duration} = info();
        const frame = state.frame % times.length;
        if (!state.playing || duration <= 0 || times.length <= 1) return frame;
        const start = state.joined ? 0 : starts[frame];
        let cursor = (start + (engine.runtime - state.since) * state.rate) % duration;
        if (cursor < 0) cursor += duration;
        for (let i = 0; i < times.length; i++) {
            if (cursor < times[i]) return i;
            cursor -= times[i];
        }
        return times.length - 1;
    };
    return {
        get frameCount() { return info().times.length; },
        get duration() { return info().duration; },
        get rate() { return state.rate; },
        set rate(value) { state.frame = estimate(); state.since = engine.runtime; state.joined = false; state.rate = Number(value) || 0; push('rate', state.rate); },
        play() { if (!state.playing) { state.playing = true; state.since = engine.runtime; state.joined = false; } push('play'); },
        stop() { state.playing = false; state.frame = 0; state.joined = false; push('stop'); },
        pause() { state.frame = estimate(); state.playing = false; state.joined = false; push('pause'); },
        isPlaying() { return state.playing; },
        getFrame() { return estimate(); },
        setFrame(frame) { state.frame = Math.max(0, Math.floor(Number(frame) || 0)); state.since = engine.runtime; state.joined = false; push('frame', state.frame); },
        join() { state.playing = true; state.rate = 1; state.frame = 0; state.since = 0; state.joined = true; push('join'); }
    };
}
const __videos = new Map();
const __videoStates = new Map();
function __videoTexture(layer, slot) {
    if (__videos.has(slot)) return __videos.get(slot).handle;
    if (!__hasVideoTexture(JSON.stringify(__nativeLayer(layer, slot)))) return undefined;
    const state = {position:0, duration:0, rate:1, looping:true, playing:true, ended:0, ...__videoStates.get(slot), callbacks:[]};
    const push = (op, value) => { if (__layerStates.get(layer)?.phase !== __alive) return; state.pending=true; __command('video', String(slot), op, value); };
    state.handle = {
        get duration() { return state.duration; },
        get rate() { return state.rate; },
        set rate(value) { if (Number.isFinite(value)) { state.rate=value; push('rate',value); } },
        get loop() { return state.looping; },
        set loop(value) { state.looping=!!value; push('loop',state.looping); },
        getCurrentTime: () => state.position,
        setCurrentTime(value) { if (Number.isFinite(value)) { state.position=Math.max(0,value); push('seek',state.position); } },
        play() { state.playing=true; push('play'); },
        pause() { state.playing=false; push('pause'); },
        stop() { state.playing=false; state.position=0; push('stop'); },
        isPlaying: () => state.playing,
        addEndedCallback(callback) {
            if (typeof callback !== 'function' || state.callbacks.length >= __listenerLimit) return;
            state.callbacks.push({callback,owner:__current});
        }
    };
    __videos.set(slot, state);
    return state.handle;
}
function __setVideos(json, committed) {
    const values = JSON.parse(json);
    __videoStates.clear();
    for (const value of values) __videoStates.set(value.object, value);
    for (const value of values) {
        const state = __videos.get(value.object);
        if (!state) continue;
        state.duration=value.duration;
        if (state.pending && !committed) continue;
        const ended = Math.min(__listenerLimit, Math.max(0, value.ended-state.ended));
        Object.assign(state, value);
        state.pending=false;
        for (let index=0;index<ended;index++) for (const {owner,callback} of [...state.callbacks]) {
            if (owner?.disabled || owner?.retired) continue;
            try { __call(owner, callback); } catch (error) { __log('video ended: ' + __errorText(error)); }
        }
    }
}
const __sceneListeners = new Map();
const __listenerLimit = 4096;
function __listen(name, callback) {
    if (typeof name !== 'string' || typeof callback !== 'function') return false;
    const count = [...__sceneListeners.values()].reduce((sum, entries) => sum + entries.length, 0);
    if (count >= __listenerLimit) throw Error('SceneScript listener budget exceeded');
    const entries = __sceneListeners.get(name) ?? [];
    entries.push({callback, owner:__current});
    __sceneListeners.set(name, entries);
    return true;
}
function __unlisten(name, callback) {
    const entries = __sceneListeners.get(name);
    if (!entries) return false;
    if (callback === undefined) return __sceneListeners.delete(name);
    const remaining = entries.filter(entry => entry.callback !== callback);
    if (remaining.length) __sceneListeners.set(name, remaining);
    else __sceneListeners.delete(name);
    return remaining.length !== entries.length;
}
function __hasListeners(name) {
    return __sceneListeners.get(name)?.some(entry => !entry.owner?.disabled && !entry.owner?.retired) ?? false;
}
function __emit(name, ...args) {
    for (const entry of [...(__sceneListeners.get(name) ?? [])]) {
        if (entry.owner?.disabled || entry.owner?.retired || !__sceneListeners.get(name)?.includes(entry)) continue;
        try { __call(entry.owner, entry.callback, ...args); }
        catch (error) { __log('scene event ' + name + ': ' + __errorText(error)); }
    }
}
function __applyUserProperties(properties = engine.userProperties) {
    for (const m of __modules) {
        if (m && !m.disabled) __callback(m, 'applyUserProperties', properties);
    }
    __emit('applyUserProperties', properties);
}
function __applyGeneralSettings(json) {
    const settings = JSON.parse(json);
    for (const m of __modules) {
        if (m && !m.disabled) __callback(m, 'applyGeneralSettings', settings);
    }
    __emit('applyGeneralSettings', settings);
}
function __resizeScreen(width, height) {
    engine.screenResolution = new Vec2(width, height);
    for (const m of __modules) {
        if (m && !m.disabled) __callback(m, 'resizeScreen', engine.screenResolution);
    }
    __emit('resizeScreen', width, height);
}
function __installMethod(layer, key, value) {
    const owner = __ownerState(layer);
    const method = owner ? new Proxy(value, owner.methods ??= {apply(target, receiver, args) {
        if (__ownsSlot(owner)) return Reflect.apply(target, receiver, args);
    }}) : value;
    Object.defineProperty(layer, key, {value:method, enumerable:true, configurable:true, writable:true});
}
const __materialHandles = new WeakSet();
const __materialVectors = [null, null, Vec2, Vec3, Vec4];
function __materialValues(value) {
    if (typeof value === 'number') return Number.isFinite(value) ? [value] : null;
    if (!value || typeof value !== 'object') return null;
    const values = Array.isArray(value) ? value.slice(0, __maxMaterialValues).map(Number)
        : __vectorComponents.filter(key => typeof value[key] === 'number').map(key => value[key]);
    return values.length && values.every(Number.isFinite) ? values : null;
}
function __installEffect(layer, effect) {
    if (!effect || __materialHandles.has(effect)) return effect;
    __materialHandles.add(effect);
    let info;
    const materials = new Map();
    const metadata = () => {
        if (info) return info;
        const entry = {file:__nativeValue(effect, 'file'),passes:__nativeValue(effect, 'passes'),visible:effect.visible};
        const result = JSON.parse(__effectMaterials(JSON.stringify({id:__layerId(layer),effects:[entry]}, (key, value) => __plain(value, key))));
        if (Object.hasOwn(result, 'Err')) throw Error(result.Err);
        info = result.Ok;
        return info;
    };
    const at = index => {
        if (!Number.isInteger(index) || index < 0) return null;
        if (materials.has(index)) return materials.get(index);
        const source = metadata().materials.find(source => source.source === index);
        if (!source) return null;
        const owner = __layerStates.get(layer);
        const effectIndex = (__nativeValue(layer, 'effects') || []).indexOf(effect);
        const material = {};
        for (const [key, binding] of Object.entries(source.uniforms)) {
            let current = binding.value?.map(Math.fround);
            Object.defineProperty(material, key, {enumerable:true,
                get:() => {
                    if (!current) return undefined;
                    const Vector = __materialVectors[binding.width];
                    return binding.width === 1 ? current[0] : Vector ? new Vector(...current) : undefined;
                },
                set:value => {
                    const values = __materialValues(value);
                    if (!values) return;
                    if (__ownsSlot(owner) && effectIndex !== -1)
                        __command(__effectMaterialCommand, __layerId(layer), binding.uniform, [effectIndex, source.source, values]);
                    current = values.map(Math.fround);
                }});
        }
        materials.set(index, material);
        return material;
    };
    __installMethod(effect, 'getMaterialCount', () => metadata().count);
    __installMethod(effect, 'getMaterial', index => at(index === undefined ? 0 : index));
    __installMethod(effect, 'setMaterialProperty', (key, value) => {
        for (const source of metadata().materials)
            if (Object.hasOwn(source.uniforms, key)) at(source.source)[key] = value;
    });
    __installMethod(effect, 'executeMaterialFunction', name => {
        const effects = __nativeValue(layer, 'effects');
        const index = effects?.indexOf(effect);
        if (typeof name !== 'string' || index === undefined || index < 0 ||
            !Object.hasOwn(metadata().clear_functions, name)) throw Error('effect function unavailable');
        __command(__effectCommand, __layerId(layer), name, index);
    });
    return effect;
}
function __installLayer(layer, slot) {
    const state = __layerStates.get(layer);
    for (const [key,value] of Object.entries({visible:true, origin:'0 0 0', scale:'1 1 1', angles:'0 0 0', color:'1 1 1', alpha:1, parallaxDepth:'1 1'}))
        if (__nativeProperty(state, key) && layer[key] === undefined) layer[key] = value;
    __installMethod(layer, 'getParent', () => {
        const parent = __nativeValue(layer, 'parent');
        return parent === undefined || parent === null ? null : __byId.get(String(parent));
    });
    __installMethod(layer, 'getTransformMatrix', () => __worldPose(layer)?.matrix ?? __localTransform(layer));
    __installMethod(layer, 'getChildren', () => __layers.filter(child => String(__nativeValue(child, 'parent')) === String(__layerId(layer))));
    __installMethod(layer, 'getRoot', () => {
        const seen = new Set();
        let root = layer;
        while (root.getParent() && !seen.has(root)) { seen.add(root); root = root.getParent(); }
        return root;
    });
    __installMethod(layer, 'isAncestorOf', other => __isAncestorOf(layer, __layerOf(other)));
    __installMethod(layer, 'getDepth', () => {
        const seen = new Set();
        let depth = 0;
        for (let parent = layer.getParent(); parent && !seen.has(parent); parent = parent.getParent()) { seen.add(parent); depth++; }
        return depth;
    });
    __installMethod(layer, 'setParent', (key, attachmentOrAdjust, adjustTransform) => __setParent(layer, key, attachmentOrAdjust, adjustTransform));
    __installMethod(layer, 'removeFromParent', () => __setParent(layer, undefined));
    __installMethod(layer, 'getAttachmentIndex', name => __attachment(layer, name)?.index ?? __missingAttachmentIndex);
    __installMethod(layer, 'getAttachmentMatrix', name => {
        const pose = __attachmentWorld(layer, name);
        return pose?.matrix;
    });
    __installMethod(layer, 'getAttachmentOrigin', name => {
        const pose = __attachmentWorld(layer, name);
        return pose?.matrix.translation();
    });
    __installMethod(layer, 'getAttachmentAngles', name => {
        const pose = __attachmentWorld(layer, name);
        return pose?.matrix.extractEuler();
    });
    __installMethod(layer, 'rotateObjectSpace', angles => {
        layer.angles = Mat4.fromEuler(layer.angles).multiply(Mat4.fromEuler(angles)).extractEuler();
    });
    if (!state.policy || state.policy.methods.has('getEffect')) __installMethod(layer, 'getEffect', key => {
        const effects = __nativeValue(layer, 'effects') || [];
        return __installEffect(layer, (typeof key === 'number' ? effects[key] : effects.find(effect => effect.name === key)) ?? null);
    });
    __installMethod(layer, 'getAnimation', name => __getAnimation(layer, __objectPrefix + slot, name));
    if (__nativeValue(layer, 'image') !== undefined) {
        let animation;
        __installMethod(layer, 'getTextureAnimation', () => __sprites.has(slot) ? animation ??= __textureAnimation(slot) : undefined);
        __installMethod(layer, 'getVideoTexture', () => __videoTexture(layer, slot));
    }
    if (state.policy?.methods.has('getEffectCount') || __nativeValue(layer, 'image') !== undefined)
        __installMethod(layer, 'getEffectCount', () => (__nativeValue(layer, 'effects') || []).length);
    if (__nativeValue(layer, 'sound') !== undefined) {
        const id = String(__layerId(layer));
        const state = {playing: layer.startsilent !== true};
        __soundStates.set(id, state);
        __installMethod(layer, 'play', () => { state.playing = true; __command('sound', id, 'play'); });
        __installMethod(layer, 'stop', () => { state.playing = false; __command('sound', id, 'stop'); });
        __installMethod(layer, 'pause', () => { state.playing = false; __command('sound', id, 'pause'); });
        __installMethod(layer, 'isPlaying', () => state.playing);
    }
    if (__nativeValue(layer, 'particle') !== undefined) {
        const id = String(__layerId(layer));
        const state = {playing: true, paused: false};
        __particleStates.set(id, state);
        const instance = {};
        const override = __authoredLayer(slot)?.instanceoverride;
        for (const [key, limit] of Object.entries(__particleFactors)) {
            const authored = override?.[key];
            const initial = Number(typeof authored === 'object' ? authored?.value : authored);
            let current = Number.isFinite(initial) ? initial : 1;
            Object.defineProperty(instance, key, {enumerable:true, get: () => current, set: value => {
                const next = Number(value);
                if (!Number.isFinite(next)) return;
                current = Math.max(0, Math.min(limit, next));
                __command('particle', id, key, current);
            }});
        }
        __particleInstanceVector(instance, id, 'colorn', override?.colorn?.value ?? override?.colorn ?? 1, true);
        for (let index = 0; index < __particleControlPointCount; index++) {
            const key = 'controlpoint' + index;
            __particleInstanceVector(instance, id, key, override?.[key]?.value ?? override?.[key] ?? 0, false);
        }
        Object.defineProperty(layer, 'instance', {value:instance, enumerable:true, configurable:true, writable:false});
        __installMethod(layer, 'play', () => { state.paused = false; state.playing = true; __command('particle', id, 'play'); });
        __installMethod(layer, 'stop', () => { state.playing = false; __command('particle', id, 'stop'); });
        __installMethod(layer, 'pause', () => { state.paused = true; state.playing = false; __command('particle', id, 'pause'); });
        __installMethod(layer, 'isPlaying', () => state.playing);
        __installMethod(layer, 'emitParticles', (count = 1) => {
            const amount = typeof count === 'number' && count === (count | 0) ? count || 1 : 1;
            if (amount < 0) return;
            if (!state.paused) state.playing = true;
            __command('particle', id, 'emit', amount);
        });
    }
}
let __initialJson = [];
function __layerOutline(layer) {
    return layer && Object.fromEntries(Object.keys(layer).map(key => [key, key === 'id' ? layer.id : null]));
}
function __authoredLayer(slot) {
    return __initialJson[slot] === undefined ? undefined : JSON.parse(__initialJson[slot]);
}
function __rememberLayer(slot, layer) {
    __initialJson[slot] = JSON.stringify(layer);
    __initial[slot] = __layerOutline(layer);
}
function __setup(initialLayers, json, props, screenWidth, screenHeight) {
    __initialJson = initialLayers;
    globalThis.__initial = initialLayers.map(layer => __layerOutline(JSON.parse(layer)));
    globalThis.__scene = __wrap(JSON.parse(json), '', '');
    const zoom = __scene.general?.zoom;
    const seed = typeof zoom === 'number' ? zoom : zoom?.value;
    __cameraTransforms = {eye:new Vec3(2,2,2), center:new Vec3(0,0,0), up:new Vec3(0,1,0), zoom:typeof seed === 'number' && seed > 0 ? Math.fround(seed) : 1};
    globalThis.__layers = __scene.objects || [];
    globalThis.__layerOrder = [...__layers];
    __layerLookup = undefined;
    globalThis.__nativeOrder = [...__layers];
    globalThis.__layerCount = __layers.length;
    __nextLayerId = BigInt(__layers.length);
    globalThis.__layerSlots = new WeakMap(__layers.map((layer, index) => [layer, index]));
    globalThis.__byId = new Map(__layers.map(layer => [String(__layerId(layer)), layer]));
    __setUserProperties(JSON.parse(props));
    engine.canvasSize = new Vec2(__scene.general?.orthogonalprojection?.width ?? 1920, __scene.general?.orthogonalprojection?.height ?? 1080);
    engine.screenResolution = new Vec2(screenWidth ?? engine.canvasSize.x, screenHeight ?? engine.canvasSize.y);
    __bloomEnabled = !!thisScene.bloom;
    __layers.forEach(__installLayer);
    __changes.clear();
}
function __owner(path) {
    const parts = path.split('/').slice(1).map(s => s.replace(/~1/g,'/').replace(/~0/g,'~'));
    const key = parts.pop(); let object = __scene;
    for (const p of parts) object = __nativeValue(object, p);
    return [object, key];
}
const __animationLayerPath = /^\/objects\/(\d+)\/animationlayers\/(\d+)\//;
function __authoredAnimation(slot, index) {
    const definition = __puppetLayers.get(slot)?.find(entry => entry?.authored === index);
    return definition ? __puppetHandle(definition) : null;
}
function __scriptObject(path) {
    const [object] = __owner(path);
    const match = __animationLayerPath.exec(path);
    return (match && __authoredAnimation(Number(match[1]), Number(match[2]))) ?? object;
}
function __currentVector(path, vector) {
    try {
        const layer = __pathLayer(path);
        if (layer && __layerStates.get(layer)?.phase === __retired) return false;
        const [object, key] = __owner(path);
        return object != null && __nativeValue(object, key) === vector;
    } catch { return false; }
}
function __nativeValue(object, key) {
    const hidden = __layerStates.get(object)?.hidden || __effectHidden.get(object)?.fields;
    return hidden?.has(key) ? hidden.get(key) : object[key];
}
function __nativeSet(object, key, value) {
    const owner = __ownerState(object);
    if (!__ownsSlot(owner)) return;
    const effect = __effectHidden.get(object);
    const hidden = __layerStates.get(object)?.hidden || effect?.fields;
    if (!hidden?.has(key)) { object[key] = value; return; }
    const path = (effect?.path || __objectPrefix + __layerSlots.get(object)) + '/' + key;
    hidden.set(key, __wrap(value, path, key, true, false, owner));
    if (!__nativeWrite) __queueNative(path, __plain(value, key));
}
function __isAncestorOf(layer, other) {
    const seen = new Set();
    for (let parent = other?.getParent?.(); parent && !seen.has(parent); parent = parent.getParent()) {
        if (parent === layer) return true;
        seen.add(parent);
    }
    return false;
}
function __localTransform(layer) {
    const origin = layer.origin, angles = layer.angles, scale = layer.scale;
    return Mat4.compose(origin, angles, scale);
}
function __attachment(parent, key) {
    const path = __nativeValue(parent, 'image');
    if (typeof path !== 'string') return null;
    return JSON.parse(__attachmentInfo(String(__layerId(parent)), path, String(key), typeof key === 'number' ? key : null));
}
function __attachmentMatrix(info) {
    const [a,b,c,d,x,y] = info.affine;
    const matrix = new Mat4();
    matrix.m = [a,b,0,0,c,d,0,0,0,0,1,0,x,y,info.depth,1];
    return matrix;
}
function __attachmentWorld(layer, key) {
    const pose = __worldPose(layer), info = __attachment(layer, key);
    return pose && info ? {matrix:pose.matrix.multiply(__attachmentMatrix(info))} : null;
}
function __worldPose(layer) {
    const chain = [];
    const seen = new Set();
    for (let node = layer; node && !seen.has(node); node = node.getParent()) { chain.push(node); seen.add(node); }
    let matrix = Mat4.identity();
    for (const node of chain.reverse()) {
        const parent = node.getParent(), name = __nativeValue(node, 'attachment');
        if (parent && name !== undefined && name !== null) {
            const info = __attachment(parent, name);
            if (!info) return null;
            matrix = matrix.multiply(__attachmentMatrix(info));
        }
        matrix = matrix.multiply(__localTransform(node));
    }
    return {matrix};
}
function __setParent(layer, key, attachmentOrAdjust, adjustTransform) {
    const parent = key === undefined || key === null ? undefined :
        typeof key === 'number' ? __byId.get(String(key)) ?? __layerOf(key) : __layerOf(key);
    if (key !== undefined && key !== null && !__publicLayer(parent)) return false;
    if (parent === layer || (parent && __isAncestorOf(layer, parent))) return false;
    const attachment = typeof attachmentOrAdjust === 'boolean' || attachmentOrAdjust === undefined ? undefined : attachmentOrAdjust;
    const preserve = typeof attachmentOrAdjust === 'boolean' ? attachmentOrAdjust : adjustTransform === true;
    const info = attachment === undefined ? null : parent && __attachment(parent, attachment);
    if (attachment !== undefined && !info) return false;
    const currentAttachment = __nativeValue(layer, 'attachment') ?? null;
    if (layer.getParent() === parent && currentAttachment === (info?.name ?? null)) return true;
    const slot = __layerSlots.get(layer);
    const state = __layerStates.get(layer);
    if (slot === undefined || state?.phase !== __alive) return false;
    let local;
    if (preserve) {
        const world = __worldPose(layer);
        const basis = parent ? __worldPose(parent) : {matrix:Mat4.identity()};
        if (!world || !basis) return false;
        const frame = info ? basis.matrix.multiply(__attachmentMatrix(info)) : basis.matrix;
        const inverse = frame.inverse();
        if (!inverse.m.every(Number.isFinite)) return false;
        const adjusted = inverse.multiply(world.matrix);
        local = adjusted.decompose();
        if (adjusted.determinant() < 0) local.scale.x = -local.scale.x;
        const rotation = adjusted.multiply(Mat4.fromScale(new Vec3(1 / local.scale.x, 1 / local.scale.y, 1 / local.scale.z)));
        local.rotation = rotation.extractEuler();
        if (!local.scale.isFinite() || !local.translation.isFinite() || !local.rotation.isFinite()) return false;
        const rebuilt = Mat4.compose(local.translation, local.rotation, local.scale);
        if (!adjusted.m.every((value, slot) => Math.abs(value - rebuilt.m[slot]) < __equalityTolerance * Math.max(1, Math.abs(value)))) return false;
    }
    const id = parent ? __layerId(parent) : null;
    state.hidden ??= new Map();
    state.hidden.set('parent', id);
    state.hidden.set('attachment', info?.name ?? null);
    __queueNative(__objectPrefix + slot + '/parent', id);
    __queueNative(__objectPrefix + slot + '/attachment', info?.name ?? null);
    if (local) {
        layer.origin = local.translation;
        layer.angles = local.rotation;
        layer.scale = local.scale;
    }
    return true;
}
let __keyframes = [];
let __timelineDefinitions = [];
const __timelineHandles = new Map();
const __timelineOps = {play:0, pause:1, stop:2, setFrame:3, setRate:4, getFrame:5, getRate:6, isPlaying:7};
function __setKeyframes(json) {
    __timelineDefinitions = JSON.parse(json);
    __keyframes = __timelineDefinitions.map(entry => entry.path.split('/').slice(1).map(s => s.replace(/~1/g,'/').replace(/~0/g,'~')));
}
function __getAnimation(object, path, name) {
    const definition = __timelineDefinitions.find(entry => name === undefined
        ? __current?.object === object && entry.path === __current.path
        : entry.name === name && entry.path.startsWith(path + '/'));
    if (!definition) return undefined;
    if (!__timelineHandles.has(definition)) __timelineHandles.set(definition, {
        get fps() { return definition.fps; },
        get frameCount() { return definition.length; },
        get duration() { return definition.length / definition.fps; },
        get name() { return definition.name ?? ''; },
        get rate() { return __timelineControl(definition.clock, __timelineOps.getRate, 0); },
        set rate(value) { __timelineControl(definition.clock, __timelineOps.setRate, Number(value)); },
        play() { __timelineControl(definition.clock, __timelineOps.play, 0); },
        pause() { __timelineControl(definition.clock, __timelineOps.pause, 0); },
        stop() { __timelineControl(definition.clock, __timelineOps.stop, 0); },
        getFrame() { return __timelineControl(definition.clock, __timelineOps.getFrame, 0); },
        setFrame(frame) { __timelineControl(definition.clock, __timelineOps.setFrame, Number(frame)); },
        isPlaying() { return !!__timelineControl(definition.clock, __timelineOps.isPlaying, 0); }
    });
    return __timelineHandles.get(definition);
}
const __puppetLayers = new Map();
const __puppetHandles = new Map();
const __puppetSingle = new Set();
function __puppetHandle(definition) {
        if (!definition) return null;
        if (__puppetHandles.has(definition.clock)) return __puppetHandles.get(definition.clock);
        const owner = __layerStates.get(__layers[definition.slot]);
        const control = (op, value = 0) => __ownsSlot(owner) ? __puppetControl(definition.clock, __timelineOps[op], value) : undefined;
        const animation = {
            get fps() { return definition.fps; },
            get frameCount() { return definition.frame_count; },
            get duration() { return definition.duration; },
            get name() { return definition.name; },
            get visible() { return __ownsSlot(owner) ? __puppetVisibility(definition.clock, undefined) : undefined; },
            set visible(value) { if (__ownsSlot(owner)) __puppetVisibility(definition.clock, !!value); },
            get blend() { return __ownsSlot(owner) ? __puppetBlend(definition.clock, undefined) : undefined; },
            set blend(value) { if (__ownsSlot(owner)) __puppetBlend(definition.clock, Number(value)); },
            get rate() { return control('getRate'); },
            set rate(value) { control('setRate', Number(value)); },
            play() { control('play'); },
            pause() { control('pause'); },
            stop() { control('stop'); },
            setFrame(frame = undefined) { control('setFrame', Number(frame)); },
            getFrame() { return control('getFrame'); },
            isPlaying() { return !!control('isPlaying'); },
            addEndedCallback(callback) {
                if (!__ownsSlot(owner)) return;
                if (typeof callback !== 'function') throw TypeError('Animation callback must be a function');
                if (!__puppetCallbacks.has(definition.clock)) __puppetCallbacks.set(definition.clock, []);
                __puppetCallbacks.get(definition.clock).push({module:__current, callback});
            },
            getAnimation() { return undefined; }
        };
        __puppetHandles.set(definition.clock, animation);
        return animation;
}
let __bonesReady = false;
const __boneWorlds = new Map();
globalThis.__boneWorldReaders = false;
function __boneWorld(layer, transforms) {
    if (transforms.world === null) transforms.world = __worldPose(layer)?.matrix;
    return transforms.world;
}
function __setPuppets(json, bonesJson, shapesJson) {
    __boneWorlds.clear();
    __puppetLayers.clear();
    __puppetHandles.clear();
    __puppetSingle.clear();
    for (const definition of JSON.parse(json)) {
        const layer = __layers[definition.slot];
        if (!layer) continue;
        if (!__puppetLayers.has(definition.slot)) __puppetLayers.set(definition.slot, []);
        __puppetLayers.get(definition.slot)[definition.layer] = definition;
    }
    for (const [slot, animations] of __puppetLayers) {
        const layer = __layers[slot];
        __installMethod(layer, 'getAnimationLayer', (key = undefined) =>
            __puppetHandle(typeof key === 'number' ? animations[key] : animations.find(animation => animation.name === key)));
        __installMethod(layer, 'getAnimationLayerCount', () => animations.length);
        __installMethod(layer, 'createAnimationLayer', config => {
            const name = typeof config === 'string' ? config : config?.animation;
            if (typeof name !== 'string') return null;
            const options = typeof config === 'string' ? {} : Object.fromEntries(Object.entries(config).filter(([key]) => key !== 'animation'));
            const definition = JSON.parse(__puppetCreate(slot, name, false, JSON.stringify(options)));
            if (!definition) return null;
            const animation = __puppetHandle(definition);
            animations.splice(definition.layer, 0, definition);
            return animation;
        });
        __installMethod(layer, 'playSingleAnimation', (config, options = undefined) => {
            const name = typeof config === 'string' ? config : config?.animation;
            if (typeof name !== 'string') return null;
            const settings = typeof config === 'string' ? options ?? {} : config;
            const fields = Object.fromEntries(Object.entries(settings).filter(([key]) => key !== 'animation'));
            const definition = JSON.parse(__puppetCreate(slot, name, true, JSON.stringify(fields)));
            if (!definition) return null;
            const animation = __puppetHandle(definition);
            __puppetSingle.add(definition.clock);
            animations.splice(definition.layer, 0, definition);
            return animation;
        });
        __installMethod(layer, 'destroyAnimationLayer', key => {
            const animation = typeof key === 'number' ? animations[key] : typeof key === 'string'
                ? animations.find(entry => entry.name === key) : animations.find(entry => __puppetHandles.has(entry.clock) && __puppetHandles.get(entry.clock) === key);
            const index = animations.indexOf(animation);
            if (index < 0) return false;
            const clock = animation.clock;
            if (clock === undefined || !__puppetDestroy(clock)) return false;
            animations.splice(index, 1);
            __puppetHandles.delete(clock);
            __puppetSingle.delete(clock);
            __puppetCallbacks.delete(clock);
            return true;
        });
    }
    for (const [slot, shapes] of JSON.parse(shapesJson)) {
        const layer = __layers[slot];
        if (!layer) continue;
        const index = key => typeof key === 'number' && Number.isInteger(key) ? key :
            typeof key === 'string' ? shapes.indexOf(key) : -1;
        __installMethod(layer, 'getBlendShapeIndex', name => typeof name === 'string' ? shapes.indexOf(name) : -1);
        __installMethod(layer, 'getBlendShapeWeight', key => {
            const shape = index(key);
            return shape < 0 || shape >= shapes.length ? undefined : __puppetMorph(slot, shape, undefined);
        });
        __installMethod(layer, 'setBlendShapeWeight', (key, value) => {
            const shape = index(key);
            if (shape >= 0 && shape < shapes.length && Number.isFinite(Number(value))) __puppetMorph(slot, shape, Number(value));
        });
    }
    for (const [slot, bones, springs] of JSON.parse(bonesJson)) {
        const layer = __layers[slot];
        if (!layer) continue;
        const transforms = {world:null, edits:new Map(), slot, springs};
        __boneWorlds.set(layer, transforms);
        const index = key => typeof key === 'number' && Number.isInteger(key) ? key :
            typeof key === 'string' && key.length > 0 ? bones.findIndex(bone => bone.name === key) : -1;
        __installMethod(layer, 'getBoneCount', () => bones.length);
        __installMethod(layer, 'getBoneIndex', name => typeof name === 'string' && name.length > 0 ? index(name) : -1);
        __installMethod(layer, 'getBoneParentIndex', key => bones[index(key)]?.parent ?? -1);
        const getBone = (key, local) => {
            const bone = index(key);
            if (bone < 0 || bone >= bones.length) return undefined;
            const matrix = new Mat4();
            if (!__bonesReady) return matrix;
            const values = JSON.parse(__puppetBoneGet(slot, bone, local));
            if (!values) return undefined;
            matrix.m = values;
            return local ? matrix : (transforms.edits.get(bone) ?? __boneWorld(layer, transforms))?.multiply(matrix);
        };
        const setBone = (key, value, local) => {
            const bone = index(key);
            if (bone < 0 || bone >= bones.length) return;
            if (!(value instanceof Mat4)) throw TypeError('Bone transform must be a Mat4');
            if (!__bonesReady) return;
            const world = __worldPose(layer)?.matrix;
            if (!world) return;
            if (!local) {
                value = world.inverse().multiply(value);
                if (!value.m.every(Number.isFinite)) return;
            }
            if (!__puppetBoneSet(slot, bone, local, JSON.stringify([value.m, springs ? world.m : null]))) return;
            transforms.edits.set(bone, world);
            if (local) {
                for (let child = bone + 1; child < bones.length; child++) {
                    if (transforms.edits.get(bones[child].parent) === world) transforms.edits.set(child, world);
                }
            }
        };
        __installMethod(layer, 'getBoneTransform', key => getBone(key, false));
        __installMethod(layer, 'setBoneTransform', (key, value) => setBone(key, value, false));
        __installMethod(layer, 'getLocalBoneTransform', key => getBone(key, true));
        __installMethod(layer, 'setLocalBoneTransform', (key, value) => setBone(key, value, true));
    }
}
const __puppetCallbacks = new Map();
function __puppetEnded(index) {
    for (const {module, callback} of __puppetCallbacks.get(index) ?? []) {
        if (module?.disabled || module?.retired) continue;
        try { __call(module, callback); } catch (error) { __log('animation ended: ' + __errorText(error)); }
    }
    if (__puppetSingle.delete(index)) {
        for (const animations of __puppetLayers.values()) {
            const layer = animations.findIndex(animation => animation.clock === index);
            if (layer >= 0) animations.splice(layer, 1);
        }
        __puppetHandles.delete(index);
        __puppetCallbacks.delete(index);
    }
}
function __puppetEvent(slot, name, frame) {
    const layer = __layers[slot];
    if (!layer || __layerStates.get(layer)?.phase === __retired) return;
    const event = {name, frame};
    for (const module of __modules) {
        if (!module || module.layer !== layer || module.disabled || module.retired) continue;
        try { __valueCallback(module, 'animationEvent', event); }
        catch (error) { __log('animationEvent ' + module.index + ': ' + __errorText(error)); }
    }
    __emit('animationEvent', {name, frame, nodeId:Number(__layerId(layer))});
}
function __keyframe(index, a, b, c, d, count) {
    const parts = __keyframes[index];
    const key = parts[parts.length - 1];
    let object = __scene;
    for (let i = 0; i < parts.length - 1; i++) { object = object && __nativeValue(object, parts[i]); }
    if (!object || typeof object !== 'object') return;
    let value = __coerce(count === 1 ? a : [a,b,c,d].slice(0,count), key);
    if (key === 'angles' && value instanceof __Vector) {
        value.x *= 180 / Math.PI;
        value.y *= 180 / Math.PI;
        value.z *= 180 / Math.PI;
    }
    __nativeWrite = true;
    try { __nativeSet(object, key, value); } finally { __nativeWrite = false; }
}
const __cursorHooks = ['cursorEnter','cursorLeave','cursorMove','cursorDown','cursorUp','cursorClick'];
function __pathLayer(path) { return path.startsWith(__objectPrefix) ? __layers[Number(path.split('/')[2])] : null; }
const __shaderConstantPath = /\/constantshadervalues\//;
function __scriptValue(module) {
    const value = __nativeValue(module.object, module.key);
    if (typeof value !== 'string' || !__shaderConstantPath.test(module.path)) return value;
    const parts = value.trim().split(/\s+/);
    const vector = [Vec2, Vec3, Vec4][parts.length - __vectorDimensions.two];
    return vector && parts.every(part => Number.isFinite(Number(part))) ? new vector(value) : value;
}
function __prepareModule(index, path) {
    const [object,key] = __owner(path);
    if (!Object.hasOwn(object, 'getAnimation')) Object.defineProperty(object, 'getAnimation', {
        value: name => __getAnimation(object, path.slice(0, path.lastIndexOf('/')), name)
    });
    const module = {ns:{}, object, key, path, layer:__pathLayer(path), override:__instanceOverridePath.test(path), disabled:false, failed:null, retired:false, index, cursor:false, compiling:true, assets:new Set()};
    __modules[index] = module;
    __current = module;
}
function __register(index, ns) {
    const module = __modules[index];
    module.ns = ns;
    module.cursor = __cursorHooks.some(name => typeof ns[name] === 'function');
    module.compiling = false;
    __current = null;
}
function __disable(index) {
    const module = __modules[index];
    if (module) module.disabled = true;
    if (module) module.compiling = false;
    if (__current === module) __current = null;
}
function __invoke(index, name) {
    const m = __modules[index];
    if (m && !m.disabled) __valueCallback(m, name);
}
function __valueCallback(m, name, ...args) {
    if (m.failed?.has(name) || typeof m.ns[name] !== 'function') return;
    try {
        const value = __scriptValue(m);
        const copy = value instanceof __Vector && !__copiesNativeField(__layerStates.get(m.object), m.object, m.key);
        const result = __call(m, m.ns[name], ...args, copy ? value.copy() : value);
        if (result === undefined) return;
        __nativeSet(m.object, m.key, result);
        if (m.override && m.layer?.instance && Object.hasOwn(m.layer.instance, m.key)) m.layer.instance[m.key] = result;
    } catch (error) {
        (m.failed ??= new Set()).add(name);
        throw error;
    }
}
function __drain() {
    const result = '[' + [...__changes].map(([path, value]) => '[' + JSON.stringify(typeof path === 'symbol' ? path.description : path) + ',' + value + ']').join(',') + ']';
    __changes.clear();
    return result;
}
function __frame(time, dt, x, y, updates) {
    for (const [layer, transforms] of __boneWorlds) {
        transforms.world = null;
        transforms.edits.clear();
        const stepping = updates && transforms.springs;
        if (!__boneWorldReaders && !stepping) continue;
        const world = __boneWorld(layer, transforms);
        if (stepping && world && !__puppetSpringStep(transforms.slot, JSON.stringify(world.m), dt)) throw Error('Puppet spring pose invalid');
    }
    __bonesReady = true;
    __startCamera();
    engine.runtime = time; engine.frametime = dt;
    input.cursorWorldPosition = new Vec3(x * engine.canvasSize.x, (1 - y) * engine.canvasSize.y, 0);
    input.cursorScreenPosition = new Vec2(x * engine.canvasSize.x, y * engine.canvasSize.y);
}
const __scriptProperties = new Map();
const __schemeColorProperty = 'schemecolor';
function __colorScheme(primary) {
    const scheme = primary.copy();
    Object.assign(scheme, {
        primary:primary.copy(), secondary:new Vec3(1), tertiary:new Vec3(1),
        text:new Vec3(0), highContrast:new Vec3(0)
    });
    return scheme;
}
function __setUserProperties(properties) {
    for (const key of Object.keys(engine.userProperties)) delete engine.userProperties[key];
    for (const [name,entry] of Object.entries(properties)) if (entry.value !== undefined) engine.userProperties[name] = entry.type === 'color' ? __coerce(entry.value, 'color') : entry.value;
    const selected = engine.userProperties[__schemeColorProperty];
    engine.colorScheme = __colorScheme(selected === undefined ? new Vec3(1) : __coerce(selected, 'color'));
}
function __updateUserProperties(json) {
    const update = JSON.parse(json);
    for (const [path,value] of update.values) {
        if (__layerStates.get(__pathLayer(path))?.phase === __retired) continue;
        const [object,key] = __owner(path);
        __nativeSet(object, key, value);
    }
    for (const [index,properties] of update.scripts) {
        const target = __scriptProperties.get(index);
        if (!target) continue;
        for (const [key,value] of Object.entries(properties)) if (key in target.values) target.values[key] = target.colors.has(key) ? __coerce(value, 'color') : value;
    }
    __setUserProperties(update.properties);
    const changed = {};
    for (const name of Object.keys(update.changed)) changed[name] = engine.userProperties[name];
    if (Object.keys(changed).length) __applyUserProperties(changed);
}
function __createScriptProperties(defaults = {}, index = -1) {
    const values = {};
    const colors = new Set();
    if (index >= 0) __scriptProperties.set(index, {values,colors});
    const builder = { finish: () => values };
    for (const kind of ['Slider','Checkbox','Combo','Color','Text','Texture']) builder['add'+kind] = o => { values[o.name] = defaults[o.name] ?? o.value ?? o.options?.[0]?.value; if (kind === 'Color') { colors.add(o.name); values[o.name] = __coerce(values[o.name], 'color'); } return builder; };
    return builder;
}
const engine = {
    runtime:0, frametime:0, canvasSize:new Vec2(1920,1080), screenResolution:new Vec2(1920,1080), userProperties:{}, colorScheme:__colorScheme(new Vec3(1)), isRunningInEditor:()=>false,
    AUDIO_RESOLUTION_16:16,AUDIO_RESOLUTION_32:32,AUDIO_RESOLUTION_64:64,
    isDesktopDevice:()=>true,isMobileDevice:()=>false,isWallpaper:()=>true,isScreensaver:()=>false,
    isPortrait:()=>engine.canvasSize.y>engine.canvasSize.x,isLandscape:()=>engine.canvasSize.y<=engine.canvasSize.x,
    registerAsset(file) {
        if (!__current?.compiling) throw new SyntaxError('registerAsset can only be called from global scope.');
        const asset = String(file);
        if (__current.assets.has(asset)) return null;
        __current.assets.add(asset);
        return asset;
    },
    openUserShortcut:__openShortcut,
    get timeOfDay(){const d=new Date();return (d.getHours()*3600+d.getMinutes()*60+d.getSeconds())/86400;},
    registerAudioBuffers: size => {
        if (!__current?.compiling || !__audioResolutions.has(size)) throw Error('Invalid audio buffer registration');
        let b = __audio.get(size);
        if (!b) {
            b = {left:new Float32Array(size),right:new Float32Array(size),average:new Float32Array(size)};
            __audio.set(size,b);
        }
        return {left:b.left.subarray(),right:b.right.subarray(),average:b.average.subarray()};
    },
    setTimeout: (fn, delay=0) => __timer(fn, delay, false), setInterval: (fn, delay=0) => __timer(fn, delay, true),
    clearTimeout: id => __timers.delete(id), clearInterval: id => __timers.delete(id)
};
Object.defineProperty(engine, 'bloomEnabled', {
    enumerable:true,
    get: () => thisScene.bloom,
    set: value => { thisScene.bloom = !!value; }
});
let __shortcutActive = false;
let __shortcutUsed = false;
const __shortcutNameLimit = 256;
const __shortcutEvents = new Set(['cursorDown', 'cursorUp', 'cursorClick']);
function __pointerEvent(name, callback) {
    const previous = __shortcutActive;
    __shortcutActive = __shortcutEvents.has(name);
    try { return callback(); } finally { __shortcutActive = previous; }
}
function __openShortcut(name) {
    if (!__shortcutActive || typeof name !== 'string' || !name.length || name.length > __shortcutNameLimit) return false;
    if (__shortcutUsed) throw Error('Only one shortcut may open per click');
    __shortcutUsed = true;
    __command('shortcut', name, '');
    __emit('userShortcut', {name});
    return true;
}
const __audio = new Map();
const __audioResolutions = new Set([engine.AUDIO_RESOLUTION_16, engine.AUDIO_RESOLUTION_32, engine.AUDIO_RESOLUTION_64]);
function __timer(fn, delay, repeat) {
    if (typeof fn !== 'function' || !Number.isFinite(delay) || __timers.size >= __timerLimits.maximum) throw Error('Invalid or excessive SceneScript timers');
    const id = ++__timerId;
    const seconds = Math.max(__timerLimits.minimumDelay, delay) / __timerLimits.millisecondsPerSecond;
    __timers.set(id, {fn, delay:seconds, due:engine.runtime+seconds, repeat, owner:__current});
    let handle = id;
    return () => {
        const timer = __timers.get(handle);
        handle = null;
        if (!timer) return false;
        for (const [first, pending] of __timers) if (pending.owner === timer.owner) return __timers.delete(first);
        return false;
    };
}
function __tickTimers() {
    for (const [id,timer] of [...__timers]) if (timer.due <= engine.runtime) {
        if (!__timers.has(id) || timer.owner?.retired) continue;
        if (timer.repeat) timer.due = engine.runtime + timer.delay; else __timers.delete(id);
        try { __call(timer.owner, timer.fn); } catch (error) { __log('timer: ' + __errorText(error)); }
    }
}
const input = {cursorWorldPosition:new Vec3(),cursorScreenPosition:new Vec2(),cursorLeftDown:false};
const __layerId = layer => __layerStates.get(layer)?.initial.id;
const __getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
const __LayerLookupMap = Map;
const __lookupHas = Function.prototype.call.bind(Map.prototype.has);
const __lookupGet = Function.prototype.call.bind(Map.prototype.get);
const __lookupSet = Function.prototype.call.bind(Map.prototype.set);
const __lookupString = String;
const __lookupOwn = Object.hasOwn;
let __layerLookup;
function __namedLayer(key) {
    if (typeof key !== 'string') return undefined;
    if (__layerLookup === undefined) {
        let lookup = new __LayerLookupMap();
        for (let index = 0; index < __layerOrder.length; index++) {
            const layer = __layerOrder[index];
            const name = __getOwnPropertyDescriptor(layer, 'name');
            if (!name || !__lookupOwn(name, 'value')) {
                lookup = null;
                break;
            }
            if (typeof name.value === 'string' && !__lookupHas(lookup, name.value)) __lookupSet(lookup, name.value, layer);
            const id = __lookupString(__layerId(layer));
            if (!__lookupHas(lookup, id)) __lookupSet(lookup, id, layer);
        }
        __layerLookup = lookup;
    }
    return __layerLookup === null
        ? __layerOrder.find(layer => layer.name === key || String(__layerId(layer)) === key)
        : __lookupGet(__layerLookup, key);
}
const __layerOf = key => typeof key === 'object' && key !== null ? key : (typeof key === 'number' ? __layerOrder[key] : __namedLayer(key));
function __publicLayer(layer) { return layer && __layerStates.get(layer)?.phase === __alive; }
function __getLayer(key) {
    if (typeof key === 'object' && __layerSlots.has(key)) return __layerStates.get(key).phase === __retired ? undefined : key;
    if (typeof key === 'number') {
        const layer = __layerOrder[key | 0];
        return layer && !__publicLayer(layer) ? null : layer;
    }
    const layer = __namedLayer(key);
    if (!layer && typeof key === 'string' && __retiringLayers) {
        for (const pending of __retiringLayers) if (pending.name === key || String(__layerId(pending)) === key) return null;
    }
    if (typeof key === 'string') return layer && __publicLayer(layer) ? layer : null;
    return undefined;
}
const __maxLayerId = '18446744073709551615';
const __layerIdModulus = BigInt(__maxLayerId) + 1n;
function __getLayerByID(key = undefined) {
    if (arguments.length === 0) return undefined;
    if (typeof key !== 'string') return null;
    const match = /^[\x09-\x0d ]*([+-]?)([0-9]+)/.exec(key);
    const digits = match?.[2].replace(/^0+/, '') || '0';
    const overflow = digits.length > __maxLayerId.length || (digits.length === __maxLayerId.length && digits > __maxLayerId);
    const magnitude = BigInt(overflow ? __maxLayerId : digits);
    const id = overflow ? magnitude : match?.[1] === '-' ? (__layerIdModulus - magnitude) % __layerIdModulus : magnitude;
    if (id === 0n) return null;
    const layer = __byId.get(String(id));
    return __publicLayer(layer) ? layer : null;
}
function __destroyLayer(key) {
    const layer = __getLayer(key), state = __layerStates.get(layer);
    if (!state || state.phase === __retired) return false;
    if (state.phase === __alive) {
        state.phase = __pending;
        __layerCount--;
        __pendingLayers.add(layer);
        const slot = __layerSlots.get(layer);
        __hiddenSlots.push(slot);
        if (__dynamicSlots.has(slot)) __destroyed.push(slot);
    }
    return true;
}
function __discardLayer(slot) {
    __destroyLayer(__layers[slot]);
}
function __destroyModule(module) {
    if (module.retired) return;
    module.retired = true;
    if (!module.disabled) __callback(module, 'destroy');
    module.disabled = true;
    for (const [name, entries] of __sceneListeners) {
        const remaining = entries.filter(entry => entry.owner !== module);
        if (remaining.length) __sceneListeners.set(name, remaining);
        else __sceneListeners.delete(name);
    }
    __pressed.delete(module.index);
    __retiredModules.push(module.index);
}
function __retireLayers() {
    if (!__pendingLayers.size) return;
    const layers = new Set(__pendingLayers);
    __pendingLayers.clear();
    __retiringLayers = layers;
    __settleRetired(layers);
    __retiringLayers = null;
}
function __halt() {
    const layers = new Set([...(__retiringLayers ?? []), ...__pendingLayers]);
    __retiringLayers = null;
    __pendingLayers.clear();
    __current = null;
    __nativeWrite = false;
    __shortcutActive = false;
    for (const module of __modules) if (module) module.disabled = true;
    __settleRetired(layers);
}
function __settleRetired(layers) {
    __layerOrder = __layerOrder.filter(layer => !layers.has(layer));
    __layerLookup = undefined;
    for (const module of __modules) if (module && layers.has(module.layer)) __destroyModule(module);
    for (const [id, timer] of __timers) if (timer.owner && layers.has(timer.owner.layer)) __timers.delete(id);
    for (const layer of layers) {
        const id = __layerId(layer);
        for (const child of __layers) {
            if (String(__nativeValue(child, 'parent')) === String(id)
                && __nativeValue(child, 'attachment') == null) __nativeSet(child, 'parent', null);
        }
        __boneWorlds.delete(layer);
        __videos.delete(__layerSlots.get(layer));
        if (__nativeValue(layer, 'particle') !== undefined) __particleStates.delete(String(__layerId(layer)));
        if (__nativeValue(layer, 'sound') !== undefined) __soundStates.delete(String(__layerId(layer)));
        __layerStates.get(layer).phase = __retired;
    }
}
function __drainLifecycle() {
    if (!__hiddenSlots.length && !__retiredModules.length && !__destroyed.length) return '';
    const result = JSON.stringify([__hiddenSlots, __retiredModules, __destroyed]);
    __hiddenSlots.length = 0;
    __retiredModules.length = 0;
    __destroyed.length = 0;
    return result;
}
const __createdLayerPrototype = Object.create(Object.prototype);
const __createdLayerConstructor = function() {};
Object.defineProperty(__createdLayerConstructor, 'name', {value:''});
Object.defineProperty(__createdLayerPrototype, 'constructor', {value:__createdLayerConstructor});
const __dynamicBindingKeys = new Set(['script', 'scriptproperties', 'user', 'animation']);
const __jsonObjectStart = '{';
const __particleAssetPrefix = 'particles/';
const __modelAssetSuffix = '.mdl';
const __imageContextKeys = ['parent', 'attachment', 'origin', 'scale', 'angles', 'visible', 'parallaxDepth', 'image', 'text', 'particle', 'size'];
const __imageGeneralKeys = ['cameraparallax', 'cameraparallaxamount', 'cameraparallaxmouseinfluence', 'cameraparallaxdelay'];
const __imageProjectionKeys = ['width', 'height', 'auto'];
function __hasDynamicBindings(value) {
    if (!value || typeof value !== 'object') return false;
    if (Array.isArray(value)) return value.some(__hasDynamicBindings);
    return Object.entries(value).some(([key,child]) => __dynamicBindingKeys.has(key) || __hasDynamicBindings(child));
}
function __nativeLayer(layer, slot) {
    const state = __layerStates.get(layer);
    const object = {};
    for (const key of __imageContextKeys) {
        const value = __nativeValue(layer, key);
        if ((__nativeProperty(state, key) || state.hidden?.has(key)) && value !== undefined) object[key] = __plain(value, key);
    }
    object.id = __initial[slot].id;
    return object;
}
function __imageGeneral() {
    const general = __scene.general || {};
    const result = {};
    for (const key of __imageGeneralKeys) if (general[key] !== undefined) result[key] = __plain(general[key], key);
    const projection = general.orthogonalprojection || {};
    result.orthogonalprojection = {};
    for (const key of __imageProjectionKeys) if (projection[key] !== undefined) result.orthogonalprojection[key] = projection[key];
    return result;
}
let __cameraTransforms;
let __cameraDirty = false;
let __cameraZoomed = false;
let __cameraStarted = false;
let __cameraOriented = false;
let __cameraScripted = false;
function __cameraVector(value, fallback) {
    if (typeof value === 'string') value = value.trim().split(/\s+/).map(Number);
    if (Array.isArray(value)) return new Vec3(...value.map(Math.fround));
    if (value && typeof value === 'object') return new Vec3(Math.fround(value.x), Math.fround(value.y), Math.fround(value.z));
    return new Vec3(...fallback);
}
function __startCamera() {
    if (__cameraStarted) return;
    __cameraStarted = true;
    __cameraOriented = __scene.camera !== undefined;
    for (const key of ['eye','center','up']) {
        if (__scene.camera?.[key] !== undefined) __cameraTransforms[key] = __cameraVector(__scene.camera[key], []);
    }
    const projection = __scene.general?.orthogonalprojection;
    if (projection?.auto === true || (projection?.width && projection?.height)) {
        __cameraTransforms.center.x -= __cameraTransforms.eye.x;
        __cameraTransforms.center.y -= __cameraTransforms.eye.y;
        __cameraTransforms.eye.x = __cameraTransforms.eye.y = 0;
    }
    __cameraDirty = true;
}
function __getCameraTransforms() {
    return {eye:__cameraTransforms.eye.copy(), center:__cameraTransforms.center.copy(), up:__cameraTransforms.up.copy(), zoom:__cameraTransforms.zoom};
}
function __setCameraTransforms(value) {
    if (value === null || value === undefined) return false;
    for (const key of ['eye','center','up']) {
        if (value[key] !== undefined) {
            __cameraTransforms[key] = __cameraVector(value[key], __vectorComponents.map(axis => __cameraTransforms[key][axis]));
            if (__cameraStarted) {
                __cameraOriented = true;
                __cameraScripted = true;
            }
        }
    }
    if (value.zoom !== undefined) {
        __cameraTransforms.zoom = Math.fround(value.zoom);
        __cameraZoomed = true;
    }
    __cameraDirty = true;
    return true;
}
function __drainCamera() {
    if (!__cameraDirty) return '';
    __cameraDirty = false;
    const zoom = __cameraZoomed && __cameraTransforms.zoom > 0 && Number.isFinite(__cameraTransforms.zoom) ? __cameraTransforms.zoom : null;
    __cameraZoomed = false;
    const axes = key => ['x','y','z'].map(axis => __cameraTransforms[key][axis]);
    const pose = {eye:axes('eye'), center:axes('center'), up:axes('up')};
    return JSON.stringify({initialized:__cameraStarted, oriented:__cameraOriented, scripted:__cameraScripted, pose:Object.values(pose).flat().every(Number.isFinite) ? pose : null, zoom});
}
function IModelData() {}
Object.assign(IModelData, {POSITION:'position', NORMAL:'normal', UV:'uv'});
Object.freeze(IModelData);
const __models = new WeakMap();
let __modelSerial = 0;
function __createModelData(config) {
    if (config === null || config === undefined) return null;
    if (config.shapes === undefined) throw new SyntaxError('Vertex buffer missing.');
    if (!Array.isArray(config.shapes) || !config.shapes.length) throw new SyntaxError('Shapes missing.');
    const shapes = config.shapes.map(shape => {
        if (!shape.vertexBuffer) throw new SyntaxError('Vertex buffer missing.');
        return {vertexBuffer:Array.from(shape.vertexBuffer), vertexFormat:shape.vertexFormat, material:shape.material};
    });
    const id = ++__modelSerial;
    const error = __createModel(id, JSON.stringify({shapes}));
    if (error) throw new SyntaxError(error);
    const handle = {};
    __models.set(handle, id);
    return handle;
}
function __destroyModelData(handle) {
    const id = __models.get(handle);
    if (id !== undefined) __destroyModel(id);
}
function __validationObjects(candidate, slot, general) {
    if (general.orthogonalprojection.auto) return __layers.map(__nativeLayer);
    const objects = new Array(Math.max(__layers.length, slot + 1)).fill(null);
    for (let parent = candidate.parent, depth = 0; parent !== undefined && depth < __layers.length; depth++) {
        const index = __initial.findIndex(object => object && String(object.id) === String(parent));
        if (index < 0 || !__layers[index]) break;
        objects[index] = __nativeLayer(__layers[index], index);
        parent = objects[index].parent;
    }
    return objects;
}
function __createLayer(config) {
    if (typeof config === 'string') {
        config = config.trim();
        config = config.startsWith(__jsonObjectStart) ? JSON.parse(config)
            : config.startsWith(__particleAssetPrefix) ? {particle:config}
            : config.toLowerCase().endsWith(__modelAssetSuffix) ? {model:config} : {image:config};
    }
    if (!config || typeof config !== 'object' || Array.isArray(config)) throw Error('Invalid SceneScript layer');
    if (__hasDynamicBindings(config)) throw Error('SceneScript dynamic script bindings are unsupported');
    const slot = __freeSlots.values().next().value ?? __layers.length;
    if (slot >= __maxSceneObjects) throw Error('SceneScript object limit exceeded');
    const candidate = Object.fromEntries(Object.entries(config).map(([key,value]) => [key,__plain(value,key)]));
    if (config.model !== undefined && typeof config.model !== 'string') {
        const id = __models.get(config.model);
        if (id === undefined) throw Error('Invalid model handle');
        candidate.model = id;
    }
    do { candidate.id = '@dynamic-' + __nextLayerId++; } while (__byId.has(candidate.id));
    const general = __imageGeneral();
    const scene = {general, objects:__validationObjects(candidate, slot, general)};
    scene.objects[slot] = JSON.parse(JSON.stringify(candidate));
    const result = JSON.parse(__validateImage(JSON.stringify(scene), slot));
    if (Object.hasOwn(result, 'Err')) throw Error(result.Err);
    __freeSlots.delete(slot);
    __rememberLayer(slot, scene.objects[slot]);
    Object.setPrototypeOf(candidate, __createdLayerPrototype);
    __layers[slot] = candidate;
    __changes.delete(__objectPrefix + slot);
    __changes.delete('/objects/length');
    const layer = __layers[slot];
    __layerSlots.set(layer, slot);
    __byId.set(String(__layerId(layer)), layer);
    __layerOrder.push(layer);
    __layerLookup = undefined;
    __nativeOrder.push(layer);
    __layerCount++;
    __dynamicSlots.add(slot);
    if (result.Ok.length) __setSprite(slot, result.Ok);
    __installLayer(layer, slot);
    __created.push([slot, scene.objects[slot]]);
    return layer;
}
function __releaseLayers(json) {
    const released = [];
    for (const slot of JSON.parse(json)) {
        const layer = __layers[slot];
        if (!__dynamicSlots.has(slot) || __layerStates.get(layer)?.phase !== __retired) continue;
        const id = __layerId(layer);
        if (__layers.some(child => !__freeSlots.has(__layerSlots.get(child))
            && String(__nativeValue(child, 'parent')) === String(id))) continue;
        const inert = {id, visible:false};
        __rememberLayer(slot, inert);
        const writing = __nativeWrite;
        __nativeWrite = true;
        try { __layers[slot] = inert; } finally { __nativeWrite = writing; }
        __nativeOrder = __nativeOrder.filter(value => value !== layer);
        __byId.delete(String(id));
        __dynamicSlots.delete(slot);
        __sprites.delete(slot);
        for (const definition of __puppetLayers.get(slot) ?? []) {
            if (!definition) continue;
            __puppetHandles.delete(definition.clock);
            __puppetSingle.delete(definition.clock);
            __puppetCallbacks.delete(definition.clock);
        }
        __puppetLayers.delete(slot);
        __layerLookup = undefined;
        __freeSlots.add(slot);
        released.push(slot);
    }
    return JSON.stringify(released);
}
function __drainCreated() { const result = JSON.stringify(__created); __created.length = 0; return result; }
const thisScene = {
    on: __listen,
    off: __unlisten,
    getAnimation(name = undefined) {
        return name === undefined ? null : __getAnimation(null, '', name) ?? null;
    },
    getLayer: __getLayer,
    getLayerByID: __getLayerByID,
    getLayerCount: () => __layerCount,
    enumerateLayers: () => __layerOrder.filter(__publicLayer),
    getLayerIndex: layer => __layerOrder.indexOf(__layerOf(layer)),
    getLayerByIndex: index => __layerOrder[index],
    getInitialLayerConfig: layer => {
        const slot = __layerSlots.get(__layerOf(layer));
        if (slot === undefined) return null;
        const config = __authoredLayer(slot) ?? {};
        delete config.id;
        return config;
    },
    destroyLayer: __destroyLayer,
    sortLayer: (key, index) => {
        const layer = thisScene.getLayer(key);
        if (!layer || typeof index !== 'number') return false;
        const from = __layerOrder.indexOf(layer);
        if (from < 0) return false;
        const to = Math.min(index >>> 0, __layerOrder.length - 1);
        if (from !== to) {
            __layerOrder.splice(from, 1);
            __layerOrder.splice(to, 0, layer);
            __layerLookup = undefined;
            const members = new Set(__layerOrder);
            let position = 0;
            __nativeOrder = __nativeOrder.map(entry => members.has(entry) ? __layerOrder[position++] : entry);
            __sortPending = true;
        }
        return true;
    },
    getCameraTransforms: __getCameraTransforms,
    setCameraTransforms: __setCameraTransforms,
    createModelData: __createModelData,
    destroyModelData: __destroyModelData,
    createLayer: __createLayer
};
const __generalDefaults = {bloom:false, bloomstrength:2, bloomthreshold:0.65, bloomtint:'1 1 1', clearenabled:true, clearcolor:'0 0 0', ambientcolor:'0.3 0.3 0.3', skylightcolor:'0.3 0.3 0.3',
    fov:50, nearz:0.01, farz:10000, zoom:1, camerafade:false, camerashake:false, camerashakespeed:1, camerashakeamplitude:1, camerashakeroughness:1,
    cameraparallax:false, cameraparallaxamount:0.5, cameraparallaxdelay:0.1, cameraparallaxmouseinfluence:0.5};
for (const [key, fallback] of Object.entries(__generalDefaults)) Object.defineProperty(thisScene, key, {
    enumerable: true,
    get() {
        const general = globalThis.__scene?.general;
        let value = general ? general[key] : undefined;
        if (value && typeof value === 'object' && !(value instanceof __Vector) && 'value' in value) value = __coerce(value.value, key);
        return value === undefined || value === null ? __coerce(fallback, key) : value;
    },
    set(value) { const general = globalThis.__scene?.general; if (general) general[key] = value; }
});
let __bloomEnabled = false;
Object.defineProperty(thisScene, 'bloomEnabled', {
    enumerable:true,
    get: () => __bloomEnabled,
    set: () => {}
});
function __destroyAll() {
    __emit('destroy');
    __retireLayers();
    for (const module of __modules) if (module) __destroyModule(module);
    __timers.clear();
    __sceneListeners.clear();
}
let shared = {};
const console = {log: (...args) => __log(args.map(String).join(' ')), warn: (...args) => __log(args.map(String).join(' ')), error: (...args) => __log(args.map(String).join(' '))};
Object.defineProperty(globalThis, 'shared', {get: () => shared, set: value => { shared = value; }});
Object.assign(globalThis, {engine,input,thisScene,scene:thisScene,console,createScriptProperties:__createScriptProperties});

Object.assign(globalThis, {MediaPlaybackEvent:{PLAYBACK_PLAYING:1,PLAYBACK_PAUSED:2,PLAYBACK_STOPPED:0}});
function __audioUpdate(count,left,right) { const b=__audio.get(count); if(!b)return; b.left.set(left); b.right.set(right); for(let i=0;i<count;i++)b.average[i]=(left[i]+right[i])*0.5; }

let __lastPointer = [-1,-1];
let __lastButtons = [false,false,false];
let __scenePressed = false;
let __hover = new Set();
const __pressed = new Map();
function __localPosition(layer,world) {
    const chain=[], seen=new Set();
    for(let current=layer;current&&!seen.has(current)&&chain.length<64;current=current.getParent()) {seen.add(current);chain.push(current);}
    let p=world.copy();
    for(const current of chain.reverse()) {
        p=p.subtract(current.origin);
        const angle=-current.angles.z*Math.PI/180, c=Math.cos(angle), s=Math.sin(angle);
        p=new Vec3(p.x*c-p.y*s,p.x*s+p.y*c,p.z).divide(current.scale);
    }
    return new Vec3(p.x+(layer.size?.x||0)/2,(layer.size?.y||0)/2-p.y,p.z);
}
function __pointer(x,y,buttons,hits) {
    const moved=x!==__lastPointer[0] || y!==__lastPointer[1];
    if(!moved && buttons.every((v,i)=>v===__lastButtons[i])) return;
    input.cursorWorldPosition=new Vec3(x*engine.canvasSize.x,(1-y)*engine.canvasSize.y,0);
    input.cursorScreenPosition=new Vec2(x*engine.canvasSize.x,y*engine.canvasSize.y);
    input.cursorLeftDown=!!buttons[0];
    if (buttons[0] && !__lastButtons[0]) __shortcutUsed = false;
    const hovered=new Set(hits), accepted=new Set(), active=new Map();
    for(let i=0;i<__modules.length;i++) {
        const m=__modules[i]; if(!m || m.disabled || m.retired || !m.layer || !m.cursor) continue;
        const id=String(__layerId(m.layer)), inside=hovered.has(id) && m.layer.solid !== false, was=__hover.has(id);
        if(inside) accepted.add(id);
        const worldPosition=input.cursorWorldPosition.copy();
        const event={worldPosition, localPosition:__localPosition(m.layer,worldPosition), cursorWorldPosition:worldPosition, cursorScreenPosition:input.cursorScreenPosition.copy()};
        if(!active.has(m.layer)) active.set(m.layer,[]);
        active.get(m.layer).push({m,i,inside,was,event});
    }
    const call=(entry,name)=>{
        const {m,event}=entry;
        if(m.disabled || m.retired) return;
        if(entry.inside) __pointerEvent(name, () => __callback(m,name,event));
        else __callback(m,name,event);
    };
    for(let index=__layerOrder.length-1;index>=0;index--) {
        const entries=active.get(__layerOrder[index]);
        if(!entries) continue;
        for(const entry of entries) {
            const {m,i,inside}=entry;
            if(m.disabled || m.retired) continue;
            if(inside&&!entry.was) call(entry,'cursorEnter');
            if(!inside&&entry.was) call(entry,'cursorLeave');
            if(m.disabled || m.retired) continue;
            if((inside||__pressed.has(i))&&moved) call(entry,'cursorMove');
            if(inside&&buttons[0]&&!__lastButtons[0]) {__pressed.set(i,true);call(entry,'cursorDown');}
            if(!buttons[0]&&__lastButtons[0]) {if(inside||__pressed.has(i))call(entry,'cursorUp');if(inside&&__pressed.has(i))call(entry,'cursorClick');}
        }
    }
    const event = {worldPosition:input.cursorWorldPosition.copy(), cursorWorldPosition:input.cursorWorldPosition.copy(), cursorScreenPosition:input.cursorScreenPosition.copy()};
    const inside = x >= 0 && x <= 1 && y >= 0 && y <= 1;
    const wasInside = __lastPointer.every(coordinate => coordinate >= 0 && coordinate <= 1);
    if (inside && !wasInside) __emit('cursorEnter', event);
    if (!inside && wasInside) __emit('cursorLeave', event);
    if (moved) __emit('cursorMove', event);
    if (buttons[0] && !__lastButtons[0]) {
        __scenePressed = inside;
        if (inside) __pointerEvent('cursorDown', () => __emit('cursorDown', event));
    }
    if (!buttons[0] && __lastButtons[0]) {
        if (inside && __scenePressed) {
            __pointerEvent('cursorUp', () => __emit('cursorUp', event));
            __pointerEvent('cursorClick', () => __emit('cursorClick', event));
        } else if (__scenePressed) {
            __emit('cursorUp', event);
        }
        __scenePressed = false;
    }
    if(!buttons[0]&&__lastButtons[0]) __pressed.clear();
    __lastPointer=[x,y];__lastButtons=buttons;__hover=accepted;
}

const __mediaState = new Map();
function __media(json) {
    const events=JSON.parse(json);
    for(const [name,event] of Object.entries(events)) {
        const signature=JSON.stringify(event);
        if(__mediaState.get(name)===signature)continue;
        __mediaState.set(name,signature);
        for(const key of ['primaryColor','secondaryColor','tertiaryColor','textColor','highContrastColor']) if(Array.isArray(event[key]))event[key]=new Vec3(...event[key]);
        for(const m of __modules) {
            if(m && !m.disabled) __callback(m, name, event);
        }
        __emit(name, event);
    }
}
const __mediaHooks = ['mediaPlaybackChanged','mediaPropertiesChanged','mediaThumbnailChanged','mediaStatusChanged','mediaTimelineChanged'];
function __needsMedia() { return __mediaHooks.some(__hasListeners) || __modules.some(m=>m && !m.disabled && !m.retired && __mediaHooks.some(name=>typeof m.ns[name]==='function' && !m.failed?.has(name))); }
