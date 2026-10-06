//! Built-in program recipes use ordinary resource loading and recovery.

use super::super::assets::context::{RenderAssetContext, RenderAssetLease};
use super::super::assets::loaders::SharedRenderDevice;
use super::shader::RenderShaderConfig;
use crate::{RenderDevice, RenderError};
use ipp_core::services::asset_management::{Asset, AssetLoader, AssetTypeId, AsyncAssetLoader};
use std::any::Any;

pub(in crate::services::render) const PROGRAM_TYPE: AssetTypeId = AssetTypeId(14);

pub(in crate::services::render) struct GlProgramData<D: RenderDevice> {
    pub program: Option<D::Program>,
    asset_lease: RenderAssetLease,
    device: SharedRenderDevice<D>,
}

impl<D: RenderDevice> Asset for GlProgramData<D> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn decoded(&self) -> &dyn Any {
        self
    }

    fn invalidate_graphics(&mut self) {
        if let Some(program) = self.program.take()
            && self.asset_lease.is_current()
        {
            self.device.borrow_mut().delete_program(program);
        }
    }

    fn graphics_ready(&self) -> Option<bool> {
        Some(self.program.is_some())
    }

    fn graphics_bytes(&self) -> Option<usize> {
        Some(0)
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl<D: RenderDevice> Drop for GlProgramData<D> {
    fn drop(&mut self) {
        if let Some(program) = self.program.take()
            && self.asset_lease.is_current()
        {
            self.device.borrow_mut().delete_program(program);
        }
    }
}

pub(in crate::services::render) fn loader<D: RenderDevice>(
    device: SharedRenderDevice<D>,
    context: RenderAssetContext,
) -> impl AssetLoader<Data = GlProgramData<D>> {
    AsyncAssetLoader::decode(move |mut reader| async move {
        let mut input =
            ipp_core::services::asset_management::decode::AssetReader::new(&mut *reader);
        let bits = input.u32().await?;
        input.finish().await?;
        let config = RenderShaderConfig::from_recipe_bits(bits);
        let sources = if bits & (1 << 9) != 0 {
            config.shadow_sources()
        } else {
            config.sources()
        };
        let (vertex, fragment) = sources.map_err(|error| error.to_string())?;
        loop {
            let asset_lease = context.wait().await;
            let result = device.borrow_mut().create_program(&vertex, &fragment);
            match result {
                Ok(program) => {
                    return Ok(GlProgramData {
                        program: Some(program),
                        asset_lease,
                        device,
                    });
                }
                Err(RenderError::ContextLost) => context.set_active(false),
                Err(error) => return Err(error.to_string()),
            }
        }
    })
}
