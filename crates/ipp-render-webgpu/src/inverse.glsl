// Naga's Metal writer has no GLSL matrix inverse intrinsic. Form the
// adjugate from column cofactors for authored GLSL and portable stages.
mat2 ippMatrixInverse(mat2 m) {
    return mat2(m[1].y, -m[0].y, -m[1].x, m[0].x) * (1.0 / determinant(m));
}

mat3 ippMatrixInverse(mat3 m) {
    vec3 x = cross(m[1], m[2]);
    mat3 cofactors = mat3(x, cross(m[2], m[0]), cross(m[0], m[1]));
    return transpose(cofactors) * (1.0 / dot(m[0], x));
}

vec4 ippColumnCofactors(vec4 a, vec4 b, vec4 c) {
    return vec4(dot(a.yzw, cross(b.yzw, c.yzw)), -dot(a.xzw, cross(b.xzw, c.xzw)),
                dot(a.xyw, cross(b.xyw, c.xyw)), -dot(a.xyz, cross(b.xyz, c.xyz)));
}

mat4 ippMatrixInverse(mat4 m) {
    vec4 x = ippColumnCofactors(m[1], m[2], m[3]);
    mat4 cofactors = mat4(x, -ippColumnCofactors(m[0], m[2], m[3]),
                          ippColumnCofactors(m[0], m[1], m[3]), -ippColumnCofactors(m[0], m[1], m[2]));
    return transpose(cofactors) * (1.0 / dot(m[0], x));
}

#define inverse ippMatrixInverse
