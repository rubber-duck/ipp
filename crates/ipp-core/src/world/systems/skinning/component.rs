use crate::{
    ErrorReason,
    components::schema::ComponentLifecycle,
    services::asset_management::service::{AssetDemandSelection, validate_source},
};
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// A mesh instance bound to one live skeleton instance and immutable inverse binds.
#[repr(C)]
#[derive(Debug, Default, SchemaComponent)]
pub struct Skin {
    /// Skeleton entity; zero leaves this binding inactive.
    pub skeleton: crate::EntityId,
    /// Immutable skin binding URI; empty leaves this binding inactive.
    pub source: String,
    /// Binding variant.
    pub variant: u32,
    /// Component-owned evaluated palette, excluded from authored copies and wire access.
    #[schema(ignore)]
    pub runtime: SkinRuntimeState,
}

/// Private evaluated storage of one Skin component incarnation.
#[derive(Debug, Default)]
pub struct SkinRuntimeState {
    pub(crate) valid: bool,
    pub(crate) palette: Option<Box<[[f32; 16]]>>,
}

impl Clone for Skin {
    fn clone(&self) -> Self {
        Self {
            skeleton: self.skeleton,
            source: self.source.clone(),
            variant: self.variant,
            runtime: Default::default(),
        }
    }
}

impl PartialEq for Skin {
    fn eq(&self, other: &Self) -> bool {
        self.skeleton == other.skeleton
            && self.source == other.source
            && self.variant == other.variant
    }
}

impl ComponentLifecycle for Skin {
    fn preserve_runtime(&mut self, previous: &mut Self) {
        if self.skeleton == previous.skeleton
            && self.source == previous.source
            && self.variant == previous.variant
        {
            self.runtime = std::mem::take(&mut previous.runtime);
        }
    }

    fn asset_references() -> &'static [crate::components::schema::ComponentAssetReference] {
        &[crate::components::schema::ComponentAssetReference {
            kind: 5,
            source_offset: std::mem::offset_of!(Self, source) as u32,
            variant_offset: std::mem::offset_of!(Self, variant) as u32,
        }]
    }

    fn validate_field(&self, offset: u32) -> Result<(), ErrorReason> {
        if offset == std::mem::offset_of!(Self, source) as u32 {
            validate_source(&self.source)?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        validate_source(&self.source)
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        if !self.source.is_empty() {
            demand.insert(AssetDemandSelection::new(
                crate::SKIN_TYPE,
                &self.source,
                self.variant,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::schema::SchemaComponent;

    #[test]
    fn skin_updates_preserve_palette_only_for_the_same_binding() {
        let mut skin = Skin {
            source: "asset://5/1".into(),
            runtime: SkinRuntimeState {
                valid: true,
                palette: Some(vec![[0.0; 16]].into_boxed_slice()),
            },
            ..Default::default()
        };
        let address = skin.runtime.palette.as_ref().unwrap().as_ptr();
        let mut next = skin.clone();
        assert!(next.runtime.palette.is_none());
        next.preserve_runtime(&mut skin);
        assert_eq!(next.runtime.palette.as_ref().unwrap().as_ptr(), address);
        assert!(skin.runtime.palette.is_none());
        assert!(!Skin::has_field(std::mem::offset_of!(Skin, runtime) as u32));
        skin.skeleton = crate::EntityId::from_bits(2);
        skin.preserve_runtime(&mut next);
        assert!(skin.runtime.palette.is_none());
    }
}
