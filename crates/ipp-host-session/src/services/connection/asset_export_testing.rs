//! Trusted instrumentation setup for fixed immutable content, never client URI authority.

use super::*;
use ipp_core::services::{
    asset_management::{AssetSource, export::AssetExportFormat},
    io::MemoryIoSource,
};
use ipp_protocol::host::asset_export::AssetReadAccess;

const PREFIX: &str = "fixture-export:";
const SOURCE: &str = "fixture-export:texture";

impl<P: HostServices> Host<P> {
    pub(super) fn asset_export_test_fixture(&mut self, operation: u8) -> Result<(), String> {
        let source = AssetSource {
            kind: ipp_core::TEXTURE_TYPE,
            uri: SOURCE.into(),
            variant: 0,
        };
        match operation {
            3 => {
                if self.connections.asset_export_policy.is_some() {
                    return Err("Export fixture already exposed".into());
                }
                let memory = MemoryIoSource::new(false);
                let mut bytes = b"IPPT\x03\0\0\0\x03\0\0\0\x02\0\0\0".to_vec();
                bytes.extend([
                    37, 83, 149, 17, 211, 67, 129, 63, 94, 203, 51, 127, 173, 121, 237, 191, 53,
                    229, 181, 223, 241, 157, 43, 255,
                ]);
                self.runtime.io_mut().register(PREFIX, memory.clone())?;
                memory.insert(SOURCE.into(), bytes)?;
                let policy = self.expose_asset_source(
                    source,
                    AssetReadAccess {
                        original: true,
                        cpu: vec![AssetExportFormat::TextureV3],
                        gpu: vec![AssetExportFormat::TextureV3],
                    },
                )?;
                self.connections.asset_export_policy = Some(policy);
            }
            4 => {
                let policy = self
                    .connections
                    .asset_export_policy
                    .take()
                    .ok_or("Export fixture policy absent")?;
                self.revoke_public_asset_source(policy);
            }
            5 => {
                self.runtime.io_mut().unregister(PREFIX);
            }
            6 => {
                self.runtime.asset_resources_mut().unload_all();
                self.runtime.flush_resource_lifecycle();
            }
            _ => return Err("Unknown asset export fixture operation".into()),
        }
        Ok(())
    }
}
