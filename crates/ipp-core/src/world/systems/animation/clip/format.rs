//! The IPPA v4 clip asset format: canonical encoding, cooperative output encoding
//! that borrows existing immutable typed tracks, and bounded decoding.

use super::track::{
    AnimationClip, AnimationEntityPlacementKey, AnimationInterpolation, AnimationKeyframe,
    AnimationProperty, AnimationSample, AnimationTrack, AnimationTrackData, AnimationTrackTarget,
    AnimationValue,
};
use crate::{
    ComponentValue, EntityId, ErrorReason,
    components::schema::{FieldKind, FieldValue},
    services::asset_management::*,
};
use std::{any::Any, collections::BTreeSet, sync::Arc};

/// Compiled immutable animation format identity.
pub const ANIMATION_TYPE: AssetTypeId = AssetTypeId(10);

impl AnimationClip {
    /// Encode the canonical IPPA v4 typed clip format.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes);
        out.extend_from_slice(b"IPPA");
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(&self.duration().to_le_bytes());
        out.extend_from_slice(&(self.tracks().len() as u32).to_le_bytes());
        for track in self.tracks() {
            let track = track.interchange();
            out.push(
                if matches!(track.target, AnimationTrackTarget::EntityLink) {
                    3
                } else if matches!(track.target, AnimationTrackTarget::DynamicProperty { .. }) {
                    2
                } else {
                    u8::from(track.target.is_pose())
                },
            );
            match &track.target {
                AnimationTrackTarget::EntityLink => {}
                AnimationTrackTarget::DynamicProperty {
                    component,
                    name,
                } => {
                    out.extend(component.to_le_bytes());
                    out.extend((name.len() as u32).to_le_bytes());
                    out.extend(name.as_bytes());
                }
                AnimationTrackTarget::AnimationProperty(property) => {
                    out.extend_from_slice(&property.component.to_le_bytes());
                    out.push(property.offsets.len() as u8);
                }
                AnimationTrackTarget::Joints(joints) => {
                    out.extend_from_slice(&(joints.len() as u32).to_le_bytes())
                }
            }
            for index in track.target.indices() {
                out.extend_from_slice(&index.to_le_bytes());
            }
            out.extend_from_slice(&(track.keys.len() as u32).to_le_bytes());
            for key in &track.keys {
                out.extend_from_slice(&key.time.to_le_bytes());
                key.value.encode(&mut out);
                match &key.interpolation {
                    AnimationInterpolation::Step => out.push(0),
                    AnimationInterpolation::Linear => out.push(1),
                    AnimationInterpolation::Bezier {
                        time1,
                        value1,
                        time2,
                        value2,
                    } => {
                        out.push(2);
                        out.extend_from_slice(&time1.to_le_bytes());
                        value1.encode(&mut out);
                        out.extend_from_slice(&time2.to_le_bytes());
                        value2.encode(&mut out);
                    }
                }
            }
        }
        out
    }

    /// Decode exact owned data, rejecting unknown tags, trailing bytes and malformed lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ErrorReason> {
        let mut r = AnimationClipReader {
            bytes,
            cursor: 0,
        };
        if r.take(4)? != b"IPPA" {
            return Err(ErrorReason::InvalidAsset);
        }
        let version = r.u32()?;
        if version != 4 {
            return Err(ErrorReason::InvalidAsset);
        }
        let duration = r.f64()?;
        let count = r.u32()? as usize;
        let mut tracks = Vec::new();
        for _ in 0..count {
            let tag = r.byte()?;
            let target = match tag {
                3 => AnimationTrackTarget::EntityLink,
                2 => {
                    let component = u16::from_le_bytes(r.array()?);
                    let length = r.u32()? as usize;
                    let name = std::str::from_utf8(r.take(length)?)
                        .map_err(|_| ErrorReason::InvalidAsset)?
                        .to_owned();
                    AnimationTrackTarget::DynamicProperty {
                        component,
                        name,
                    }
                }
                0 => {
                    let component = u16::from_le_bytes(r.array()?);
                    let n = r.byte()?;
                    if !matches!(n, 1 | 4) {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    let mut offsets = Vec::new();
                    for _ in 0..n {
                        offsets.push(r.u32()?);
                    }
                    AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component,
                        offsets,
                    })
                }
                1 => {
                    let n = r.u32()? as usize;
                    if n == 0 || n > crate::MAX_JOINTS {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    let mut joints = Vec::with_capacity(n);
                    for _ in 0..n {
                        joints.push(r.u32()?);
                    }
                    AnimationTrackTarget::Joints(joints)
                }
                _ => return Err(ErrorReason::InvalidAsset),
            };
            let count = r.u32()? as usize;
            let mut keys = Vec::new();
            for _ in 0..count {
                let time = r.f64()?;
                let value = r.value()?;
                let interpolation = match r.byte()? {
                    0 => AnimationInterpolation::Step,
                    1 => AnimationInterpolation::Linear,
                    2 => AnimationInterpolation::Bezier {
                        time1: r.f64()?,
                        value1: r.value()?,
                        time2: r.f64()?,
                        value2: r.value()?,
                    },
                    _ => return Err(ErrorReason::InvalidAsset),
                };
                keys.push(AnimationKeyframe {
                    time,
                    value,
                    interpolation,
                });
            }
            tracks.push(AnimationTrack {
                target,
                keys,
            });
        }
        if r.cursor != bytes.len() {
            return Err(ErrorReason::InvalidAsset);
        }
        Self::new(duration, tracks)
    }
}

impl AnimationValue {
    pub(super) fn encoded_bytes(&self) -> usize {
        1 + match self {
            Self::Field(FieldValue::Dynamic(value)) => 4 + value.encode().len(),
            Self::Field(FieldValue::F32(_) | FieldValue::U32(_)) => 4,
            Self::Field(FieldValue::U64(_) | FieldValue::Entity(_)) => 8,
            Self::Field(FieldValue::Bool(_)) => 1,
            Self::Field(FieldValue::String(v)) => 4 + v.len(),
            Self::Field(FieldValue::Bytes(v)) => 4 + v.len(),
            Self::Field(
                FieldValue::Rows(_)
                | FieldValue::Unset
                | FieldValue::World(_)
                | FieldValue::Output(_),
            ) => {
                unreachable!("clip validation rejects row tables and absence")
            }
            Self::Rotation(_) => 16,
            Self::EntityPlacement(_) => 8,
            Self::Pose(pose) => 4 + pose.len() * 40,
        }
    }

    pub(in crate::world::systems::animation) fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.kind());
        match self {
            Self::EntityPlacement(key) => {
                out.extend_from_slice(&key.parent.unwrap_or(u32::MAX).to_le_bytes());
                out.extend_from_slice(&key.before.unwrap_or(u32::MAX).to_le_bytes());
            }
            Self::Field(FieldValue::Dynamic(value)) => {
                let bytes = value.encode();
                out.extend((bytes.len() as u32).to_le_bytes());
                out.extend(bytes);
            }
            Self::Field(FieldValue::F32(v)) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Field(FieldValue::U32(v)) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Field(FieldValue::U64(v)) => out.extend_from_slice(&v.to_le_bytes()),
            Self::Field(FieldValue::Entity(v)) => out.extend_from_slice(&v.to_bits().to_le_bytes()),
            Self::Field(FieldValue::Bool(v)) => out.push(u8::from(*v)),
            Self::Field(FieldValue::String(v)) => {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v.as_bytes());
            }
            Self::Field(FieldValue::Bytes(v)) => {
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v);
            }
            Self::Field(
                FieldValue::Rows(_)
                | FieldValue::Unset
                | FieldValue::World(_)
                | FieldValue::Output(_),
            ) => {
                unreachable!("clip validation rejects row tables and absence")
            }
            Self::Pose(pose) => {
                out.extend_from_slice(&(pose.len() as u32).to_le_bytes());
                for joint in pose {
                    crate::services::asset_management::formats::skeleton::encode_transform(
                        out, joint,
                    );
                }
            }
            Self::Rotation(q) => {
                for v in q {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
}

impl AnimationClip {
    pub(crate) async fn encode_output(
        &self,
        output: &mut export::AssetOutput,
    ) -> Result<(), String> {
        output.write(b"IPPA\x04\0\0\0").await?;
        output.write(&self.duration().to_le_bytes()).await?;
        output.count(self.tracks().len()).await?;
        for track in self.tracks() {
            let target = track.target();
            output
                .byte(match target {
                    AnimationTrackTarget::EntityLink => 3,
                    AnimationTrackTarget::DynamicProperty {
                        ..
                    } => 2,
                    _ => u8::from(target.is_pose()),
                })
                .await?;
            match target {
                AnimationTrackTarget::EntityLink => {}
                AnimationTrackTarget::DynamicProperty {
                    component,
                    name,
                } => {
                    output.write(&component.to_le_bytes()).await?;
                    output.text(name).await?;
                }
                AnimationTrackTarget::AnimationProperty(property) => {
                    output.write(&property.component.to_le_bytes()).await?;
                    output.byte(property.offsets.len() as u8).await?;
                }
                AnimationTrackTarget::Joints(joints) => output.count(joints.len()).await?,
            }
            for index in target.indices() {
                output.u32(*index).await?;
            }
            track.encode_output(output).await?;
        }
        Ok(())
    }
}

async fn encode_sample<T: AnimationSample>(
    sample: &T,
    output: &mut export::AssetOutput,
) -> Result<(), String> {
    let value = sample as &dyn Any;
    if let Some(bytes) = value.downcast_ref::<Vec<u8>>() {
        output.byte(FieldKind::Bytes as u8).await?;
        output.count(bytes.len()).await?;
        return output.write(bytes).await;
    }
    if let Some(pose) = value.downcast_ref::<Vec<crate::components::Transform>>() {
        output.byte(9).await?;
        output.count(pose.len()).await?;
        for t in pose {
            output
                .floats(&[t.x, t.y, t.z, t.qx, t.qy, t.qz, t.qw, t.sx, t.sy, t.sz])
                .await?;
        }
        return Ok(());
    }
    let value = sample.to_value();
    if let AnimationValue::Field(FieldValue::String(text)) = &value {
        output.byte(FieldKind::String as u8).await?;
        return output.text(text).await;
    }
    // All remaining accepted samples are fixed-size numeric/placement values.
    let mut bytes = Vec::with_capacity(80);
    value.encode(&mut bytes);
    output.write(&bytes).await
}

pub(in crate::world::systems::animation) async fn encode_keys<T: AnimationSample>(
    track: &AnimationTrack<T>,
    output: &mut export::AssetOutput,
) -> Result<(), String> {
    output.count(track.keys.len()).await?;
    for key in &track.keys {
        output.write(&key.time.to_le_bytes()).await?;
        encode_sample(&key.value, output).await?;
        match &key.interpolation {
            AnimationInterpolation::Step => output.byte(0).await?,
            AnimationInterpolation::Linear => output.byte(1).await?,
            AnimationInterpolation::Bezier {
                time1,
                value1,
                time2,
                value2,
            } => {
                output.byte(2).await?;
                output.write(&time1.to_le_bytes()).await?;
                encode_sample(value1, output).await?;
                output.write(&time2.to_le_bytes()).await?;
                encode_sample(value2, output).await?;
            }
        }
    }
    Ok(())
}

struct AnimationClipReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> AnimationClipReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ErrorReason> {
        let end = self
            .cursor
            .checked_add(n)
            .ok_or(ErrorReason::InvalidAsset)?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(ErrorReason::InvalidAsset)?;
        self.cursor = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ErrorReason> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    fn byte(&mut self) -> Result<u8, ErrorReason> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, ErrorReason> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn f64(&mut self) -> Result<f64, ErrorReason> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    fn value(&mut self) -> Result<AnimationValue, ErrorReason> {
        let tag = self.byte()?;
        let value = match tag {
            11 => {
                let length = self.u32()? as usize;
                FieldValue::Dynamic(
                    crate::DynamicValue::decode(self.take(length)?)
                        .map_err(|_| ErrorReason::InvalidAsset)?,
                )
            }
            1 => FieldValue::F32(f32::from_le_bytes(self.array()?)),
            2 => FieldValue::Entity(EntityId::from_bits(u64::from_le_bytes(self.array()?))),
            3 => FieldValue::U32(self.u32()?),
            4 => FieldValue::U64(u64::from_le_bytes(self.array()?)),
            5 | 6 => {
                let len = self.u32()? as usize;
                let bytes = self.take(len)?;
                if tag == FieldKind::String as u8 {
                    FieldValue::String(
                        std::str::from_utf8(bytes)
                            .map_err(|_| ErrorReason::InvalidAsset)?
                            .into(),
                    )
                } else {
                    FieldValue::Bytes(bytes.to_vec())
                }
            }
            7 => FieldValue::Bool(match self.byte()? {
                0 => false,
                1 => true,
                _ => return Err(ErrorReason::InvalidAsset),
            }),
            8 => {
                return Ok(AnimationValue::Rotation([
                    f32::from_le_bytes(self.array()?),
                    f32::from_le_bytes(self.array()?),
                    f32::from_le_bytes(self.array()?),
                    f32::from_le_bytes(self.array()?),
                ]));
            }
            10 => {
                let parent = self.u32()?;
                let before = self.u32()?;
                return Ok(AnimationValue::EntityPlacement(
                    AnimationEntityPlacementKey {
                        parent: (parent != u32::MAX).then_some(parent),
                        before: (before != u32::MAX).then_some(before),
                    },
                ));
            }
            9 => {
                let count = self.u32()? as usize;
                if count == 0 || count > crate::MAX_JOINTS {
                    return Err(ErrorReason::InvalidAsset);
                }
                let mut pose = Vec::with_capacity(count);
                for _ in 0..count {
                    pose.push(
                        crate::services::asset_management::formats::skeleton::transform(
                            self.take(40)?,
                        )?,
                    );
                }
                return Ok(AnimationValue::Pose(pose));
            }
            _ => return Err(ErrorReason::InvalidAsset),
        };
        Ok(AnimationValue::Field(value))
    }
}

impl Asset for AnimationClip {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn decoded(&self) -> &dyn std::any::Any {
        self
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.tracks.capacity() * std::mem::size_of::<std::sync::Arc<dyn AnimationTrackData>>()
            + self
                .tracks
                .iter()
                .map(|track| track.resident_bytes())
                .sum::<usize>()
    }
}

pub(crate) fn animation_asset_loader() -> impl AssetLoader<Data = AnimationClip> {
    AsyncAssetLoader::decode(|mut reader| async move {
        AnimationClip::decode_reader(&mut *reader).await
    })
}

impl crate::services::asset_management::AssetEncoder for AnimationClip {
    fn encode_asset(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        if self.bytes > max_bytes {
            return Err("Asset output byte budget exhausted".into());
        }
        Ok(self.encode())
    }
}

impl AnimationClip {
    /// Decode targets and keys in order into private semantic storage.
    pub async fn decode_reader(reader: &mut dyn IoReader) -> Result<Self, String> {
        use crate::services::asset_management::decode::{AssetReader, push};
        let mut input = AssetReader::new(reader);
        if input.array::<4>().await? != *b"IPPA" || input.u32().await? != 4 {
            return Err("Invalid animation header".into());
        }
        let duration = input.f64().await?;
        let count = input.u32().await?;
        let mut tracks = Vec::new();
        for _ in 0..count {
            let target = match input.u8().await? {
                3 => AnimationTrackTarget::EntityLink,
                2 => {
                    let component = u16::from_le_bytes(input.array().await?);
                    let length = input.u32().await? as usize;
                    let name = input.text(length).await?;
                    AnimationTrackTarget::DynamicProperty {
                        component,
                        name,
                    }
                }
                0 => {
                    let component = u16::from_le_bytes(input.array().await?);
                    let count = input.u8().await?;
                    if !matches!(count, 1 | 4) {
                        return Err("Invalid animation property width".into());
                    }
                    let mut offsets = Vec::new();
                    for _ in 0..count {
                        offsets.push(input.u32().await?);
                    }
                    AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component,
                        offsets,
                    })
                }
                1 => {
                    let count = input.u32().await? as usize;
                    if count == 0 || count > crate::MAX_JOINTS {
                        return Err("Invalid animation joint count".into());
                    }
                    let mut joints = Vec::with_capacity(count);
                    for _ in 0..count {
                        joints.push(input.u32().await?);
                    }
                    AnimationTrackTarget::Joints(joints)
                }
                _ => return Err("Invalid animation target".into()),
            };
            let count = input.u32().await?;
            let mut keys = Vec::new();
            for _ in 0..count {
                let time = input.f64().await?;
                let value = read_animation_value(&mut input).await?;
                let interpolation = match input.u8().await? {
                    0 => AnimationInterpolation::Step,
                    1 => AnimationInterpolation::Linear,
                    2 => AnimationInterpolation::Bezier {
                        time1: input.f64().await?,
                        value1: read_animation_value(&mut input).await?,
                        time2: input.f64().await?,
                        value2: read_animation_value(&mut input).await?,
                    },
                    _ => return Err("Invalid animation interpolation".into()),
                };
                push(
                    &mut keys,
                    AnimationKeyframe {
                        time,
                        value,
                        interpolation,
                    },
                )?;
            }
            push(
                &mut tracks,
                AnimationTrack {
                    target,
                    keys,
                },
            )?;
        }
        input.finish().await?;
        let bytes = validate_parsed_tracks(duration, &tracks)
            .await
            .map_err(|error| error.to_string())?;
        let mut owned = Vec::new();
        for track in tracks {
            push(
                &mut owned,
                typed_track_async(track)
                    .await
                    .map_err(|error| error.to_string())?,
            )?;
        }
        Ok(Self {
            duration,
            tracks: owned,
            bytes,
        })
    }
}

async fn read_animation_value(
    input: &mut crate::services::asset_management::decode::AssetReader<'_>,
) -> Result<AnimationValue, String> {
    let tag = input.u8().await?;
    let value = match tag {
        11 => {
            let length = input.u32().await? as usize;
            if length == 0 {
                return Err("Empty dynamic animation value".into());
            }
            let tag = input.u8().await?;
            let kind =
                crate::DynamicPropertyKind::from_tag(tag).map_err(|error| format!("{error:?}"))?;
            let dynamic = if kind == crate::DynamicPropertyKind::Asset {
                if length < 7 {
                    return Err("Truncated animated asset value".into());
                }
                let kind = AssetTypeId(u16::from_le_bytes(input.array().await?));
                let variant = input.u32().await?;
                let uri = input.text(length - 7).await?.into();
                crate::DynamicValue::Asset(AssetSource {
                    kind,
                    variant,
                    uri,
                })
            } else {
                if length != kind.byte_len() + 1 {
                    return Err("Invalid dynamic animation length".into());
                }
                let mut bytes = [0; 65];
                bytes[0] = tag;
                input.fill(&mut bytes[1..length]).await?;
                crate::DynamicValue::decode(&bytes[..length])
                    .map_err(|error| format!("{error:?}"))?
            };
            FieldValue::Dynamic(dynamic)
        }
        1 => FieldValue::F32(input.f32().await?),
        2 => FieldValue::Entity(EntityId::from_bits(input.u64().await?)),
        3 => FieldValue::U32(input.u32().await?),
        4 => FieldValue::U64(input.u64().await?),
        5 | 6 => {
            let count = input.u32().await? as usize;
            if tag == 5 {
                FieldValue::String(input.text(count).await?.into())
            } else {
                FieldValue::Bytes(input.bytes(count).await?)
            }
        }
        7 => FieldValue::Bool(match input.u8().await? {
            0 => false,
            1 => true,
            _ => return Err("Invalid animation boolean".into()),
        }),
        8 => return Ok(AnimationValue::Rotation(input.floats().await?)),
        10 => {
            let parent = input.u32().await?;
            let before = input.u32().await?;
            return Ok(AnimationValue::EntityPlacement(
                AnimationEntityPlacementKey {
                    parent: (parent != u32::MAX).then_some(parent),
                    before: (before != u32::MAX).then_some(before),
                },
            ));
        }
        9 => {
            let count = input.u32().await? as usize;
            if count == 0 || count > crate::MAX_JOINTS {
                return Err("Invalid animation pose count".into());
            }
            let mut pose = Vec::with_capacity(count);
            for _ in 0..count {
                pose.push(
                    crate::services::asset_management::formats::skeleton::transform(
                        &input.array::<40>().await?,
                    )
                    .map_err(|error| error.to_string())?,
                );
            }
            return Ok(AnimationValue::Pose(pose));
        }
        _ => return Err("Invalid animation value".into()),
    };
    Ok(AnimationValue::Field(value))
}

async fn typed_track_async(
    track: AnimationTrack,
) -> Result<Arc<dyn AnimationTrackData>, ErrorReason> {
    async fn convert<T: AnimationSample>(
        track: AnimationTrack,
    ) -> Result<Arc<dyn AnimationTrackData>, ErrorReason> {
        let mut keys = Vec::new();
        let mut budget = crate::services::asset_management::decode::DecodeBudget::default();
        for key in track.keys {
            let interpolation = match key.interpolation {
                AnimationInterpolation::Step => AnimationInterpolation::Step,
                AnimationInterpolation::Linear => AnimationInterpolation::Linear,
                AnimationInterpolation::Bezier {
                    time1,
                    value1,
                    time2,
                    value2,
                } => AnimationInterpolation::Bezier {
                    time1,
                    value1: T::from_value(value1)?,
                    time2,
                    value2: T::from_value(value2)?,
                },
            };
            keys.try_reserve(1).map_err(|_| ErrorReason::Capacity)?;
            keys.push(AnimationKeyframe {
                time: key.time,
                value: T::from_value(key.value)?,
                interpolation,
            });
            budget.advance(0).await;
        }
        Ok(Arc::new(AnimationTrack::<T> {
            target: track.target,
            keys,
        }))
    }
    match track
        .keys
        .first()
        .ok_or(ErrorReason::InvalidAsset)?
        .value
        .kind()
    {
        11 => convert::<crate::DynamicValue>(track).await,
        1 => convert::<f32>(track).await,
        2 => convert::<EntityId>(track).await,
        3 => convert::<u32>(track).await,
        4 => convert::<u64>(track).await,
        5 => convert::<Arc<str>>(track).await,
        6 => convert::<Vec<u8>>(track).await,
        7 => convert::<bool>(track).await,
        8 => convert::<[f32; 4]>(track).await,
        10 => convert::<AnimationEntityPlacementKey>(track).await,
        9 => convert::<Vec<crate::components::Transform>>(track).await,
        _ => Err(ErrorReason::InvalidAsset),
    }
}

async fn validate_parsed_tracks(
    duration: f64,
    tracks: &[AnimationTrack],
) -> Result<usize, ErrorReason> {
    let mut budget = crate::services::asset_management::decode::DecodeBudget::default();
    if !duration.is_finite()
        || duration <= 0.0
        || tracks.is_empty()
        || u32::try_from(tracks.len()).is_err()
    {
        return Err(ErrorReason::InvalidAsset);
    }
    let mut bytes = 20usize;
    for track in tracks {
        budget.advance(0).await;
        if u32::try_from(track.keys.len()).is_err() || track.keys.is_empty() {
            return Err(ErrorReason::InvalidAsset);
        }
        let kind = track.keys[0].value.kind();
        match &track.target {
            AnimationTrackTarget::EntityLink => {
                if kind != 10 {
                    return Err(ErrorReason::InvalidAsset);
                }
            }
            AnimationTrackTarget::DynamicProperty {
                component,
                name,
            } => {
                crate::DynamicProperties::validate_name(name)
                    .map_err(|_| ErrorReason::InvalidAsset)?;
                if !ComponentValue::supports_dynamic_properties(*component) || kind != 11 {
                    return Err(ErrorReason::InvalidAsset);
                }
                let expected = match &track.keys[0].value {
                    AnimationValue::Field(FieldValue::Dynamic(v)) => v.kind(),
                    _ => unreachable!(),
                };
                for key in &track.keys {
                    budget.advance(0).await;
                    let valid = |v: &AnimationValue| matches!(v, AnimationValue::Field(FieldValue::Dynamic(v)) if v.kind() == expected);
                    if !valid(&key.value)
                        || matches!(&key.interpolation, AnimationInterpolation::Bezier { value1, value2, .. } if !valid(value1) || !valid(value2))
                    {
                        return Err(ErrorReason::InvalidAsset);
                    }
                }
            }
            AnimationTrackTarget::AnimationProperty(property) => {
                if !matches!(property.offsets.len(), 1 | 4)
                    || (kind == 8) != (property.offsets.len() == 4)
                    || matches!(kind, 9 | 10)
                {
                    return Err(ErrorReason::InvalidAsset);
                }
            }
            AnimationTrackTarget::Joints(joints) => {
                if joints.is_empty()
                    || joints.len() > crate::MAX_JOINTS
                    || joints
                        .iter()
                        .any(|&joint| joint as usize >= crate::MAX_JOINTS)
                    || joints.windows(2).any(|pair| pair[0] >= pair[1])
                    || kind != 9
                {
                    return Err(ErrorReason::InvalidAsset);
                }
                for key in &track.keys {
                    budget.advance(0).await;
                    let valid = |value: &AnimationValue| matches!(value, AnimationValue::Pose(pose) if pose.len() == joints.len());
                    if !valid(&key.value)
                        || matches!(&key.interpolation,
                            AnimationInterpolation::Bezier { value1, value2, .. } if !valid(value1) || !valid(value2))
                    {
                        return Err(ErrorReason::InvalidAsset);
                    }
                }
            }
        }
        let mut properties = BTreeSet::new();
        for &index in track.target.indices() {
            if !properties.insert(index) {
                return Err(ErrorReason::InvalidAsset);
            }
        }
        let target_bytes = match &track.target {
            AnimationTrackTarget::EntityLink => 5,
            AnimationTrackTarget::DynamicProperty {
                name,
                ..
            } => {
                u32::try_from(name.len()).map_err(|_| ErrorReason::Capacity)?;
                11usize
                    .checked_add(name.len())
                    .ok_or(ErrorReason::Capacity)?
            }
            AnimationTrackTarget::AnimationProperty(property) => 8usize
                .checked_add(
                    property
                        .offsets
                        .len()
                        .checked_mul(4)
                        .ok_or(ErrorReason::Capacity)?,
                )
                .ok_or(ErrorReason::Capacity)?,
            AnimationTrackTarget::Joints(joints) => 9usize
                .checked_add(joints.len().checked_mul(4).ok_or(ErrorReason::Capacity)?)
                .ok_or(ErrorReason::Capacity)?,
        };
        bytes = bytes
            .checked_add(target_bytes)
            .ok_or(ErrorReason::Capacity)?;
        for (index, key) in track.keys.iter().enumerate() {
            budget.advance(0).await;
            if !key.time.is_finite()
                || !(0.0..=duration).contains(&key.time)
                || !key.value.same_type(&track.keys[0].value)
            {
                return Err(ErrorReason::InvalidAsset);
            }
            key.value.validate()?;
            bytes = bytes
                .checked_add(9 + key.value.encoded_bytes())
                .ok_or(ErrorReason::Capacity)?;
            let next = track.keys.get(index + 1);
            if next.is_some_and(|next| key.time >= next.time) {
                return Err(ErrorReason::InvalidAsset);
            }
            match &key.interpolation {
                AnimationInterpolation::Step => {}
                AnimationInterpolation::Linear if next.is_some() && key.value.numeric() => {}
                AnimationInterpolation::Bezier {
                    time1,
                    value1,
                    time2,
                    value2,
                } => {
                    let next = next.ok_or(ErrorReason::InvalidAsset)?;
                    if !key.value.numeric()
                        || !time1.is_finite()
                        || !time2.is_finite()
                        || !(key.time..=next.time).contains(time1)
                        || !(time1..=&next.time).contains(&time2)
                        || !value1.same_type(&key.value)
                        || !value2.same_type(&key.value)
                    {
                        return Err(ErrorReason::InvalidAsset);
                    }
                    value1.validate()?;
                    value2.validate()?;
                    bytes = bytes
                        .checked_add(16 + value1.encoded_bytes() + value2.encoded_bytes())
                        .ok_or(ErrorReason::Capacity)?;
                }
                _ => return Err(ErrorReason::InvalidAsset),
            }
        }
    }

    Ok(bytes)
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod format_tests;
