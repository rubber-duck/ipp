//! Cooperative encoding borrows existing immutable typed tracks.

use super::{
    AnimationClip, AnimationInterpolation, AnimationSample, AnimationTrack, AnimationTrackTarget,
    AnimationValue,
};
use crate::{
    components::schema::{FieldKind, FieldValue},
    services::asset_management::export,
};
use std::any::Any;

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

pub(super) async fn encode_keys<T: AnimationSample>(
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
