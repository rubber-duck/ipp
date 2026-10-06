use super::*;

#[test]
fn asset_controls_round_trip_and_reject_truncation() {
    let capability = AssetReadCapability {
        connection: 7,
        grant: 91,
    };
    let source = AssetSource {
        kind: ipp_core::TEXTURE_TYPE,
        uri: "client://exact-name".into(),
        variant: 0,
    };
    for request in [
        AssetExportRequest::Find(source.clone()),
        AssetExportRequest::Read {
            capability,
            representation: AssetReadRepresentation::Original,
            format: None,
        },
        AssetExportRequest::Read {
            capability,
            representation: AssetReadRepresentation::Cpu,
            format: Some(AssetExportFormat::TextureV3),
        },
        AssetExportRequest::Revoke(capability),
    ] {
        let mut writer = Writer::new(Vec::new());
        writer.asset_export_request(&request).unwrap();
        let bytes = writer.0;
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        assert_eq!(reader.asset_export_request().unwrap(), request);
        assert_eq!(reader.at, bytes.len());
        for end in 0..bytes.len() {
            assert!(
                Reader {
                    bytes: &bytes[..end],
                    at: 0
                }
                .asset_export_request()
                .is_err()
            );
        }
    }
    for response in [
        AssetExportResponse::Capability {
            capability,
            source,
            access: AssetReadAccess {
                original: true,
                cpu: vec![AssetExportFormat::TextureV3],
                gpu: vec![AssetExportFormat::TextureV3],
            },
        },
        AssetExportResponse::Read {
            read: BulkReadDescriptor {
                reference: crate::bulk_read::BulkReadReference {
                    connection: 7,
                    read: 17,
                },
                length: Some(24),
            },
            representation: AssetReadRepresentation::Gpu,
            format: Some(AssetExportFormat::TextureV3),
        },
        AssetExportResponse::Revoked,
    ] {
        let mut writer = Writer::new(Vec::new());
        writer.asset_export_response(&response).unwrap();
        let bytes = writer.0;
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        assert_eq!(reader.asset_export_response().unwrap(), response);
        assert_eq!(reader.at, bytes.len());
        for end in 0..bytes.len() {
            assert!(
                Reader {
                    bytes: &bytes[..end],
                    at: 0
                }
                .asset_export_response()
                .is_err()
            );
        }
    }
}

#[test]
fn asset_controls_reject_invalid_authority_and_representation() {
    for connection in [0, 7] {
        for representation in [
            AssetReadRepresentation::Original,
            AssetReadRepresentation::Cpu,
            AssetReadRepresentation::Gpu,
        ] {
            for format in [None, Some(AssetExportFormat::TextureV3)] {
                let request = AssetExportRequest::Read {
                    capability: AssetReadCapability {
                        connection,
                        grant: 9,
                    },
                    representation,
                    format,
                };
                let mut writer = Writer::new(Vec::new());
                let valid = connection != 0
                    && (representation == AssetReadRepresentation::Original) == format.is_none();
                assert_eq!(writer.asset_export_request(&request).is_ok(), valid);
            }
        }
    }
    let mut bytes = vec![ASSET_EXPORT_READ];
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&9u64.to_le_bytes());
    bytes.extend_from_slice(&[ASSET_REPRESENTATION_GPU, 0]);
    assert!(
        Reader {
            bytes: &bytes,
            at: 0
        }
        .asset_export_request()
        .is_err()
    );
    bytes[17] = 255;
    assert!(
        Reader {
            bytes: &bytes,
            at: 0
        }
        .asset_export_request()
        .is_err()
    );
    let mut bytes = vec![ASSET_EXPORT_OPENED];
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&[0, ASSET_REPRESENTATION_ORIGINAL, 0]);
    assert!(
        Reader {
            bytes: &bytes,
            at: 0
        }
        .asset_export_response()
        .is_err()
    );
}

#[test]
fn every_semantic_format_round_trips_through_typed_controls() {
    let capability = AssetReadCapability {
        connection: 7,
        grant: 91,
    };
    for format in [
        AssetExportFormat::MeshV3,
        AssetExportFormat::TextureV3,
        AssetExportFormat::SkeletonV1,
        AssetExportFormat::PoseV1,
        AssetExportFormat::SkinV1,
        AssetExportFormat::ShaderV3,
        AssetExportFormat::AnimationV4,
        AssetExportFormat::GeometryV1,
        AssetExportFormat::ParticleCacheV1,
        AssetExportFormat::ExpressionV1,
    ] {
        for representation in [AssetReadRepresentation::Cpu, AssetReadRepresentation::Gpu] {
            let request = AssetExportRequest::Read {
                capability,
                representation,
                format: Some(format),
            };
            let mut writer = Writer::new(Vec::new());
            writer.asset_export_request(&request).unwrap();
            let mut reader = Reader {
                bytes: &writer.0,
                at: 0,
            };
            assert_eq!(reader.asset_export_request().unwrap(), request);
            assert_eq!(reader.at, writer.0.len());

            let response = AssetExportResponse::Read {
                read: BulkReadDescriptor {
                    reference: crate::bulk_read::BulkReadReference {
                        connection: 7,
                        read: 17,
                    },
                    length: None,
                },
                representation,
                format: Some(format),
            };
            let mut writer = Writer::new(Vec::new());
            writer.asset_export_response(&response).unwrap();
            let mut reader = Reader {
                bytes: &writer.0,
                at: 0,
            };
            assert_eq!(reader.asset_export_response().unwrap(), response);
            assert_eq!(reader.at, writer.0.len());
        }
    }
}
