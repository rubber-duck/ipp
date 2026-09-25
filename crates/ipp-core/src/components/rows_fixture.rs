//! Registered test-only component with two schema rows fields, exercising
//! region dispatch, registry access, staging and persistence without a
//! production consumer.
#![allow(missing_docs)]
use super::rows::{Rows, SchemaRow};
use crate::services::asset_management::AssetSource;
use ipp_schema_derive::SchemaComponent;

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
    pub label: Option<String>,
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

impl crate::components::schema::ComponentLifecycle for RowsFixture {
    fn resource_demand(
        &self,
        demand: &mut std::collections::BTreeSet<
            crate::services::asset_management::service::AssetDemandSelection,
        >,
    ) {
        self.items.resource_demand(demand);
    }
}
