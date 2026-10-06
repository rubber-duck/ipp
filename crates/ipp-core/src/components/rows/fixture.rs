//! Registered test-only component with two schema rows fields, exercising
//! region dispatch, registry access, staging and persistence without a
//! production consumer.
#![allow(missing_docs)]
use super::{Rows, SchemaRow};
use crate::services::asset_management::AssetSource;
use ipp_schema_derive::SchemaComponent;
use std::sync::Arc;

/// Ten properties, so the presence mask spans two bytes; `label` is bounded text.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct RowsFixtureItem {
    pub weight: f32,
    pub count: u32,
    pub offset: Option<[f32; 3]>,
    #[schema(rotation)]
    pub rotation: Option<[f32; 4]>,
    pub enabled: bool,
    pub texture: Option<AssetSource>,
    pub delta: i32,
    pub size: Option<[f32; 2]>,
    pub mark: Option<f32>,
    #[schema(text = 16)]
    pub label: Option<Arc<str>>,
}

#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct RowsFixtureTag {
    pub value: u32,
}

#[repr(C)]
#[derive(Debug, Default, PartialEq, SchemaComponent)]
pub struct RowsFixture {
    pub marker: u32,
    #[schema(rows)]
    pub items: Rows<RowsFixtureItem>,
    #[schema(rows)]
    pub tags: Rows<RowsFixtureTag>,
}

thread_local! {
    static CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl RowsFixture {
    /// Whole-value copies made on this thread since the previous call, so
    /// staging tests can show that row writes do not copy per write.
    pub fn take_clone_count() -> usize {
        CLONES.with(|clones| clones.replace(0))
    }
}

impl Clone for RowsFixture {
    fn clone(&self) -> Self {
        CLONES.with(|clones| clones.set(clones.get() + 1));
        Self {
            marker: self.marker,
            items: self.items.clone(),
            tags: self.tags.clone(),
        }
    }
}

impl RowsFixture {
    /// Property index of `mark` in [`RowsFixtureItem`] layout order.
    pub const MARK: u32 = 8;

    /// Item row property addressed by `offset`, if any.
    fn item_property(offset: u32) -> Option<u32> {
        let relative = super::row_region_relative(offset, 0)?;
        super::row_address(relative, RowsFixtureItem::LAYOUT.property_count())
            .map(|address| address.property)
    }
}

/// Item row properties take numeric animation through the compiled row
/// destination, and `mark` accepts only values in `0..=1` there; the `tags`
/// table is owned by its writer and is never an animation target.
impl crate::components::schema::ComponentLifecycle for RowsFixture {
    fn animatable_field(offset: u32) -> bool {
        super::row_region(offset).is_none_or(|(field, _)| field == 0)
    }

    fn supports_numeric_property(offset: u32) -> bool {
        Self::item_property(offset).is_some()
    }

    fn validate_numeric_properties(
        &self,
        fields: &[(u32, crate::components::schema::FieldValue)],
    ) -> Result<(), crate::ErrorReason> {
        use crate::DynamicValue;
        use crate::components::schema::FieldValue;

        for (offset, value) in fields {
            let property = Self::item_property(*offset).ok_or(crate::ErrorReason::InvalidField)?;
            if property == Self::MARK
                && let FieldValue::Dynamic(DynamicValue::F32(mark)) = value
                && !(0.0..=1.0).contains(mark)
            {
                return Err(crate::ErrorReason::InvalidValue);
            }
        }
        Ok(())
    }

    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::AssetDemandSelection,
        >,
    ) {
        self.items.visit_assets(&mut |asset| {
            if !asset.uri.is_empty() {
                crate::services::asset_management::AssetDemandSelection::insert_into(
                    demand,
                    asset.kind,
                    &asset.uri,
                    asset.variant,
                );
            }
        });
    }
}
