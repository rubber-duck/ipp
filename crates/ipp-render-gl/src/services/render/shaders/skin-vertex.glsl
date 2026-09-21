layout(location = 5) in vec4 a_joints;
layout(location = 6) in vec4 a_joint_weights;
uniform mat4 u_joints[32];

mat4 skin_matrix() {
    ivec4 joints = ivec4(a_joints);
    return a_joint_weights.x * u_joints[joints.x]
              + a_joint_weights.y * u_joints[joints.y]
              + a_joint_weights.z * u_joints[joints.z]
              + a_joint_weights.w * u_joints[joints.w];
}

vec4 skinned_position(vec3 position) {
    return skin_matrix() * vec4(position, 1.0);
}

vec3 skinned_normal(vec3 normal) {
    mat3 linear = mat3(skin_matrix());
    // Cofactors give inverse-transpose direction without dividing by a tiny determinant.
    mat3 cofactors = mat3(cross(linear[1], linear[2]), cross(linear[2], linear[0]), cross(linear[0], linear[1]));
    vec3 transformed = cofactors * normal;
    if (dot(linear[0], cofactors[0]) < 0.0) transformed = -transformed;
    return any(notEqual(transformed, vec3(0.0))) ? transformed : normal;
}
