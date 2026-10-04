use super::{Particle, ParticleRuntimeState};
use crate::{
    ErrorReason,
    services::asset_management::{Asset, AssetLoader, AssetTypeId, AsyncAssetLoader},
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
    AsyncAssetLoader::decode(|mut reader| async move {
        ParticleCache::decode_reader(&mut *reader).await
    })
}

impl ParticleCache {
    /// Retain the typed directory, then decode its contiguous sample streams.
    pub async fn decode_reader(
        reader: &mut dyn crate::services::io::IoReader,
    ) -> Result<Self, String> {
        use crate::services::asset_management::decode::{AssetReader, push};
        use std::num::NonZeroUsize;
        let mut input = AssetReader::new(reader);
        if input.array::<4>().await? != *b"IPPC" || input.u32().await? != 1 {
            return Err("Invalid particle cache header".into());
        }
        let space = input.u32().await?;
        let count = input.u32().await? as usize;
        if space > 1 || count == 0 {
            return Err("Invalid particle cache directory".into());
        }
        let mut offset = count
            .checked_mul(12)
            .and_then(|bytes| bytes.checked_add(16))
            .ok_or("Particle cache length overflow")?;
        let mut previous = f32::NEG_INFINITY;
        let mut directory = Vec::new();
        for _ in 0..count {
            let time = input.f32().await?;
            let start = input.u32().await? as usize;
            let count = input.u32().await? as usize;
            if time <= previous || start != offset {
                return Err("Invalid particle cache frame".into());
            }
            offset = count
                .checked_mul(60)
                .and_then(|bytes| offset.checked_add(bytes))
                .ok_or("Particle cache length overflow")?;
            push(&mut directory, (time, count))?;
            previous = time;
        }
        let mut identities = std::collections::BTreeMap::new();
        let mut frames = Vec::new();
        for (time, count) in directory {
            let mut samples = Vec::new();
            let mut last = None;
            input
                .records(NonZeroUsize::new(60).unwrap(), count, |record| {
                    let id = u64::from_le_bytes(record[..8].try_into().unwrap());
                    let values: [f32; 13] = std::array::from_fn(|index| {
                        f32::from_le_bytes(
                            record[8 + index * 4..12 + index * 4].try_into().unwrap(),
                        )
                    });
                    let qlen = values[8..12].iter().map(|value| value * value).sum::<f32>();
                    if last.is_some_and(|last| last >= id)
                        || values.iter().any(|value| !value.is_finite())
                        || values[0] >= values[1]
                        || values[12] <= 0.0
                        || (qlen - 1.0).abs() > 0.001
                        || time < values[0]
                        || time > values[1]
                        || identities
                            .insert(id, (values[0], values[1]))
                            .is_some_and(|old| old != (values[0], values[1]))
                    {
                        return Err("Invalid particle cache sample".into());
                    }
                    push(
                        &mut samples,
                        ParticleCacheSample {
                            id,
                            birth: values[0],
                            death: values[1],
                            position: values[2..5].try_into().unwrap(),
                            velocity: values[5..8].try_into().unwrap(),
                            rotation: values[8..12].try_into().unwrap(),
                            size: values[12],
                        },
                    )?;
                    last = Some(id);
                    Ok(())
                })
                .await?;
            push(
                &mut frames,
                ParticleCacheFrame {
                    time,
                    samples,
                },
            )?;
        }
        input.finish().await?;
        Ok(Self {
            space,
            frames,
        })
    }
}
