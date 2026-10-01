use crate::binary_reader::Reader;
use crate::model::{
    AssetFormat, Component, WireContract, WireEncoding, WireField, WireLayout, WireTag,
};
use crate::typescript_names::identifier;

pub(super) fn read_wire_contract(
    r: &mut Reader<'_>,
    components: &[Component],
) -> Result<WireContract, String> {
    if r.u16()? != 3 {
        return Err("wire contract format".into());
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

    let mut limits = Vec::new();
    let mut limit_names = std::collections::BTreeSet::new();
    for _ in 0..r.u16()? {
        let name = r.string()?;
        let value = r.u32()?;
        if name.is_empty() || value == 0 || !limit_names.insert(name.clone()) {
            return Err("invalid wire limit".into());
        }
        limits.push((name, value));
    }

    let mut layouts = Vec::new();
    let mut layout_names = std::collections::BTreeSet::new();
    for _ in 0..r.u16()? {
        let name = r.string()?;
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
                WireEncoding::List | WireEncoding::U8CountedList => {
                    layout_names.contains(&field.target)
                        || is_tag_space_name(&field.target)
                        || ["u8", "u16", "u32", "u64", "utf8-65536", "empty"]
                            .contains(&field.target.as_str())
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
        conventions,
        limits,
        layouts,
        tags,
        asset_formats,
    })
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
        13 => Ok("resource-status"),
        15 => Ok("snapshot-value"),
        16 => Ok("snapshot-reference"),
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
        32 => Ok("output-kind"),
        33 => Ok("view-target"),
        34 => Ok("operation-effect"),
        35 => Ok("attachment-receipt-state"),
        36 => Ok("presentation-request"),
        37 => Ok("presentation-response"),
        38 => Ok("presentation-error"),
        39 => Ok("lifecycle-watch-change"),
        40 => Ok("lifecycle-watch-target"),
        41 => Ok("lifecycle-watch-record"),
        42 => Ok("lifecycle-membership-result"),
        43 => Ok("lifecycle-target-lifetime"),
        44 => Ok("lifecycle-membership-rejection"),
        45 => Ok("lifecycle-watch-kinds"),
        46 => Ok("gui-physical-request"),
        47 => Ok("gui-physical-event"),
        48 => Ok("gui-physical-response"),
        49 => Ok("gui-physical-button"),
        50 => Ok("gui-physical-key"),
        51 => Ok("gui-native-edit"),
        52 => Ok("gui-physical-disposition"),
        53 => Ok("gui-action"),
        54 => Ok("output-target"),
        _ => Err("unknown wire tag space".into()),
    }
}

fn is_tag_space_name(name: &str) -> bool {
    (1..=54).any(|id| tag_space_name(id).is_ok_and(|candidate| candidate == name))
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
            16 => Self::U8CountedList,
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
            Self::U8CountedList => "u8-counted-list",
        }
    }

    const fn valid_limit(self, limit: u32) -> bool {
        match self {
            Self::Utf8 | Self::Bytes => limit != 0,
            Self::List => true,
            Self::U8CountedList => limit <= u8::MAX as u32,
            Self::Masked => limit.is_power_of_two() && limit <= 0x8000,
            _ => limit == 0,
        }
    }

    const fn has_target(self) -> bool {
        matches!(
            self,
            Self::Named
                | Self::List
                | Self::U8CountedList
                | Self::Option
                | Self::Variant
                | Self::Union
                | Self::Masked
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
        assert_eq!(tag_space_name(32).unwrap(), "output-kind");
        assert_eq!(tag_space_name(33).unwrap(), "view-target");
        assert!(is_tag_space_name("view-target"));
        assert_eq!(tag_space_name(36).unwrap(), "presentation-request");
        assert_eq!(tag_space_name(37).unwrap(), "presentation-response");
        assert_eq!(tag_space_name(38).unwrap(), "presentation-error");
        for (tag, name) in [
            (39, "lifecycle-watch-change"),
            (40, "lifecycle-watch-target"),
            (41, "lifecycle-watch-record"),
            (42, "lifecycle-membership-result"),
            (43, "lifecycle-target-lifetime"),
            (44, "lifecycle-membership-rejection"),
            (45, "lifecycle-watch-kinds"),
        ] {
            assert_eq!(tag_space_name(tag).unwrap(), name);
            assert!(is_tag_space_name(name));
        }

        assert_eq!(tag_space_name(53).unwrap(), "gui-action");
        assert_eq!(tag_space_name(54).unwrap(), "output-target");
        assert!(is_tag_space_name("output-target"));
        assert!(tag_space_name(55).is_err());
        assert!(!is_tag_space_name("animation-transition-unknown"));
        assert!(!is_tag_space_name("lifecycle-watch-unknown"));
    }
}
