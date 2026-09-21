use super::ParticleEmitter;

/// Logical evaluated particle; no renderer packing or graphics handles.
#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    /// Stable birth identity.
    pub id: u64,
    /// Elapsed lifetime in seconds.
    pub age: f64,
    /// Lifetime in seconds.
    pub lifetime: f64,
    /// Position in simulation coordinates, in metres.
    pub position: [f32; 3],
    /// Velocity in simulation coordinates, in metres per second.
    pub velocity: [f32; 3],
    /// Unit xyzw orientation quaternion.
    pub rotation: [f32; 4],
    /// Uniform positive size.
    pub size: f32,
    /// Angular velocity in radians per second.
    pub spin: f32,
}

/// Per-producer evaluation storage. Clone intentionally reconstructs empty state.
#[derive(Debug, Default)]
pub struct ParticleRuntimeState {
    pub(crate) particles: Vec<Particle>,
    // Nonnegative f64 death offsets sort by their bits; Reverse gives the earliest.
    deaths: std::collections::BinaryHeap<std::cmp::Reverse<u64>>,
    pub(crate) elapsed: f64,
    pub(crate) fraction: f64,
    pub(crate) births: u64,
    pub(crate) started: bool,
    pub(crate) signature: Option<(u32, u32, u32)>,
    pub(crate) space: u32,
}

impl Clone for ParticleRuntimeState {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for ParticleRuntimeState {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

fn random(seed: u32, id: u64, lane: u64) -> f32 {
    let mut x = id.wrapping_mul(0x9e3779b97f4a7c15)
        ^ u64::from(seed)
        ^ lane.wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((x ^ (x >> 31)) >> 40) as f32 / 16777216.0
}

fn advance(p: &mut Particle, dt: f64, e: &ParticleEmitter) {
    let t = dt as f32;
    let acceleration = [e.acceleration_x, e.acceleration_y, e.acceleration_z];
    let (decay, integral, second) = if e.drag > 0.00001 {
        let decay = (-e.drag * t).exp();
        let integral = -(-e.drag * t).exp_m1() / e.drag;
        (decay, integral, (t - integral) / e.drag)
    } else {
        (1.0, t, 0.5 * t * t)
    };
    for (i, a) in acceleration.into_iter().enumerate() {
        p.position[i] += p.velocity[i] * integral + a * second;
        p.velocity[i] = p.velocity[i] * decay + a * integral;
    }
    let (s, c) = (p.spin * t * 0.5).sin_cos();
    let [x, y, z, w] = p.rotation;
    p.rotation = [c * x - s * y, c * y + s * x, c * z + s * w, c * w - s * z];
    p.age += dt;
}

fn transform_point(m: [f32; 16], p: [f32; 3], w: f32) -> [f32; 3] {
    std::array::from_fn(|i| m[i] * p[0] + m[i + 4] * p[1] + m[i + 8] * p[2] + m[i + 12] * w)
}

/// Emission tables are temporary CPU mesh observations, independent of GPU residency.
type ParticleEmissionTriangle = ([f32; 3], [f32; 3], [f32; 3], f32);

#[derive(Debug)]
pub(crate) struct ParticleEmissionSurface {
    /// Authored sample value.
    pub triangles: Vec<ParticleEmissionTriangle>,
    /// Authored sample value.
    pub area: f32,
}

impl ParticleEmissionSurface {
    pub fn new(mesh: &crate::MeshAsset) -> Self {
        let mut area = 0.0;
        let triangles = mesh
            .indices()
            .as_chunks::<3>()
            .0
            .iter()
            .filter_map(|indices| {
                let [a, b, c] = std::array::from_fn(|i| mesh.positions()[indices[i] as usize]);
                let u = sub(b, a);
                let v = sub(c, a);
                let n = cross(u, v);
                let weight = (n.iter().map(|v| v * v).sum::<f32>()).sqrt() * 0.5;
                if weight <= 0.0 {
                    return None;
                }
                area += weight;
                Some((a, b, c, area))
            })
            .collect();
        Self {
            triangles,
            area,
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|v| v * v).sum::<f32>().sqrt();
    v.map(|v| v / length)
}

pub(crate) fn simulate(
    e: &ParticleEmitter,
    state: &mut ParticleRuntimeState,
    dt: f64,
    model: [f32; 16],
    surface: Option<&ParticleEmissionSurface>,
) {
    use crate::components::schema::ComponentLifecycle;
    let signature = (e.seed, e.space, e.restart);
    if state.signature != Some(signature) {
        let (mut particles, mut deaths) = if crate::evaluation_scratch_reuse_enabled() {
            (
                std::mem::take(&mut state.particles),
                std::mem::take(&mut state.deaths),
            )
        } else {
            Default::default()
        };
        particles.clear();
        deaths.clear();
        *state = ParticleRuntimeState {
            particles,
            deaths,
            ..Default::default()
        };
        state.signature = Some(signature);
    }
    state.space = e.space;
    if e.validate().is_err() || !dt.is_finite() || dt < 0.0 {
        return;
    }
    if !crate::evaluation_scratch_reuse_enabled() {
        state.deaths = Default::default();
    }
    state.deaths.clear();
    state.deaths.extend(
        state
            .particles
            .iter()
            .map(|p| std::cmp::Reverse((p.lifetime - p.age).max(0.0).to_bits())),
    );
    state.particles.retain(|p| p.age + dt < p.lifetime);
    for p in &mut state.particles {
        advance(p, dt, e);
    }
    // A missing emission surface pauses the producer rather than losing births.
    if e.shape == 3 && surface.is_none_or(|s| s.area <= 0.0) {
        return;
    }
    let start = state.elapsed;
    let end = start + dt;
    let stop = if e.duration > 0.0 {
        end.min(e.delay as f64 + e.duration as f64)
    } else {
        end
    };
    let active = if e.enabled {
        (stop - start.max(e.delay as f64)).max(0.0)
    } else {
        0.0
    };
    // Carry fractional births across edits of rate without retroactive emission.
    let previous = state.births;
    let before = (state.elapsed_fraction() + 1e-10).floor() as u64;
    let total = state.elapsed_fraction() + active * e.rate as f64;
    let count = (total + 1e-10).floor() as u64 - before;
    let fraction = state.elapsed_fraction();
    let burst = if !state.started && e.enabled && end >= e.delay as f64 {
        u64::from(e.burst)
    } else {
        0
    };
    if end >= e.delay as f64 {
        state.started = true;
    }
    let births = count.saturating_add(burst);
    let mut ordinal = 0u64;
    while ordinal < births {
        let i = ordinal;
        ordinal += 1;
        let Some(id) = previous.checked_add(i) else {
            break;
        };
        let r = |lane| random(e.seed, id, lane);
        let birth_offset = (e.delay as f64 - start).max(0.0)
            + if i < burst {
                0.0
            } else {
                ((i - burst + 1) as f64 - fraction) / e.rate as f64
            };
        let age = (dt - birth_offset).max(0.0);
        let lifetime =
            (e.lifetime * (1.0 - e.lifetime_random * r(0))).max(f32::MIN_POSITIVE) as f64;
        while state
            .deaths
            .peek()
            .is_some_and(|death| f64::from_bits(death.0) <= birth_offset + 1e-10)
        {
            state.deaths.pop();
        }
        if state.deaths.len() >= e.capacity as usize {
            // Skip rejected births up to the next available slot without scanning
            // every particle or iterating a huge burst at the same timestamp.
            if i < burst {
                ordinal = burst;
            }
            if let Some(death) = state.deaths.peek() {
                let next = ((f64::from_bits(death.0) - (e.delay as f64 - start).max(0.0))
                    * e.rate as f64
                    + fraction
                    - 1e-10)
                    .ceil()
                    .max(1.0) as u64;
                ordinal = ordinal.max(burst.saturating_add(next.saturating_sub(1)));
            }
            continue;
        }
        state
            .deaths
            .push(std::cmp::Reverse((birth_offset + lifetime).to_bits()));
        if age >= lifetime {
            continue;
        }
        let mut normal = [0.0, 1.0, 0.0];
        let mut position = match e.shape {
            1 => [
                (2.0 * r(1) - 1.0) * e.extent_x,
                (2.0 * r(2) - 1.0) * e.extent_y,
                (2.0 * r(3) - 1.0) * e.extent_z,
            ],
            2 => {
                let y = 2.0 * r(1) - 1.0;
                let phi = std::f32::consts::TAU * r(2);
                let radius = e.extent_x * r(3).cbrt();
                let ring = (1.0 - y * y).sqrt();
                [
                    radius * ring * phi.cos(),
                    radius * y,
                    radius * ring * phi.sin(),
                ]
            }
            3 => {
                let s = surface.unwrap();
                let t = r(1) * s.area;
                let index = s
                    .triangles
                    .partition_point(|v| v.3 <= t)
                    .min(s.triangles.len() - 1);
                let (a, b, c, _) = s.triangles[index];
                normal = normalize(cross(sub(b, a), sub(c, a)));
                let u = r(2).sqrt();
                let v = r(3);
                std::array::from_fn(|i| (1.0 - u) * a[i] + u * (1.0 - v) * b[i] + u * v * c[i])
            }
            _ => [0.0; 3],
        };
        let tangent = normalize(cross(
            normal,
            if normal[1].abs() < 0.9 {
                [0.0, 1.0, 0.0]
            } else {
                [1.0, 0.0, 0.0]
            },
        ));
        let bitangent = cross(normal, tangent);
        let cos = 1.0 - r(4) * (1.0 - e.spread.cos());
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let phi = std::f32::consts::TAU * r(5);
        let speed = e.speed * (1.0 - e.speed_random * r(6));
        let mut velocity = std::array::from_fn(|i| {
            speed * (normal[i] * cos + sin * (tangent[i] * phi.cos() + bitangent[i] * phi.sin()))
        });
        if e.space == 1 {
            position = transform_point(model, position, 1.0);
            velocity = transform_point(model, velocity, 0.0);
        }
        let (s, c) = (0.5 * e.rotation_random * (2.0 * r(7) - 1.0)).sin_cos();
        let mut particle = Particle {
            id,
            age: 0.0,
            lifetime,
            position,
            velocity,
            rotation: [0.0, 0.0, s, c],
            size: e.size * (1.0 - e.size_random * r(8)).max(f32::MIN_POSITIVE),
            spin: e.spin,
        };
        advance(&mut particle, age, e);
        state.particles.push(particle);
    }
    state.births = previous.saturating_add(births);
    state.fraction = (total - (total + 1e-10).floor()).max(0.0);
    state.elapsed = end;
}

impl ParticleRuntimeState {
    fn elapsed_fraction(&self) -> f64 {
        self.fraction
    }
}

impl crate::services::asset_management::Asset for ParticleEmissionSurface {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of_val(self.triangles.as_slice())
    }
}

pub(crate) fn particle_surface_loader()
-> impl crate::services::asset_management::AssetLoader<Data = ParticleEmissionSurface> {
    crate::services::asset_management::BufferedAssetLoader::new(|bytes| {
        let (mesh, _) = crate::MeshAsset::decode(bytes).map_err(|e| e.to_string())?;
        let surface = ParticleEmissionSurface::new(&mesh);
        if !surface.area.is_finite() || surface.area <= 0.0 {
            return Err("Particle emission mesh has invalid area".into());
        }
        Ok(surface)
    })
}
