use crate::{
    ErrorReason,
    components::schema::ComponentLifecycle,
    services::asset_management::service::{AssetDemandSelection, validate_source},
};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// Per-instance skeleton source, optional reusable pose, and sparse local overrides.
#[repr(C)]
#[derive(Debug, Default, SchemaComponent)]
pub struct Skeleton {
    /// Immutable hierarchy/rest-pose URI; empty leaves the instance inactive.
    pub source: String,
    /// Skeleton variant.
    pub variant: u32,
    /// Optional immutable local pose URI; empty selects the skeleton rest pose.
    pub pose_source: String,
    /// Reusable pose variant.
    pub pose_variant: u32,
    /// Sparse joint-local TRS replacements, encoded in ascending joint order.
    pub joints: Vec<u8>,
    /// Component-owned evaluated data, excluded from authored copies and wire access.
    #[schema(ignore)]
    pub runtime: SkeletonRuntimeState,
}

/// Private evaluated storage of one Skeleton component incarnation.
#[derive(Debug, Default)]
pub struct SkeletonRuntimeState {
    pub(crate) pose: Option<SkeletonPoseState>,
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
            crate::services::asset_management::skeleton::overrides(&self.joints)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)?;
        validate_source(&self.pose_source)?;
        crate::services::asset_management::skeleton::overrides(&self.joints)?;
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
                pose: Some(SkeletonPoseState {
                    valid: true,
                    source: crate::services::asset_management::AssetKey::from_u64(1),
                    local: vec![Transform::default()].into_boxed_slice(),
                    global: vec![[0.0; 16]].into_boxed_slice(),
                    evaluation: vec![Transform::default()].into_boxed_slice(),
                    sampled: vec![false].into_boxed_slice(),
                }),
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
