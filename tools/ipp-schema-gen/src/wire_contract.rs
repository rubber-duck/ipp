use crate::binary_reader::Reader;
use crate::model::{
    AssetFormat, Capabilities, Component, TargetFeature, WireContract, WireEncoding, WireField,
    WireLayout, WireTag,
};
use crate::typescript_names::identifier;

pub(super) fn read_wire_contract(
    r: &mut Reader<'_>,
    components: &[Component],
    target_features: &[TargetFeature],
) -> Result<WireContract, String> {
    if r.u16()? != 1 {
        return Err("wire contract format".into());
    }

    let mut capabilities = Capabilities::default();
    let mut capability_ids = std::collections::BTreeSet::new();
    let mut capability_names = std::collections::BTreeSet::new();
    for _ in 0..r.u8()? {
        let id = r.u8()?;
        let enabled = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err("wire capability flag".into()),
        };
        let name = r.string()?;
        if !capability_ids.insert(id) || !capability_names.insert(name.clone()) {
            return Err("duplicate wire capability".into());
        }
        if name == "builtin-assets" {
            if !target_features
                .iter()
                .any(|feature| feature.name == name && feature.enabled == enabled)
            {
                return Err("wire and target capability selection differ".into());
            }
        } else if !enabled {
            return Err("baseline wire capability is disabled".into());
        }
        match (id, name.as_str()) {
            (1, "state-overlays") => capabilities.state_overlays = enabled,
            (2, "spatial") => capabilities.spatial = enabled,
            (3, "textures") => capabilities.textures = enabled,
            (4, "builtin-assets") => capabilities.builtin_assets = enabled,
            (5, "picking") => capabilities.picking = enabled,
            (6, "debug-geometry") => capabilities.debug_geometry = enabled,
            (7, "animation") => capabilities.animation = enabled,
            (8, "assets") => capabilities.assets = enabled,
            _ => return Err("unknown wire capability".into()),
        }
    }
    if capability_ids != std::collections::BTreeSet::from([1, 2, 3, 4, 5, 6, 7, 8])
        || capabilities.assets != (capabilities.spatial || capabilities.animation)
        || capabilities.debug_geometry && !capabilities.spatial
        || capabilities.builtin_assets && !capabilities.textures
        || capabilities.textures && !capabilities.spatial
        || capabilities.picking && !capabilities.spatial
    {
        return Err("incomplete wire capabilities".into());
    }

    let mut conventions = Vec::new();
    let mut convention_names = std::collections::BTreeSet::new();
    for _ in 0..r.u16()? {
        let name = r.string()?;
        let value = r.string()?;
        if name.is_empty() || value.is_empty() || !convention_names.insert(name.clone()) {
            return Err("invalid wire convention".into());
        }
        conventions.push((name, value));
    }

    let mut layouts = Vec::new();
    let mut layout_names = std::collections::BTreeSet::new();
    for _ in 0..r.u16()? {
        let name = r.string()?;
        let capability = r.u8()?;
        validate_capability(capability, capabilities)?;
        if name.is_empty() || !layout_names.insert(name.clone()) {
            return Err("duplicate wire layout".into());
        }

        let mut fields = Vec::new();
        let mut field_names = std::collections::BTreeSet::new();
        for _ in 0..r.u16()? {
            let name = r.string()?;
            let encoding = WireEncoding::parse(r.u8()?)?;
            let limit = r.u32()?;
            let target = r.string()?;
            if name.is_empty() || !field_names.insert(name.clone()) {
                return Err("duplicate wire field".into());
            }
            if !encoding.valid_limit(limit) || encoding.has_target() != !target.is_empty() {
                return Err("invalid wire field encoding".into());
            }
            fields.push(WireField {
                name,
                encoding,
                limit,
                target,
            });
        }
        layouts.push(WireLayout {
            name,
            capability,
            fields,
        });
    }
    for layout in &layouts {
        for field in &layout.fields {
            let valid = match field.encoding {
                WireEncoding::Masked => {
                    field.target == "bool" || layout_names.contains(&field.target)
                }
                WireEncoding::Named => layout_names.contains(&field.target),
                WireEncoding::List => {
                    layout_names.contains(&field.target)
                        || is_tag_space_name(&field.target)
                        || ["u32", "utf8-65536", "empty"].contains(&field.target.as_str())
                }
                WireEncoding::Option => {
                    ["bool", "u16", "u32", "u64", "utf8-65536"].contains(&field.target.as_str())
                        || layout_names.contains(&field.target)
                }
                WireEncoding::Variant | WireEncoding::Union => is_tag_space_name(&field.target),
                _ => field.target.is_empty(),
            };
            if !valid {
                return Err(format!(
                    "wire field references unknown layout or tag space: {}.{} -> {}",
                    layout.name, field.name, field.target
                ));
            }
        }
    }

    let mut tags = Vec::new();
    let mut tag_names = std::collections::BTreeSet::new();
    let mut tag_values = std::collections::BTreeSet::new();
    for _ in 0..r.u16()? {
        let name = r.string()?;
        identifier(&name)?;
        let space = r.u8()?;
        tag_space_name(space)?;
        let capability = r.u8()?;
        validate_capability(capability, capabilities)?;
        let value = r.u8()?;
        let layout = r.string()?;
        if !tag_names.insert(name.clone()) || !tag_values.insert((space, value)) {
            return Err("duplicate wire tag".into());
        }
        if !["empty", "present"].contains(&layout.as_str()) && !layout_names.contains(&layout) {
            return Err("wire tag references unknown layout".into());
        }
        tags.push(WireTag {
            name,
            space,
            capability,
            value,
            layout,
        });
    }
    for tag in &tags {
        if !["empty", "present"].contains(&tag.layout.as_str()) {
            let space = tag_space_name(tag.space)?;
            let layout = layouts
                .iter()
                .find(|layout| layout.name == tag.layout)
                .unwrap();
            if !layout
                .fields
                .iter()
                .any(|field| field.encoding == WireEncoding::Variant && field.target == space)
            {
                return Err("wire tag layout has no matching discriminant".into());
            }
        }
    }

    for field in components.iter().flat_map(|component| &component.fields) {
        for prefix in ["VALUE_", "SNAPSHOT_VALUE_"] {
            if !tags
                .iter()
                .any(|tag| tag.name.starts_with(prefix) && tag.value == field.kind)
            {
                return Err("component field kind has no wire codec".into());
            }
        }
    }

    let mut asset_formats = Vec::new();
    for _ in 0..r.u16()? {
        let name = r.string()?;
        identifier(&name)?;
        let capability = r.u8()?;
        validate_capability(capability, capabilities)?;
        let type_id = r.u16()?;
        let format = r.string()?;
        if type_id == 0
            || format.is_empty()
            || tag_names.contains(&name)
            || asset_formats
                .iter()
                .any(|existing: &AssetFormat| existing.name == name || existing.type_id == type_id)
        {
            return Err("invalid asset format".into());
        }
        asset_formats.push(AssetFormat {
            name,
            capability,
            type_id,
            format,
        });
    }

    let mut reasons = std::collections::BTreeSet::new();
    for _ in 0..r.u16()? {
        let reason = r.string()?;
        identifier(&reason)?;
        if !reasons.insert(reason) {
            return Err("duplicate error reason".into());
        }
    }

    Ok(WireContract {
        capabilities,
        conventions,
        layouts,
        tags,
        asset_formats,
    })
}

fn validate_capability(id: u8, capabilities: Capabilities) -> Result<(), String> {
    let enabled = match id {
        0 => true,
        1 => capabilities.state_overlays,
        2 => capabilities.spatial,
        3 => capabilities.textures,
        4 => capabilities.builtin_assets,
        5 => capabilities.picking,
        6 => capabilities.debug_geometry,
        7 => capabilities.animation,
        8 => capabilities.assets,
        _ => return Err("unknown wire capability reference".into()),
    };
    if enabled {
        Ok(())
    } else {
        Err("disabled capability leaked into wire contract".into())
    }
}

pub(super) fn capability_name(id: u8) -> Result<&'static str, String> {
    match id {
        0 => Ok("base"),
        1 => Ok("state-overlays"),
        2 => Ok("spatial"),
        3 => Ok("textures"),
        4 => Ok("builtin-assets"),
        5 => Ok("picking"),
        6 => Ok("debug-geometry"),
        7 => Ok("animation"),
        8 => Ok("assets"),
        _ => Err("unknown wire capability reference".into()),
    }
}

fn tag_space_name(id: u8) -> Result<&'static str, String> {
    match id {
        1 => Ok("value"),
        2 => Ok("request"),
        3 => Ok("command"),
        4 => Ok("reference"),
        5 => Ok("response"),
        6 => Ok("outcome"),
        8 => Ok("option"),
        9 => Ok("entity-overlay-mode"),
        10 => Ok("component-overlay-mode"),
        11 => Ok("state-overlay-handle-kind"),
        12 => Ok("state-overlay-lifecycle-reason"),
        13 => Ok("resource-status"),
        15 => Ok("snapshot-value"),
        16 => Ok("snapshot-reference"),
        17 => Ok("camera-motion"),
        18 => Ok("geometry-pick-outcome"),
        19 => Ok("lifecycle-observation"),
        20 => Ok("batch-error-scope"),
        21 => Ok("runtime-failure-scope"),
        22 => Ok("inspection-collection"),
        23 => Ok("host-request"),
        24 => Ok("host-response"),
        25 => Ok("world-selector"),
        26 => Ok("animation-target"),
        27 => Ok("playback-control"),
        28 => Ok("playback-state"),
        29 => Ok("playback-event-kind"),
        30 => Ok("animation-transition-easing"),
        31 => Ok("animation-transition-start-time"),
        _ => Err("unknown wire tag space".into()),
    }
}

fn is_tag_space_name(name: &str) -> bool {
    (1..=31).any(|id| tag_space_name(id).is_ok_and(|candidate| candidate == name))
}

impl WireEncoding {
    fn parse(value: u8) -> Result<Self, String> {
        Ok(match value {
            2 => Self::U16,
            3 => Self::U32,
            4 => Self::U64,
            5 => Self::FiniteF32,
            6 => Self::NonnegativeFiniteF64,
            7 => Self::Utf8,
            8 => Self::Bytes,
            9 => Self::Named,
            10 => Self::List,
            11 => Self::Option,
            12 => Self::Variant,
            13 => Self::Union,
            14 => Self::Bool,
            15 => Self::Masked,
            _ => return Err("unknown wire field encoding".into()),
        })
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::FiniteF32 => "finite-f32",
            Self::NonnegativeFiniteF64 => "nonnegative-finite-f64",
            Self::Utf8 => "utf8",
            Self::Bytes => "bytes",
            Self::Named => "named",
            Self::List => "list",
            Self::Option => "option",
            Self::Variant => "variant",
            Self::Union => "union",
            Self::Bool => "bool",
            Self::Masked => "masked",
        }
    }

    const fn valid_limit(self, limit: u32) -> bool {
        match self {
            Self::Utf8 | Self::Bytes => limit != 0,
            Self::List => true,
            Self::Masked => limit.is_power_of_two() && limit <= 0x8000,
            _ => limit == 0,
        }
    }

    const fn has_target(self) -> bool {
        matches!(
            self,
            Self::Named | Self::List | Self::Option | Self::Variant | Self::Union | Self::Masked
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{is_tag_space_name, tag_space_name};

    #[test]
    fn animation_transition_tag_spaces_are_known_by_id_and_name() {
        assert_eq!(tag_space_name(30).unwrap(), "animation-transition-easing");
        assert_eq!(
            tag_space_name(31).unwrap(),
            "animation-transition-start-time"
        );
        assert!(is_tag_space_name("animation-transition-easing"));
        assert!(is_tag_space_name("animation-transition-start-time"));
        assert!(tag_space_name(7).is_err());
        assert!(tag_space_name(32).is_err());
        assert!(!is_tag_space_name("animation-transition-unknown"));
    }
}
