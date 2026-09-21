use super::{Particle, ParticleRuntimeState};
use crate::{
    ErrorReason,
    services::asset_management::{Asset, AssetLoader, AssetTypeId, BufferedAssetLoader},
};

/// IPPC immutable sampled particle data.
pub const PARTICLE_CACHE_TYPE: AssetTypeId = AssetTypeId(15);

/// Portable sampled particle data.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleCacheSample {
    /// Stable birth identity.
    pub id: u64,
    /// Absolute birth time in seconds.
    pub birth: f32,
    /// Absolute death time in seconds.
    pub death: f32,
    /// Position in simulation coordinates, in metres.
    pub position: [f32; 3],
    /// Velocity in simulation coordinates, in metres per second.
    pub velocity: [f32; 3],
    /// Unit xyzw orientation quaternion.
    pub rotation: [f32; 4],
    /// Uniform positive size.
    pub size: f32,
}

/// Portable sampled particle data.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleCacheFrame {
    /// Absolute sample time in seconds.
    pub time: f32,
    /// Increasing stable identities; no correspondence by array slot.
    pub samples: Vec<ParticleCacheSample>,
}

/// Portable sampled particle data.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleCache {
    /// Local=0 or World=1, independent of renderer packing.
    pub space: u32,
    /// Strictly increasing sample frames.
    pub frames: Vec<ParticleCacheFrame>,
}

impl ParticleCache {
    /// IPPC v1: 16-byte header, 12-byte frame directory, 60-byte sample records.
    pub fn encode(&self) -> Result<Vec<u8>, ErrorReason> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"IPPC");
        for word in [
            1,
            self.space,
            u32::try_from(self.frames.len()).map_err(|_| ErrorReason::Capacity)?,
        ] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut offset = 16 + self.frames.len() * 12;
        for frame in &self.frames {
            bytes.extend_from_slice(&frame.time.to_le_bytes());
            bytes.extend_from_slice(
                &u32::try_from(offset)
                    .map_err(|_| ErrorReason::Capacity)?
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(
                &u32::try_from(frame.samples.len())
                    .map_err(|_| ErrorReason::Capacity)?
                    .to_le_bytes(),
            );
            offset = offset
                .checked_add(frame.samples.len() * 60)
                .ok_or(ErrorReason::Capacity)?;
        }
        for frame in &self.frames {
            for p in &frame.samples {
                bytes.extend_from_slice(&p.id.to_le_bytes());
                for value in [p.birth, p.death]
                    .into_iter()
                    .chain(p.position)
                    .chain(p.velocity)
                    .chain(p.rotation)
                    .chain([p.size])
                {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        Self::decode(&bytes)?;
        Ok(bytes)
    }

    /// Decode and validate a complete immutable cache.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        let invalid = ErrorReason::InvalidAsset;
        if bytes.len() < 16 || &bytes[..4] != b"IPPC" {
            return Err(invalid);
        }
        let word = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let float = |i| f32::from_bits(word(i));
        let count = word(12) as usize;
        let mut offset = count
            .checked_mul(12)
            .and_then(|n| n.checked_add(16))
            .filter(|n| *n <= bytes.len())
            .ok_or(invalid)?;
        if word(4) != 1 || word(8) > 1 || count == 0 {
            return Err(invalid);
        }
        let mut frames = Vec::with_capacity(count);
        let mut previous = f32::NEG_INFINITY;
        let mut identities = std::collections::BTreeMap::new();
        for i in 0..count {
            let at = 16 + i * 12;
            let time = float(at);
            let n = word(at + 8) as usize;
            if !time.is_finite() || time <= previous || word(at + 4) as usize != offset {
                return Err(invalid);
            }
            let end = n
                .checked_mul(60)
                .and_then(|v| offset.checked_add(v))
                .filter(|v| *v <= bytes.len())
                .ok_or(invalid)?;
            let mut samples = Vec::with_capacity(n);
            let mut last = None;
            while offset < end {
                let id = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
                let v: [f32; 13] = std::array::from_fn(|i| float(offset + 8 + i * 4));
                let qlen = v[8..12].iter().map(|v| v * v).sum::<f32>();
                if last.is_some_and(|last| last >= id)
                    || v.iter().any(|v| !v.is_finite())
                    || v[0] >= v[1]
                    || v[12] <= 0.0
                    || (qlen - 1.0).abs() > 0.001
                    || time < v[0]
                    || time > v[1]
                {
                    return Err(invalid);
                }
                if identities
                    .insert(id, (v[0], v[1]))
                    .is_some_and(|old| old != (v[0], v[1]))
                {
                    return Err(invalid);
                }
                samples.push(ParticleCacheSample {
                    id,
                    birth: v[0],
                    death: v[1],
                    position: v[2..5].try_into().unwrap(),
                    velocity: v[5..8].try_into().unwrap(),
                    rotation: v[8..12].try_into().unwrap(),
                    size: v[12],
                });
                last = Some(id);
                offset += 60;
            }
            frames.push(ParticleCacheFrame {
                time,
                samples,
            });
            previous = time;
        }
        if offset != bytes.len() {
            return Err(invalid);
        }
        Ok(Self {
            space: word(8),
            frames,
        })
    }

    pub(crate) fn sample(&self, time: f32, state: &mut ParticleRuntimeState) {
        state.particles.clear();
        state.space = self.space;
        if !time.is_finite()
            || time < self.frames[0].time
            || time > self.frames.last().unwrap().time
        {
            return;
        }
        let left = self
            .frames
            .partition_point(|f| f.time <= time)
            .saturating_sub(1);
        let a = &self.frames[left];
        let b = self.frames.get(left + 1).unwrap_or(a);
        let weight = if b.time > a.time {
            (time - a.time) / (b.time - a.time)
        } else {
            0.0
        };
        for p in &a.samples {
            if time < p.birth || time >= p.death {
                continue;
            }
            let next = b
                .samples
                .binary_search_by_key(&p.id, |v| v.id)
                .ok()
                .map(|i| &b.samples[i]);
            let mut position = p.position;
            let mut velocity = p.velocity;
            let mut rotation = p.rotation;
            let mut size = p.size;
            if let Some(q) = next {
                position = std::array::from_fn(|i| {
                    p.position[i] + weight * (q.position[i] - p.position[i])
                });
                velocity = std::array::from_fn(|i| {
                    p.velocity[i] + weight * (q.velocity[i] - p.velocity[i])
                });
                let sign = if p
                    .rotation
                    .iter()
                    .zip(q.rotation)
                    .map(|(a, b)| a * b)
                    .sum::<f32>()
                    < 0.0
                {
                    -1.0
                } else {
                    1.0
                };
                rotation = std::array::from_fn(|i| {
                    p.rotation[i] * (1.0 - weight) + q.rotation[i] * sign * weight
                });
                let norm = rotation.iter().map(|v| v * v).sum::<f32>().sqrt();
                rotation = rotation.map(|v| v / norm);
                size = p.size + weight * (q.size - p.size);
            }
            state.particles.push(Particle {
                id: p.id,
                age: f64::from(time - p.birth),
                lifetime: f64::from(p.death - p.birth),
                position,
                velocity,
                rotation,
                size,
                spin: 0.0,
            });
        }
    }
}

impl Asset for ParticleCache {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        self.frames
            .iter()
            .map(|f| std::mem::size_of_val(f.samples.as_slice()))
            .sum()
    }
}

pub(crate) fn particle_cache_loader() -> impl AssetLoader<Data = ParticleCache> {
    BufferedAssetLoader::new(|bytes| ParticleCache::decode(bytes).map_err(|e| e.to_string()))
}
