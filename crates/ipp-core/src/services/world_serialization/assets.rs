//! Validate typed resource reference declarations without acquiring their contents.

use crate::services::asset_management::{AssetSource, AssetTypeId};
use crate::{ComponentValue, components::schema::FieldValue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn component_sources(
    component: &ComponentValue,
) -> Result<BTreeSet<AssetSource>, String> {
    let fields: BTreeMap<_, _> = component.fields().into_iter().collect();
    let mut sources = BTreeSet::new();
    for reference in ComponentValue::asset_references(component.type_id()) {
        let Some(FieldValue::String(source)) = fields.get(&reference.source_offset) else {
            return Err("Invalid typed asset source declaration".into());
        };
        let Some(FieldValue::U32(variant)) = fields.get(&reference.variant_offset) else {
            return Err("Invalid typed asset variant declaration".into());
        };
        if !source.is_empty() {
            sources.insert(AssetSource {
                kind: AssetTypeId(reference.kind),
                uri: source.clone(),
                variant: *variant,
            });
        }
    }
    for value in fields.values() {
        if let FieldValue::Dynamic(crate::DynamicValue::Asset(asset)) = value
            && !asset.uri.is_empty()
        {
            sources.insert(AssetSource {
                kind: asset.kind,
                uri: asset.uri.clone(),
                variant: asset.variant,
            });
        }
    }
    {
        let mut demand = BTreeSet::new();
        component.resource_demand(&mut demand);
        let demand: BTreeSet<_> = demand
            .into_iter()
            .map(|selection| selection.descriptor())
            .collect();
        if sources != demand {
            return Err(format!(
                "Component {} has undeclared persistent asset references",
                component.type_id()
            ));
        }
    }
    Ok(sources)
}
