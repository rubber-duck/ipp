//! Codec bounds resolved from the executed target contract at generation time.

use std::fmt::Write as _;

use crate::model::WireContract;

/// Layout field bounds the codec enforces, as `(constant, layout, field, exported)`.
const FIELD_LIMITS: &[(&str, &str, &str, bool)] = &[
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

/// GUI payload bounds, present exactly when the target compiles GUI.
const GUI_FIELD_LIMITS: &[(&str, &str, &str, bool)] = &[
    ("GUI_EDITS_BYTES", "request-gui", "edits", false),
    ("GUI_INPUT_BYTES", "request-gui-input", "input", false),
    (
        "GUI_ACTION_BYTES",
        "request-gui-semantic-action",
        "action",
        false,
    ),
    (
        "GUI_INSPECT_BYTES",
        "response-gui-inspect",
        "payload",
        false,
    ),
    (
        "GUI_SNAPSHOT_BYTES",
        "response-gui-semantic-snapshot",
        "snapshot",
        false,
    ),
    (
        "GUI_OBSERVATION_BYTES",
        "response-gui-observations",
        "observations",
        false,
    ),
    (
        "GUI_UNHANDLED_BYTES",
        "response-gui-unhandled",
        "unhandled",
        false,
    ),
];

/// Every constant name this module emits, reserved against component names.
pub(super) const CODEC_LIMIT_NAMES: &[&str] = &[
    "MAX_MESSAGE_BYTES",
    "HOST_REQUEST_MAGIC",
    "HOST_RESPONSE_MAGIC",
    "INSPECTED_BYTES_LIMIT",
    "ROWS_VALUE_BYTES",
    "INSPECTED_ROWS_LIMIT",
    "RESOURCE_ERROR_BYTES",
    "GUI_EDITS_BYTES",
    "GUI_INPUT_BYTES",
    "GUI_ACTION_BYTES",
    "GUI_INSPECT_BYTES",
    "GUI_SNAPSHOT_BYTES",
    "GUI_OBSERVATION_BYTES",
    "GUI_UNHANDLED_BYTES",
];

/// Emit the message budget, host magics and field bounds the codec reads.
///
/// A missing or invalid bound fails generation instead of the shipped client.
pub(super) fn render(out: &mut String, wire: &WireContract, gui: bool) -> Result<(), String> {
    let message = convention(wire, "max-message-bytes")?
        .parse::<u32>()
        .ok()
        .filter(|bytes| *bytes > 0)
        .ok_or("target contract declares an invalid message budget")?;
    out.push_str("/** Complete application message budget declared by the target contract. */\n");
    writeln!(out, "export const MAX_MESSAGE_BYTES = {message};").unwrap();

    for (name, convention_name) in [
        ("HOST_REQUEST_MAGIC", "host-request-magic"),
        ("HOST_RESPONSE_MAGIC", "host-response-magic"),
    ] {
        let bytes = magic(convention(wire, convention_name)?)?;
        writeln!(out, "const {name} = [{bytes}] as const;").unwrap();
    }

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
    Ok(())
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
            render(&mut String::new(), &wire, false).unwrap_err(),
            "target contract omits the host-request-magic convention"
        );

        wire.conventions[0].1 = "0".into();
        assert!(render(&mut String::new(), &wire, false).is_err());
    }
}
