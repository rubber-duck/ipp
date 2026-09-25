use crate::{
    ErrorReason,
    components::Transform,
    components::rows::{Rows, SchemaRow, row_address, row_region_relative},
    components::schema::ComponentLifecycle,
    services::asset_management::service::{AssetDemandSelection, validate_source},
};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// Per-instance skeleton source, optional reusable pose, and sparse local overrides.
///
/// Every operation that changes a Skeleton validates the complete value: two
/// source URIs and at most [`crate::MAX_JOINTS`] (32) override rows, a bounded
/// check of well under a microsecond that needs no per-field replacement.
#[repr(C)]
#[derive(Debug, SchemaComponent)]
pub struct Skeleton {
    /// Immutable hierarchy/rest-pose URI; empty leaves the instance inactive.
    pub source: String,
    /// Skeleton variant.
    pub variant: u32,
    /// Optional immutable local pose URI; empty selects the skeleton rest pose.
    pub pose_source: String,
    /// Reusable pose variant.
    pub pose_variant: u32,
    /// Joint-local overrides; the slot is the joint ordinal in the skeleton asset.
    ///
    /// Every ordinal below [`crate::MAX_JOINTS`] starts as a live row with no
    /// properties, so clients set and clear override properties by slot without
    /// creating rows; a row may never occupy a higher slot. Clear an override with
    /// `Unset` rather than removing its row: a removed ordinal cannot hold a row
    /// again within this component incarnation.
    ///
    /// Override properties are never animation targets: `animatable_field`
    /// rejects every offset in this rows region at bind, for numeric and discrete
    /// tracks alike. Joint animation samples the evaluated local pose through
    /// `AnimationTrackTarget::Joints`, so the pose keeps one animation writer and
    /// these rows stay authored input.
    #[schema(rows)]
    pub joints: Rows<JointOverrideRow>,
    /// Component-owned evaluated data, excluded from authored copies and wire access.
    #[schema(ignore)]
    pub runtime: SkeletonRuntimeState,
}

/// One joint's local TRS override. Each present property replaces that part of
/// the selected pose or rest transform; absent properties keep it.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct JointOverrideRow {
    /// Local translation in metres.
    pub translation: Option<[f32; 3]>,
    /// Local xyzw rotation; any nonzero finite quaternion, normalized during evaluation.
    #[schema(rotation)]
    pub rotation: Option<[f32; 4]>,
    /// Positive local scale.
    pub scale: Option<[f32; 3]>,
}

impl JointOverrideRow {
    /// Whether this row overrides any property.
    pub fn is_empty(&self) -> bool {
        self.translation.is_none() && self.rotation.is_none() && self.scale.is_none()
    }

    /// Replace the present properties of a local joint transform.
    pub fn apply(&self, local: &mut Transform) {
        if let Some([x, y, z]) = self.translation {
            (local.x, local.y, local.z) = (x, y, z);
        }

        if let Some([qx, qy, qz, qw]) = self.rotation {
            (local.qx, local.qy, local.qz, local.qw) = (qx, qy, qz, qw);
        }

        if let Some([sx, sy, sz]) = self.scale {
            (local.sx, local.sy, local.sz) = (sx, sy, sz);
        }
    }

    /// The Transform rule for each present property: finite values, a nonzero
    /// rotation and a positive scale.
    fn validate(&self) -> Result<(), ErrorReason> {
        let finite = |values: &[f32]| values.iter().all(|value| value.is_finite());
        if self.translation.is_some_and(|value| !finite(&value))
            || self
                .rotation
                .is_some_and(|value| !finite(&value) || value.iter().all(|&v| v == 0.0))
            || self
                .scale
                .is_some_and(|value| !finite(&value) || value.iter().any(|&v| v <= 0.0))
        {
            return Err(ErrorReason::InvalidValue);
        }

        Ok(())
    }
}

impl Skeleton {
    /// Whether every present override names a joint below `joint_count`.
    pub(crate) fn joint_overrides_fit(&self, joint_count: usize) -> bool {
        self.joints
            .iter()
            .all(|(joint, row)| (joint as usize) < joint_count || row.is_empty())
    }

    /// Merge overrides into a complete local pose in the skeleton's joint order.
    pub(crate) fn apply_joint_overrides(&self, local: &mut [Transform]) -> Result<(), ErrorReason> {
        if !self.joint_overrides_fit(local.len()) {
            return Err(ErrorReason::InvalidValue);
        }

        for (joint, row) in self.joints.iter() {
            if let Some(local) = local.get_mut(joint as usize) {
                row.apply(local);
            }
        }

        Ok(())
    }

    /// Merge overrides into the selected ascending joints of a local pose.
    pub(crate) fn apply_selected_joint_overrides(
        &self,
        joint_count: usize,
        joints: &[u32],
        selected: &mut [Transform],
    ) -> Result<(), ErrorReason> {
        if !self.joint_overrides_fit(joint_count) {
            return Err(ErrorReason::InvalidValue);
        }

        for (&joint, local) in joints.iter().zip(selected) {
            if let Some(row) = self.joints.get(joint) {
                row.apply(local);
            }
        }

        Ok(())
    }

    fn validate_joint_overrides(&self) -> Result<(), ErrorReason> {
        for (joint, row) in self.joints.iter() {
            if joint as usize >= crate::MAX_JOINTS {
                return Err(ErrorReason::InvalidValue);
            }

            row.validate()?;
        }

        Ok(())
    }
}

/// An empty live override row at every addressable joint ordinal.
fn joint_override_rows() -> Rows<JointOverrideRow> {
    let mut rows = Rows::new();
    for joint in 0..crate::MAX_JOINTS as u32 {
        rows.insert(joint, JointOverrideRow::default())
            .expect("joint ordinals lie within the rows region");
    }

    rows
}

impl Default for Skeleton {
    fn default() -> Self {
        Self {
            source: String::new(),
            variant: 0,
            pose_source: String::new(),
            pose_variant: 0,
            joints: joint_override_rows(),
            runtime: SkeletonRuntimeState::default(),
        }
    }
}

/// Private evaluated storage of one Skeleton component incarnation.
#[derive(Debug, Default)]
pub struct SkeletonRuntimeState {
    /// Evaluated pose, absent until the rig's assets resolve. Boxed because it
    /// is runtime state no command or snapshot carries: inline it would add
    /// 72 bytes to every Skeleton cell and, in builds without `gui`, make
    /// Skeleton the largest `ComponentValue` variant (152 to 200 bytes on
    /// 64-bit targets).
    pub(crate) pose: Option<Box<SkeletonPoseState>>,
}

/// Allocations remain stable while their source and component incarnation survive.
#[derive(Debug)]
pub(crate) struct SkeletonPoseState {
    pub(crate) valid: bool,
    pub(crate) source: crate::services::asset_management::AssetKey,
    pub(crate) local: Box<[crate::components::Transform]>,
    pub(crate) global: Box<[[f32; 16]]>,
    // Evaluation scratch has the same lifetime as this rig, never authored or serialized.
    pub(crate) evaluation: Box<[crate::components::Transform]>,
    pub(crate) sampled: Box<[bool]>,
}

impl Clone for Skeleton {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
            variant: self.variant,
            pose_source: self.pose_source.clone(),
            pose_variant: self.pose_variant,
            joints: self.joints.clone(),
            runtime: Default::default(),
        }
    }
}

impl PartialEq for Skeleton {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.variant == other.variant
            && self.pose_source == other.pose_source
            && self.pose_variant == other.pose_variant
            && self.joints == other.joints
    }
}

impl ComponentLifecycle for Skeleton {
    fn animatable_field(offset: u32) -> bool {
        row_region_relative(offset, 0).is_none()
    }

    fn preserve_runtime(&mut self, previous: &mut Self) {
        if self.source == previous.source && self.variant == previous.variant {
            self.runtime = std::mem::take(&mut previous.runtime);
        }
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[
            crate::components::schema::ComponentAssetReference {
                kind: 3,
                source_offset: std::mem::offset_of!(Self, source) as u32,
                variant_offset: std::mem::offset_of!(Self, variant) as u32,
            },
            crate::components::schema::ComponentAssetReference {
                kind: 4,
                source_offset: std::mem::offset_of!(Self, pose_source) as u32,
                variant_offset: std::mem::offset_of!(Self, pose_variant) as u32,
            },
        ]
    }

    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == std::mem::offset_of!(Self, source) as u32 {
            validate_source(&self.source)?;
        }
        if offset == std::mem::offset_of!(Self, pose_source) as u32 {
            validate_source(&self.pose_source)?;
        }
        if offset == std::mem::offset_of!(Self, joints) as u32 {
            self.validate_joint_overrides()?;
        } else if let Some(relative) = row_region_relative(offset, 0) {
            let address = row_address(relative, JointOverrideRow::LAYOUT.property_count())
                .ok_or(ErrorReason::InvalidField)?;
            if address.slot as usize >= crate::MAX_JOINTS {
                return Err(ErrorReason::InvalidValue);
            }

            if let Some(row) = self.joints.get(address.slot) {
                row.validate()?;
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)?;
        validate_source(&self.pose_source)?;
        self.validate_joint_overrides()?;
        Ok(())
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::SKELETON_TYPE,
                &self.source,
                self.variant,
            ));
        }
        if !self.pose_source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::POSE_TYPE,
                &self.pose_source,
                self.pose_variant,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ComponentValue, components::Transform, components::registry::ComponentStorage,
        components::schema::SchemaComponent,
    };

    #[test]
    fn skeleton_updates_move_runtime_without_cloning_or_exposing_it() {
        let mut storage = ComponentStorage::default();
        storage.reserve(1);
        let value = Skeleton {
            source: "asset://3/1".into(),
            runtime: SkeletonRuntimeState {
                pose: Some(Box::new(SkeletonPoseState {
                    valid: true,
                    source: crate::services::asset_management::AssetKey::from_u64(1),
                    local: vec![Transform::default()].into_boxed_slice(),
                    global: vec![[0.0; 16]].into_boxed_slice(),
                    evaluation: vec![Transform::default()].into_boxed_slice(),
                    sampled: vec![false].into_boxed_slice(),
                })),
            },
            ..Default::default()
        };
        let address = value.runtime.pose.as_ref().unwrap().local.as_ptr();
        let mut authored = value.clone();
        assert!(authored.runtime.pose.is_none());
        assert_eq!(authored, value);
        assert!(!Skeleton::has_field(
            std::mem::offset_of!(Skeleton, runtime) as u32
        ));
        assert_eq!(value.fields().len(), 5);
        storage.set(0, ComponentValue::Skeleton(value));

        authored.pose_source = "asset://4/2".into();
        storage.set(0, ComponentValue::Skeleton(authored));
        assert_eq!(
            storage
                .skeleton(0)
                .unwrap()
                .runtime
                .pose
                .as_ref()
                .unwrap()
                .local
                .as_ptr(),
            address
        );
        let Some(ComponentValue::Skeleton(copy)) = storage.get(ComponentValue::SKELETON, 0) else {
            panic!("missing authored observation");
        };
        assert!(copy.runtime.pose.is_none());

        let replacement = Skeleton {
            source: "asset://3/2".into(),
            ..Default::default()
        };
        storage.set(0, ComponentValue::Skeleton(replacement));
        assert!(storage.skeleton(0).unwrap().runtime.pose.is_none());
    }
}
