    vec3 target = a_pose_normal / max(max(abs(a_pose_normal.x), abs(a_pose_normal.y)), abs(a_pose_normal.z));
    target *= inversesqrt(dot(target, target));
    vec3 blended = mix(authored, target, u_pose_weight);
    // Opposite endpoint normals can cancel. Retain a defined endpoint direction.
    float length_squared = dot(blended, blended);
    authored = length_squared > 1e-12 ? blended * inversesqrt(length_squared) : authored;
