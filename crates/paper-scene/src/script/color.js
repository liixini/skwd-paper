export function hsv2rgb(hsv) {
    const h=((hsv.x%1)+1)%1;
    const channel=n=>hsv.z*(1-hsv.y*Math.max(0,Math.min(1,Math.min((n+h*6)%6,4-(n+h*6)%6))));
    return new Vec3(channel(5),channel(3),channel(1));
}
export function rgb2hsv(rgb) {
    const max=Math.max(rgb.x,rgb.y,rgb.z),min=Math.min(rgb.x,rgb.y,rgb.z),d=max-min;
    let h=0;
    if(d) h=max===rgb.x?((rgb.y-rgb.z)/d+6)%6:max===rgb.y?(rgb.z-rgb.x)/d+2:(rgb.x-rgb.y)/d+4;
    return new Vec3(h/6,max===0?0:d/max,max);
}
export const normalizeColor=rgb=>new Vec3(rgb).divide(255);
export const expandColor=rgb=>new Vec3(rgb).multiply(255);
