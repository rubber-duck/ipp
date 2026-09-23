//! Registered test-only component with two schema rows fields, exercising
//! region dispatch, registry access, staging and persistence without a
//! production consumer.
#![allow(missing_docs)]
use super::rows::{Rows, SchemaRow};
use crate::services::asset_management::AssetSource;
use ipp_schema_derive::SchemaComponent;

/// Nine properties, so the presence mask spans two bytes.
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
}

#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct RowsFixtureTag {
    pub value: u32,
}

#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct RowsFixture {
    pub marker: u32,
    #[schema(rows)]
    pub items: Rows<RowsFixtureItem>,
    #[schema(rows)]
    pub tags: Rows<RowsFixtureTag>,
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
