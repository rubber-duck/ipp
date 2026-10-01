//! Codec bounds resolved from the executed target contract at generation time.

use std::fmt::Write as _;

use crate::model::{RowLimits, WireContract};

/// Layout field bounds the codec enforces, as `(constant, layout, field, exported)`.
const FIELD_LIMITS: &[(&str, &str, &str, bool)] = &[
    ("LIFECYCLE_MEMBERS", "lifecycle-watch-add", "members", false),
    (
        "LIFECYCLE_VALUE_FIELDS",
        "lifecycle-watch-value",
        "fields",
        false,
    ),
    ("ATTACHMENT_EFFECTS", "response-batch", "effects", false),
    ("FIELD_BYTES", "value-string", "value", false),
    ("METADATA_CLASSES", "metadata", "classes", false),
    ("INSERT_FIELDS", "command-insert", "fields", false),
    (
        "LIFECYCLE_PUBLICATIONS",
        "response-lifecycle-events",
        "events",
        false,
    ),
    (
        "RESOURCE_EVENT_RECORDS",
        "response-resources",
        "resources",
        false,
    ),
    ("PLAYBACK_EVENTS", "response-playback", "events", false),
    (
        "FAILURE_MESSAGE_BYTES",
        "response-batch-aborted",
        "message",
        false,
    ),
    ("INSPECTION_PAGE", "response-inspect", "entities", false),
    ("INSPECTED_COMPONENTS", "entity", "components", false),
    ("INSPECTED_FIELDS", "component", "fields", false),
    ("ENTITY_TREE_PAGE", "response-entity-tree", "nodes", false),
    (
        "ANIMATION_TARGET_INDICES",
        "animation-target-property",
        "indices",
        false,
    ),
    (
        "GRAPH_METADATA_PAGE",
        "host-response-world-graph-page",
        "nodes",
        false,
    ),
    (
        "GRAPH_BINDING_PAGE",
        "host-response-world-graph-bindings",
        "bindings",
        false,
    ),
    // Inspected byte fields, including named-property descriptor tables.
    (
        "INSPECTED_BYTES_LIMIT",
        "snapshot-value-bytes",
        "value",
        true,
    ),
    // Whole schema rows tables written by insertion or inspected by snapshots.
    ("ROWS_VALUE_BYTES", "value-rows", "value", false),
    (
        "INSPECTED_ROWS_LIMIT",
        "snapshot-value-rows",
        "value",
        false,
    ),
    (
        "RESOURCE_ERROR_BYTES",
        "resource-status-failed",
        "error",
        false,
    ),
];

/// Named contract limits no layout field carries, as `(constant, limit, exported)`.
const CONTRACT_LIMITS: &[(&str, &str, bool)] = &[
    ("ENTITY_TREE_DEPTH", "entity-tree-depth", false),
    ("MAX_MESH_VERTICES", "mesh-vertices", true),
    ("MAX_JOINTS", "skeleton-joints", true),
];

/// Physical input bounds the maintained Host client reads by name, present exactly when
/// the target compiles GUI, as `(name, layout, field)`.
const HOST_LIMITS: &[(&str, &str, &str)] = &[
    (
        "GUI_PHYSICAL_POINTERS",
        "gui-physical-cancelled",
        "pointers",
    ),
    ("GUI_PHYSICAL_BLOCKERS", "gui-physical-open", "blockers"),
];

/// GUI payload bounds, present exactly when the target compiles GUI.
const GUI_FIELD_LIMITS: &[(&str, &str, &str, bool)] = &[
    (
        "GUI_OBSERVATION_CONTROL_BYTES",
        "request-gui-observation",
        "control",
        false,
    ),
    (
        "GUI_OBSERVATION_BYTES",
        "response-gui-observation",
        "record",
        false,
    ),
];

/// Every constant name this module emits, reserved against component names.
pub(super) const CODEC_LIMIT_NAMES: &[&str] = &[
    "LIFECYCLE_MEMBERS",
    "LIFECYCLE_VALUE_FIELDS",
    "LIFECYCLE_PREFERRED_PAGE_MEMBERS",
    "MAX_MESSAGE_BYTES",
    "COMMAND_PAGE_LIMITS",
    "BATCH_OUTCOME_ALIASES",
    "ATTACHMENT_EFFECTS",
    "FIELD_BYTES",
    "METADATA_CLASSES",
    "INSERT_FIELDS",
    "LIFECYCLE_PUBLICATIONS",
    "RESOURCE_EVENT_RECORDS",
    "PLAYBACK_EVENTS",
    "FAILURE_MESSAGE_BYTES",
    "INSPECTION_PAGE",
    "INSPECTED_COMPONENTS",
    "INSPECTED_FIELDS",
    "ENTITY_TREE_PAGE",
    "ANIMATION_TARGET_INDICES",
    "ENTITY_TREE_DEPTH",
    "MAX_MESH_VERTICES",
    "MAX_JOINTS",
    "ROW_REGION_SPAN",
    "ROW_PROPERTIES",
    "HOST_LIMITS",
    "GRAPH_METADATA_PAGE",
    "GRAPH_BINDING_PAGE",
    "HOST_REQUEST_MAGIC",
    "HOST_RESPONSE_MAGIC",
    "INSPECTED_BYTES_LIMIT",
    "ROWS_VALUE_BYTES",
    "INSPECTED_ROWS_LIMIT",
    "RESOURCE_ERROR_BYTES",
    "GUI_OBSERVATION_CONTROL_BYTES",
    "GUI_OBSERVATION_BYTES",
];

/// Emit the message budget, host magics and field bounds the codec reads.
///
/// A missing or invalid bound fails generation instead of the shipped client.
pub(super) fn render(
    out: &mut String,
    wire: &WireContract,
    rows: &RowLimits,
    gui: bool,
) -> Result<(), String> {
    let message = convention(wire, "max-message-bytes")?
        .parse::<u32>()
        .ok()
        .filter(|bytes| *bytes > 0)
        .ok_or("target contract declares an invalid message budget")?;
    out.push_str("/** Complete application message budget declared by the target contract. */\n");
    writeln!(out, "export const MAX_MESSAGE_BYTES = {message};").unwrap();
    let lifecycle_page = convention(wire, "lifecycle-preferred-page-members")?
        .parse::<u32>()
        .ok()
        .filter(|members| {
            *members > 0
                && *members <= field_limit(wire, "lifecycle-watch-add", "members").unwrap_or(0)
        })
        .ok_or("target contract declares an invalid lifecycle page budget")?;
    writeln!(
        out,
        "const LIFECYCLE_PREFERRED_PAGE_MEMBERS = {lifecycle_page};"
    )
    .unwrap();

    for (name, convention_name) in [
        ("HOST_REQUEST_MAGIC", "host-request-magic"),
        ("HOST_RESPONSE_MAGIC", "host-response-magic"),
    ] {
        let bytes = magic(convention(wire, convention_name)?)?;
        writeln!(out, "const {name} = [{bytes}] as const;").unwrap();
    }

    let page_bytes = convention(wire, "command-page-bytes")?
        .parse::<u32>()
        .ok()
        .filter(|bytes| *bytes > 0 && *bytes <= message)
        .ok_or("target contract declares an invalid command page byte limit")?;
    let page_commands = field_limit(wire, "request-submit-batch", "operations")?;
    let outcome_aliases = convention(wire, "batch-outcome-aliases")?
        .parse::<u32>()
        .ok()
        .filter(|aliases| *aliases > 0)
        .ok_or("target contract declares an invalid batch outcome alias bound")?;
    writeln!(out, "const BATCH_OUTCOME_ALIASES = {outcome_aliases};").unwrap();
    out.push_str("/** Batch page bounds declared by the target contract. */\n");
    writeln!(
        out,
        "export const COMMAND_PAGE_LIMITS = {{ commands: {page_commands}, bytes: {page_bytes} }} as const;"
    )
    .unwrap();
    let gui_limits = if gui {
        GUI_FIELD_LIMITS
    } else {
        &[]
    };
    for &(name, layout, field, exported) in FIELD_LIMITS.iter().chain(gui_limits) {
        let limit = field_limit(wire, layout, field)?;
        let export = if exported {
            "export "
        } else {
            ""
        };
        writeln!(out, "{export}const {name} = {limit};").unwrap();
    }

    for &(name, limit, exported) in CONTRACT_LIMITS {
        let value = contract_limit(wire, limit)?;
        let export = if exported {
            "export "
        } else {
            ""
        };
        writeln!(out, "{export}const {name} = {value};").unwrap();
    }
    writeln!(out, "const ROW_REGION_SPAN = {};", rows.region_span).unwrap();
    writeln!(out, "const ROW_PROPERTIES = {};", rows.properties).unwrap();

    out.push_str("/** Host protocol bounds the maintained Host client reads by name. */\n");
    out.push_str("const HOST_LIMITS: Readonly<Record<string, number>> = Object.freeze({");
    if gui {
        for &(name, layout, field) in HOST_LIMITS {
            write!(out, " {name}: {},", field_limit(wire, layout, field)?).unwrap();
        }
    }
    out.push_str(" });\n");
    Ok(())
}

fn contract_limit(wire: &WireContract, name: &str) -> Result<u32, String> {
    wire.limits
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("target contract omits the {name} limit"))
}

fn convention<'a>(wire: &'a WireContract, name: &str) -> Result<&'a str, String> {
    wire.conventions
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| format!("target contract omits the {name} convention"))
}

fn field_limit(wire: &WireContract, layout: &str, field: &str) -> Result<u32, String> {
    wire.layouts
        .iter()
        .find(|candidate| candidate.name == layout)
        .and_then(|layout| {
            layout
                .fields
                .iter()
                .find(|candidate| candidate.name == field)
        })
        .map(|field| field.limit)
        .filter(|limit| *limit > 0)
        .ok_or_else(|| format!("target contract omits the {layout}.{field} bound"))
}

/// Comma-separated bytes of an eight-byte lowercase hexadecimal magic.
fn magic(hex: &str) -> Result<String, String> {
    if hex.len() != 16 || !hex.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("target contract declares an invalid host magic".into());
    }

    Ok((0..8)
        .map(|i| {
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Capabilities, WireEncoding, WireField, WireLayout};

    const ROWS: RowLimits = RowLimits {
        region_span: 0x1000_0000,
        fields: 7,
        properties: 256,
        text_bytes: 65_536,
    };

    #[test]
    fn bounds_resolve_from_the_contract_or_fail_generation() {
        assert_eq!(
            magic("4950504801000000").unwrap(),
            "73, 80, 80, 72, 1, 0, 0, 0"
        );
        for invalid in ["49505048010000", "4950504801000G00", "4950504801000A00"] {
            assert!(magic(invalid).is_err());
        }

        let mut wire = WireContract {
            capabilities: Capabilities::default(),
            conventions: vec![("max-message-bytes".into(), "1048576".into())],
            limits: Vec::new(),
            layouts: vec![WireLayout {
                name: "value-rows".into(),
                capability: 0,
                fields: vec![WireField {
                    name: "value".into(),
                    encoding: WireEncoding::Bytes,
                    limit: 4096,
                    target: String::new(),
                }],
            }],
            tags: Vec::new(),
            asset_formats: Vec::new(),
        };
        assert_eq!(field_limit(&wire, "value-rows", "value").unwrap(), 4096);
        assert!(field_limit(&wire, "value-rows", "other").is_err());
        assert!(field_limit(&wire, "snapshot-value-rows", "value").is_err());
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract omits the lifecycle-preferred-page-members convention"
        );

        wire.conventions
            .push(("lifecycle-preferred-page-members".into(), "2".into()));
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract declares an invalid lifecycle page budget"
        );
        wire.layouts.push(WireLayout {
            name: "lifecycle-watch-add".into(),
            capability: 0,
            fields: vec![WireField {
                name: "members".into(),
                encoding: WireEncoding::List,
                limit: 2,
                target: "lifecycle-watch-selection".into(),
            }],
        });
        for invalid in ["0", "3", "invalid"] {
            wire.conventions[1].1 = invalid.into();
            assert_eq!(
                render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
                "target contract declares an invalid lifecycle page budget"
            );
        }

        wire.conventions[1].1 = "2".into();
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract omits the host-request-magic convention"
        );

        wire.conventions.extend([
            ("host-request-magic".into(), "4950504801000000".into()),
            ("host-response-magic".into(), "4950504802000000".into()),
        ]);
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract omits the command-page-bytes convention"
        );
        wire.conventions
            .push(("command-page-bytes".into(), "2097152".into()));
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract declares an invalid command page byte limit"
        );
        wire.conventions.last_mut().unwrap().1 = "262144".into();
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract omits the request-submit-batch.operations bound"
        );
        wire.layouts.push(WireLayout {
            name: "request-submit-batch".into(),
            capability: 0,
            fields: vec![WireField {
                name: "operations".into(),
                encoding: WireEncoding::List,
                limit: 1024,
                target: "command".into(),
            }],
        });
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract omits the batch-outcome-aliases convention"
        );

        wire.conventions[0].1 = "0".into();
        assert_eq!(
            render(&mut String::new(), &wire, &ROWS, false).unwrap_err(),
            "target contract declares an invalid message budget"
        );
    }
}
