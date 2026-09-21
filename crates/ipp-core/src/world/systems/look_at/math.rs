/// Local -Z aim with local +Y aligned to the pulled-back World-up plane.
/// Coincident targets withdraw the contribution. Poles select the least aligned
/// cardinal axis with X/Y/Z tie order, so there is no history-dependent roll.
pub(super) fn aim(direction: [f64; 3], up: [f64; 3]) -> Option<[f32; 4]> {
    let z = normalize(direction)?.map(|v| -v);
    let up = normalize(up)?;
    let mut x = cross(up, z);
    if dot(x, x) < 1e-12 {
        let axis = (0..3)
            .min_by(|&a, &b| z[a].abs().total_cmp(&z[b].abs()))
            .unwrap();
        let mut fallback = [0.0; 3];
        fallback[axis] = 1.0;
        x = cross(fallback, z);
    }
    let x = normalize(x)?;
    let y = cross(z, x);
    let m = [x, y, z];
    let trace = x[0] + y[1] + z[2];
    let mut q = [0.0; 4];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        q = [
            (y[2] - z[1]) / s,
            (z[0] - x[2]) / s,
            (x[1] - y[0]) / s,
            s * 0.25,
        ];
    } else {
        let i = (0..3).max_by(|&a, &b| m[a][a].total_cmp(&m[b][b])).unwrap();
        let j = (i + 1) % 3;
        let k = (i + 2) % 3;
        let s = (1.0 + m[i][i] - m[j][j] - m[k][k]).max(0.0).sqrt() * 2.0;
        q[i] = s * 0.25;
        q[j] = (m[j][i] + m[i][j]) / s;
        q[k] = (m[k][i] + m[i][k]) / s;
        q[3] = (m[j][k] - m[k][j]) / s;
    }
    let length = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    let q = q.map(|v| (v / length) as f32);
    q.iter().all(|v| v.is_finite()).then_some(q)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn normalize(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(v, v).sqrt();
    (length.is_finite() && length > 0.0).then(|| v.map(|v| v / length))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
