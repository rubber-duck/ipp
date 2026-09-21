//! Verified binary target export processing. This tool never links a host core.

mod binary_reader;
mod export_reader;
mod model;
mod typescript;
mod typescript_names;
mod wire_contract;

/// Generate a browser-compatible typed contract from executed target output.
pub fn generate(bytes: &[u8]) -> Result<String, String> {
    typescript::render(export_reader::read_export(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::binary_reader::Reader;
    use super::export_reader::read_target_features;
    use super::model::Capabilities;
    use super::typescript::render_template;
    use super::typescript_names::{identifier, js_string};
    use super::*;

    #[test]
    fn rejects_unverified_exports_and_invalid_identifiers() {
        assert!(generate(&[]).is_err());
        let mut bytes = b"IPPB".to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(b"untrusted body");
        assert_eq!(generate(&bytes).unwrap_err(), "export hash mismatch");
        for name in [
            "class",
            "default",
            "Entity",
            "TARGET",
            "freezeContract",
            "decodeResponse",
            "r#type",
            "a-b",
            "9name",
            "",
        ] {
            assert!(identifier(name).is_err());
        }
        assert!(identifier("LinearDriver").is_ok());
    }

    #[test]
    fn field_export_strings_cannot_inject_typescript() {
        assert_eq!(js_string("\";evil\n"), "\"\\\";evil\\n\"");
    }

    #[test]
    fn target_features_are_self_describing_and_unique() {
        let feature = |id: u8, name: &str| {
            let mut bytes = vec![id, 1];
            bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes
        };
        let mut bytes = vec![1];
        bytes.extend_from_slice(&feature(99, "future.feature"));
        let parsed = read_target_features(&mut Reader {
            bytes: &bytes,
            at: 0,
        })
        .unwrap();
        assert_eq!(parsed[0].id, 99);
        assert_eq!(parsed[0].name, "future.feature");
        assert!(parsed[0].enabled);

        let mut duplicate = vec![2];
        duplicate.extend_from_slice(&feature(7, "first"));
        duplicate.extend_from_slice(&feature(7, "second"));
        assert!(
            read_target_features(&mut Reader {
                bytes: &duplicate,
                at: 0,
            })
            .is_err()
        );
    }

    #[test]
    fn template_conditions_nest_and_reject_malformed_directives() {
        let capabilities = Capabilities {
            builtin_assets: true,
            ..Capabilities::default()
        };
        let template = "root\n// #if builtin-assets\noverlay\n// #if skeletal-animation\nscene\n// #endif\nafter\n// #endif\ntail\n";
        assert_eq!(
            render_template(template, capabilities).unwrap(),
            "root\noverlay\nafter\ntail\n",
        );
        assert!(render_template("// #if unknown\n", capabilities).is_err());
        assert!(render_template("// #endif\n", capabilities).is_err());
        assert!(render_template("// #if skeletal-animation\n", capabilities).is_err());
    }
}
