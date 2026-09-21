use std::fmt::Write as _;

/// Reserve actual maintained template bindings, including imports and re-exports.
fn template_identifiers() -> &'static std::collections::BTreeSet<String> {
    static NAMES: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = std::collections::BTreeSet::new();
        for template in [
            include_str!("codec.template.ts"),
            include_str!("animation.template.ts"),
            include_str!("geometry.template.ts"),
            include_str!("skeleton.template.ts"),
            include_str!("skinning.template.ts"),
        ] {
            let mut bindings = String::new();
            for line in template.lines() {
                if !bindings.is_empty()
                    || line.starts_with("import {")
                    || line.starts_with("import type {")
                    || line.starts_with("export {")
                    || line.starts_with("export type {")
                {
                    bindings.push_str(line);
                    bindings.push(' ');
                    if let Some(end) = bindings.find('}') {
                        let start = bindings.find('{').unwrap() + 1;
                        for entry in bindings[start..end].split(',') {
                            if let Some(name) = entry.split_whitespace().last() {
                                names.insert(name.to_owned());
                            }
                        }
                        bindings.clear();
                    }
                    continue;
                }
                if line.starts_with(char::is_whitespace) {
                    continue;
                }
                let line = line.strip_prefix("export ").unwrap_or(line);
                let line = line.strip_prefix("abstract ").unwrap_or(line);
                for prefix in [
                    "const ",
                    "let ",
                    "function ",
                    "class ",
                    "interface ",
                    "type ",
                ] {
                    if let Some(rest) = line.strip_prefix(prefix) {
                        let name: String = rest
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                            .collect();
                        if !name.is_empty() {
                            names.insert(name);
                        }
                    }
                }
            }
        }
        names
    })
}

pub(super) fn identifier(s: &str) -> Result<(), String> {
    const RESERVED: &[&str] = &[
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
        "let",
        "static",
        "implements",
        "interface",
        "package",
        "private",
        "protected",
        "public",
        "await",
        "arguments",
        "eval",
        "freezeContract",
        "TARGET",
        "PROTOCOL_VERSION",
        "ENCODING",
        "WIRE_TAG_LAYOUTS",
        "WIRE_LAYOUTS",
        "WIRE_CONVENTIONS",
        "ASSET_FORMATS",
        "components",
        "WIRE",
        "CAPABILITIES",
        "SCHEMA_HASH",
        "Entity",
        "__proto__",
        "prototype",
        "constructor",
    ];
    if s.is_empty()
        || RESERVED.contains(&s)
        || template_identifiers().contains(s)
        || !s
            .bytes()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphabetic() || c == b'_' || i > 0 && c.is_ascii_digit())
    {
        Err("invalid exported identifier".into())
    } else {
        Ok(())
    }
}

pub(super) fn js_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                write!(out, "\\u{:04x}", c as u32).unwrap();
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
