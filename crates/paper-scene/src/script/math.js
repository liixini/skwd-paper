export const deg2rad = __angleUnits.radiansPerDegree;
export const rad2deg = __angleUnits.degreesPerRadian;
export const mix = (a, b, t) => a + (b - a) * t;
export const clamp = (x, a, b) => Math.min(b, Math.max(a, x));
export const smoothStep = (a, b, x) => {
    const t = clamp((x - a) / (b - a), 0, 1);
    return t * t * (3 - 2 * t);
};
export const smoothstep = smoothStep;
export const random = (a = 0, b = 1) => mix(a, b, Math.random());
export const radians = x => x * deg2rad;
export const degrees = x => x * rad2deg;
