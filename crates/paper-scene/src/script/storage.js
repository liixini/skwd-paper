let __storage = {screen:{},global:{}};
let __storageDirty = false;
function __storageClone(v) {
    if (v && v.__vector) return new ({Vec2,Vec3,Vec4}[v.__vector])(...v.values);
    if (Array.isArray(v)) return v.map(__storageClone);
    if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k,x])=>[k,__storageClone(x)]));
    return v;
}
function __storageEncode(v) {
    if (v instanceof Vec2) return {__vector:v.constructor.name,values:['x','y','z','w'].filter(k=>k in v).map(k=>v[k])};
    if (Array.isArray(v)) return v.map(__storageEncode);
    if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k,x])=>[k,__storageEncode(x)]));
    return v;
}
function __storageLocation(location) {
    if (location !== 'screen' && location !== 'global') throw Error('Invalid storage location');
    return __storage[location];
}
const localStorage = {
    LOCATION_GLOBAL:'global',LOCATION_SCREEN:'screen',
    get(key, location='screen') { return __storageClone(Object.hasOwn(__storageLocation(location),String(key)) ? __storageLocation(location)[String(key)] : undefined); },
    set(key,value,location='screen') {
        const data=__storageLocation(location), next={...data,[String(key)]:__storageEncode(value)};
        const json=JSON.stringify({...__storage,[location]:next});
        if (unescape(encodeURIComponent(json)).length > 100*1024) throw Error('Wallpaper storage exceeds 100 KB');
        __storage=JSON.parse(json);__storageDirty=true;
    },
    delete(key,location='screen') { const data=__storageLocation(location);if(!Object.hasOwn(data,String(key)))return false;delete data[String(key)];__storageDirty=true;return true; },
    clear(location='screen') { __storageLocation(location);__storage[location]={};__storageDirty=true; }
};
function __storageLoad(json) { __storage=JSON.parse(json);__storageDirty=false; }
function __storageDrain() { if(!__storageDirty)return '';__storageDirty=false;return JSON.stringify(__storage); }
Object.assign(globalThis,{localStorage});
