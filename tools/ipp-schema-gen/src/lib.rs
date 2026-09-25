//! Verified binary target export processing. This tool never links a host core.

mod binary_reader;
mod codec_limits;
mod export_reader;
mod model;
mod typescript;
mod typescript_names;
mod wire_contract;
mod wire_manifest;

/// TypeScript modules generated from one executed target contract.
#[derive(Debug)]
pub struct GeneratedContract {
    /// Shipped client: typed contract, codecs and resolved codec bounds.
    pub client: String,
    /// Descriptive wire manifest imported only by tests and tools.
    pub manifest: String,
}

/// Generate a browser-compatible typed contract from executed target output.
pub fn generate(bytes: &[u8]) -> Result<GeneratedContract, String> {
    let export = export_reader::read_export(bytes)?;
    let manifest = wire_manifest::render(&export);

    Ok(GeneratedContract {
        client: typescript::render(export)?,
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::binary_reader::Reader;
    use super::export_reader::read_target_features;
    use super::model::Capabilities;
    use super::typescript::render_template;
    use super::typescript_names::{identifier, js_string, member_identifier};
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

        // Row property names are interface members only, so template bindings
        // such as `asset` are allowed while syntax and reserved words are not.
        assert!(identifier("asset").is_err());
        assert!(member_identifier("asset").is_ok());
        for name in ["class", "__proto__", "a-b", "9name", ""] {
            assert!(member_identifier(name).is_err());
        }
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
    fn rows_layouts_require_their_region_and_supported_unique_properties() {
        use super::export_reader::read_rows_layout;

        let layout = |base: u32, properties: &[(&str, u8, u8, u8)]| {
            let mut bytes = base.to_le_bytes().to_vec();
            bytes.extend_from_slice(&(properties.len() as u16).to_le_bytes());
            for (name, kind, optional, hint) in properties {
                bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
                bytes.extend_from_slice(name.as_bytes());
                bytes.extend_from_slice(&[*kind, *optional, *hint]);
            }
            bytes
        };
        let read = |bytes: &[u8], ordinal| {
            read_rows_layout(
                &mut Reader {
                    bytes,
                    at: 0,
                },
                ordinal,
            )
        };

        let valid = layout(
            0x2000_0000,
            &[
                ("weight", 1, 0, 0),
                ("rotation", 7, 1, 1),
                ("source", 12, 1, 0),
            ],
        );
        let parsed = read(&valid, 2).unwrap();
        assert_eq!(parsed.region_base, 0x2000_0000);
        assert_eq!(parsed.properties.len(), 3);
        assert!(parsed.properties[1].optional && parsed.properties[1].rotation);
        assert_eq!(parsed.properties[2].kind, 12);

        assert!(
            read(&valid, 1).is_err(),
            "region follows the rows field ordinal"
        );
        assert!(read(&layout(0x8000_0000, &[("a", 1, 0, 0)]), 8).is_err());
        assert!(read(&layout(0x1000_0000, &[]), 1).is_err(), "empty layout");
        for invalid in [
            [("a", 8, 0, 0), ("b", 1, 0, 0)], // matrices are not row properties
            [("a", 6, 0, 1), ("b", 1, 0, 0)], // rotation requires Vec4
            [("a", 1, 2, 0), ("b", 1, 0, 0)], // optional flag
            [("a", 1, 0, 0), ("a", 3, 0, 0)], // duplicate property
            [("a", 1, 0, 0), ("class", 3, 0, 0)], // reserved identifier
        ] {
            assert!(read(&layout(0x1000_0000, &invalid), 1).is_err());
        }
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
