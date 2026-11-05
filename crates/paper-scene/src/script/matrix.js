const __matrix = (() => {
    const dimensions = {plane: 3, space: 4};
    const separator = " ";
    const at = (matrix, row, column) => matrix.m[column * Math.sqrt(matrix.m.length) + row];
    const make = (Type, elements) => { const matrix = Object.create(Type.prototype); matrix.m = elements; return matrix; };
    const identity = dimension => Array.from({length: dimension * dimension}, (_, slot) => slot % (dimension + 1) === 0 ? 1 : 0);
    const column = (matrix, index, dimension) => __vectorComponents.slice(0, dimension).map((_, row) => matrix.m[index * (matrix instanceof Mat3 ? dimensions.plane : dimensions.space) + row] ?? 0);
    const format = matrix => Array.from({length: matrix instanceof Mat3 ? dimensions.plane ** 2 : dimensions.space ** 2}, (_, slot) => String(matrix.m[slot])).join(separator);
    const minor = (elements, dimension, row, column) => elements.filter((_, slot) => Math.floor(slot / dimension) !== column && slot % dimension !== row);
    const determinant = (elements, dimension) => {
        if (dimension === __vectorDimensions.two) return elements[0] * elements[dimension + 1] - elements[1] * elements[dimension];
        if (dimension === dimensions.plane) {
            const [xx, yx, zx, xy, yy, zy, xz, yz, zz] = elements;
            return xx * (yy * zz - zy * yz) - xy * (yx * zz - zx * yz) + xz * (yx * zy - zx * yy);
        }
        let sum = 0;
        for (let column = 0; column < dimension; column++) {
            sum += (column % 2 ? -1 : 1) * elements[column * dimension] * determinant(minor(elements, dimension, 0, column), dimension - 1);
        }
        return sum;
    };
    const transform = (matrix, vector, homogeneous) => {
        const dimension = Math.sqrt(matrix.m.length);
        return Array.from({length: dimension}, (_, row) => {
            let sum = 0;
            for (let column = 0; column < dimension; column++) sum += at(matrix, row, column) * (column === dimension - 1 && homogeneous !== undefined ? homogeneous : vector[__vectorComponents[column]]);
            return sum;
        });
    };
    const translation = (matrix, position) => {
        const dimension = Math.sqrt(matrix.m.length);
        if (position === undefined) return new (dimension === dimensions.plane ? Vec2 : Vec3)(...column(matrix, dimension - 1, dimension - 1));
        for (let row = 0; row < dimension - 1; row++) matrix.m[(dimension - 1) * dimension + row] = position[__vectorComponents[row]] ?? 0;
        return matrix;
    };
    const scale = (Type, amount) => {
        const matrix = new Type();
        const dimension = Math.sqrt(matrix.m.length);
        for (let row = 0; row < dimension - 1; row++) matrix.m[row * (dimension + 1)] = typeof amount === 'number' ? amount : amount[__vectorComponents[row]];
        return matrix;
    };
    const basis = (Type, vectors) => {
        const matrix = new Type();
        const dimension = Math.sqrt(matrix.m.length);
        vectors.forEach((vector, column) => {
            for (let row = 0; row < dimension - 1; row++) matrix.m[column * dimension + row] = vector[__vectorComponents[row]];
        });
        return matrix;
    };
    const methods = {
        copy() { return make(this.constructor, this.m.slice()); },
        equals(other) { return this.m.every((value, slot) => Math.abs(value - other.m[slot]) < __equalityTolerance); },
        toString() { return format(this); },
        toConfigString() { return format(this); },
        add(other) { return make(this.constructor, this.m.map((value, slot) => value + other.m[slot])); },
        subtract(other) { return make(this.constructor, this.m.map((value, slot) => value - other.m[slot])); },
        multiply(other) {
            if (typeof other === 'number') return make(this.constructor, this.m.map(value => value * other));
            const dimension = Math.sqrt(this.m.length);
            if (other instanceof __Vector) return new (dimension === dimensions.plane ? Vec3 : Vec4)(...transform(this, other));
            return make(this.constructor, this.m.map((_, slot) => {
                const row = slot % dimension, column = Math.floor(slot / dimension);
                let sum = 0;
                for (let inner = 0; inner < dimension; inner++) sum += at(this, row, inner) * at(other, inner, column);
                return sum;
            }));
        },
        translation(position) { return translation(this, position); },
        translate(position) { return this.multiply(this.constructor.fromTranslation(position)); },
        scale(amount) { return this.multiply(this.constructor.fromScale(amount)); },
        transpose() {
            const dimension = Math.sqrt(this.m.length);
            return make(this.constructor, this.m.map((_, slot) => at(this, Math.floor(slot / dimension), slot % dimension)));
        },
        determinant() { return determinant(this.m, Math.sqrt(this.m.length)); },
        inverse() {
            const dimension = Math.sqrt(this.m.length), det = this.determinant();
            return make(this.constructor, this.m.map((_, slot) => {
                const row = Math.floor(slot / dimension), column = slot % dimension;
                return ((row + column) % 2 ? -1 : 1) * determinant(minor(this.m, dimension, row, column), dimension - 1) / det;
            }));
        }
    };
    const install = (Type, order) => {
        const own = new Map(Object.getOwnPropertyNames(Type.prototype).map(name => [name, Object.getOwnPropertyDescriptor(Type.prototype, name)]));
        for (const name of own.keys()) if (name !== 'constructor') delete Type.prototype[name];
        for (const name of order) Object.defineProperty(Type.prototype, name, own.get(name) ?? {value: methods[name], writable: true, configurable: true});
    };
    return {dimensions, at, make, identity, column, transform, scale, basis, install};
})();
class Mat3 {
    constructor() { this.m = __matrix.identity(__matrix.dimensions.plane); }
    static identity() { return new Mat3(); }
    static fromTranslation(position) { return new Mat3().translation(position); }
    static fromScale(amount) { return __matrix.scale(Mat3, amount); }
    static fromRotation(angle) {
        const radians = angle * __angleUnits.radiansPerDegree, cosine = Math.cos(radians), sine = Math.sin(radians);
        return __matrix.basis(Mat3, [new Vec2(cosine, sine), new Vec2(-sine, cosine)]);
    }
    static fromBasis(right, up) { return __matrix.basis(Mat3, [right, up]); }
    static fromMat4(matrix) {
        const dimension = __matrix.dimensions.plane;
        return __matrix.make(Mat3, Array.from({length: dimension * dimension}, (_, slot) => __matrix.at(matrix, slot % dimension, Math.floor(slot / dimension))));
    }
    static compose(translation, rotation, scale) { return Mat3.fromTranslation(translation).multiply(Mat3.fromRotation(rotation)).multiply(Mat3.fromScale(scale)); }
    right() { return new Vec3(...__matrix.column(this, 0, __matrix.dimensions.plane)); }
    up() { return new Vec3(...__matrix.column(this, 1, __matrix.dimensions.plane)); }
    forward() { return new Vec3(...__matrix.column(this, 2, __matrix.dimensions.plane)); }
    rotate(angle) { return this.multiply(Mat3.fromRotation(angle)); }
    angle() { return __matrixAtan2(__matrix.at(this, 0, 0), -__matrix.at(this, 1, 0)) * __angleUnits.degreesPerRadian; }
    transformPoint(point) { return new Vec2(...__matrix.transform(this, point, 1)); }
    transformDirection(direction) { return new Vec2(...__matrix.transform(this, direction, 0)); }
    decompose() {
        const right = new Vec2(...__matrix.column(this, 0, __vectorDimensions.two));
        const up = new Vec2(...__matrix.column(this, 1, __vectorDimensions.two));
        return {translation: this.translation(), rotation: right.angle(), scale: new Vec2(right.length(), up.length())};
    }
}
class Mat4 {
    constructor() { this.m = __matrix.identity(__matrix.dimensions.space); }
    static identity() { return new Mat4(); }
    static fromTranslation(position) { return new Mat4().translation(position); }
    static fromScale(amount) { return __matrix.scale(Mat4, amount); }
    static fromRotation(angle, axis) {
        const normal = axis.normalize(), radians = angle * __angleUnits.radiansPerDegree;
        const cosine = Math.cos(radians), sine = Math.sin(radians), complement = 1 - cosine;
        const {x, y, z} = normal;
        return Mat4.fromBasis(
            new Vec3(cosine + x * x * complement, y * x * complement + z * sine, z * x * complement - y * sine),
            new Vec3(x * y * complement - z * sine, cosine + y * y * complement, z * y * complement + x * sine),
            new Vec3(x * z * complement + y * sine, y * z * complement - x * sine, cosine + z * z * complement)
        );
    }
    static fromEuler(x, y, z) {
        const angles = x instanceof Vec3 ? x : new Vec3(x, y, z);
        return Mat4.fromRotation(angles.z, new Vec3(0, 0, 1)).multiply(Mat4.fromRotation(angles.y, new Vec3(0, 1, 0))).multiply(Mat4.fromRotation(angles.x, new Vec3(1, 0, 0)));
    }
    static fromBasis(right, up, forward) { return __matrix.basis(Mat4, [right, up, forward]); }
    static compose(translation, rotation, scale) { return Mat4.fromTranslation(translation).multiply(Mat4.fromEuler(rotation)).multiply(Mat4.fromScale(scale)); }
    static lookAt(eye, center, up) {
        const forward = eye.subtract(center).normalize(), right = up.cross(forward).normalize(), vertical = forward.cross(right);
        return Mat4.fromBasis(right, vertical, forward).transpose().multiply(Mat4.fromTranslation(eye.negate()));
    }
    right() { return new Vec3(...__matrix.column(this, 0, __vectorDimensions.three)); }
    up() { return new Vec3(...__matrix.column(this, 1, __vectorDimensions.three)); }
    forward() { return new Vec3(...__matrix.column(this, 2, __vectorDimensions.three)); }
    rotate(angle, axis) { return this.multiply(Mat4.fromRotation(angle, axis)); }
    transformDirection(direction) { return new Vec3(...__matrix.transform(this, direction, 0)); }
    transformPoint(point) { return new Vec3(...__matrix.transform(this, point, 1)); }
    extractEuler() {
        const right = this.right(), up = this.up(), forward = this.forward();
        return new Vec3(
            __matrixAtan2(right.y * forward.x - right.x * forward.y, right.x * up.y - right.y * up.x),
            __matrixAtan2(-right.z, Math.hypot(up.z, forward.z)),
            __matrixAtan2(right.y, right.x)
        ).multiply(__angleUnits.degreesPerRadian);
    }
    normalMatrix() { return Mat3.fromMat4(this).inverse().transpose(); }
    decompose() {
        const right = this.right(), up = this.up(), forward = this.forward();
        const scale = new Vec3(right.length(), up.length(), forward.length());
        const rotation = new Vec3(
            __matrixAtan2(up.z / scale.y, forward.z / scale.z),
            __matrixAtan2(-right.z / scale.x, Math.hypot(right.x, right.y) / scale.x),
            __matrixAtan2(right.y, right.x)
        ).multiply(__angleUnits.degreesPerRadian);
        return {translation: this.translation(), rotation, scale};
    }
}
__matrix.install(Mat3, ['right', 'up', 'forward', 'translation', 'angle', 'add', 'subtract', 'multiply', 'translate', 'rotate', 'scale', 'transformPoint', 'transformDirection', 'transpose', 'determinant', 'inverse', 'decompose', 'copy', 'equals', 'toString', 'toConfigString']);
__matrix.install(Mat4, ['translation', 'right', 'up', 'forward', 'add', 'subtract', 'multiply', 'translate', 'rotate', 'scale', 'transformPoint', 'transformDirection', 'transpose', 'inverse', 'determinant', 'extractEuler', 'normalMatrix', 'decompose', 'copy', 'equals', 'toString', 'toConfigString']);
Object.assign(globalThis, {Mat3, Mat4});
