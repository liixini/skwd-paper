import { deg2rad, rad2deg } from 'WEMath';

export const lerp = (a, b, t) => a.add(b.subtract(a).multiply(t));
export const angleVector2 = angle => new Vec2(Math.cos(angle * deg2rad), Math.sin(angle * deg2rad));
export const vectorAngle2 = direction => Math.atan2(direction.y, direction.x) * rad2deg;
