//! Owned GPU export: private staging, nonblocking fence and semantic output.

use crate::services::render::assets::{context::RenderAssetLease, loaders::GlTextureData};
use crate::{RenderDevice, RenderService};
use ipp_core::services::asset_management::{
    AssetProvider, AssetTypeId,
    export::{
        AssetExportFormat, AssetExportFuture, AssetOutput, AssetOutputObserver,
        write_texture_header,
    },
};
use ipp_core::services::io::IoCancellation;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    time::Duration,
};

/// Host supplied deferred wake, independent of World evaluation or drawing.
pub type RenderAssetExportDelay =
    Rc<dyn Fn(Duration) -> Pin<Box<dyn Future<Output = ()> + 'static>>>;

struct ExportObserver {
    observer: Rc<dyn AssetOutputObserver>,
    available: IoCancellation,
    context: RenderAssetLease,
    staging: Cell<usize>,
    output: Cell<usize>,
}

impl AssetOutputObserver for ExportObserver {
    fn reserve(&self, bytes: usize) -> Result<(), String> {
        self.check()?;
        self.observer.reserve(
            bytes
                .checked_add(self.staging.get())
                .ok_or("GPU export charge overflow")?,
        )?;
        self.output.set(bytes);
        Ok(())
    }

    fn check(&self) -> Result<(), String> {
        self.observer.check()?;
        if self.available.is_cancelled() {
            return Err("GPU asset representation unloaded during export".into());
        }
        if !self.context.is_current() {
            return Err("Texture creating context was lost or replaced".into());
        }
        Ok(())
    }
}

struct Staging<D: RenderDevice> {
    device: Rc<RefCell<D>>,
    readback: Option<D::TextureReadback>,
    observer: Rc<ExportObserver>,
}

impl<D: RenderDevice> Drop for Staging<D> {
    fn drop(&mut self) {
        if let Some(readback) = self.readback.take()
            && self.observer.context.is_current()
        {
            self.device.borrow_mut().delete_texture_readback(readback);
        }
        // Lost-generation names belong to destroyed context storage. Never bind
        // or delete them through a replacement device. Cancelled observers can
        // refuse shrinking; the enclosing operation then retains its charge
        // conservatively until its allocation owner is actually dropped.
        self.observer.staging.set(0);
        let _ = self.observer.observer.reserve(self.observer.output.get());
    }
}

impl<D: RenderDevice> RenderService<D> {
    /// Supported working GPU formats, independent of CPU/source encodings.
    pub fn asset_export_formats(&self, kind: AssetTypeId) -> Vec<AssetExportFormat> {
        if kind == ipp_core::TEXTURE_TYPE && self.device.borrow().texture_readback_supported() {
            vec![AssetExportFormat::TextureV3]
        } else {
            Vec::new()
        }
    }

    /// Supply the Host event-loop timer used by pending fence checks.
    pub fn set_asset_export_delay(&mut self, delay: RenderAssetExportDelay) {
        self.asset_export_delay = Some(delay);
    }

    /// Hold newly admitted exports after real staging through the Host's deferred
    /// wake. Test controls may retain that timer to observe paused service work;
    /// ordinary signaled fences do not require a timer or another event-loop turn.
    #[cfg(feature = "instrumentation")]
    pub fn set_asset_export_staging_gate(&mut self, enabled: bool) {
        self.asset_export_staging_gate = enabled;
    }

    /// Capture exact loaded identity without issuing a GL command. Device work
    /// starts only when the Host polls the returned owned future with its context
    /// current. Availability and creating generation guard every continuation.
    pub fn export_asset(
        &self,
        provider: &AssetProvider,
        format: AssetExportFormat,
        observer: Rc<dyn AssetOutputObserver>,
    ) -> Result<AssetExportFuture, String> {
        if format != AssetExportFormat::TextureV3
            || provider.source().kind != ipp_core::TEXTURE_TYPE
        {
            return Err("Unsupported GPU asset export format".into());
        }
        if !self.device.borrow().texture_readback_supported() {
            return Err("Device does not support GPU texture exports".into());
        }
        let data = provider
            .data()
            .and_then(|data| data.as_any().downcast_ref::<GlTextureData<D>>())
            .ok_or("Working GPU texture representation is unavailable")?;
        if !Rc::ptr_eq(&data.device, &self.device) {
            return Err("GPU texture belongs to another rendering device".into());
        }
        let texture = data
            .gpu
            .as_ref()
            .ok_or("Working GPU texture is unloaded")?
            .clone();
        let info = data.info;
        let observer = Rc::new(ExportObserver {
            observer,
            available: provider.gpu_export_availability(),
            context: data.asset_lease.clone(),
            staging: Cell::new(0),
            output: Cell::new(0),
        });
        if !observer.context.is_current() {
            return Err("Texture creating context is unavailable".into());
        }
        let delay = self
            .asset_export_delay
            .clone()
            .ok_or("Host GPU export deferred wake is unavailable")?;
        let device = self.device.clone();
        #[cfg(feature = "instrumentation")]
        let staging_gate = self.asset_export_staging_gate;
        Ok(Box::pin(async move {
            observer.check()?;
            let bytes = usize::try_from(info.pixel_bytes)
                .map_err(|_| "GPU staging size exceeds address range")?;
            observer.observer.reserve(bytes)?;
            observer.staging.set(bytes);
            let readback =
                match device
                    .borrow_mut()
                    .begin_texture_readback(&texture, info.width, info.height)
                {
                    Ok(readback) => readback,
                    Err(error) => {
                        observer.staging.set(0);
                        let _ = observer.observer.reserve(0);
                        return Err(error.to_string());
                    }
                };
            let staging = Staging {
                device: device.clone(),
                readback: Some(readback),
                observer: observer.clone(),
            };
            #[cfg(feature = "instrumentation")]
            if staging_gate {
                delay(Duration::from_millis(2)).await;
                observer.check()?;
            }
            loop {
                observer.check()?;
                if device
                    .borrow_mut()
                    .poll_texture_readback(staging.readback.as_ref().unwrap())
                    .map_err(|error| error.to_string())?
                {
                    break;
                }
                delay(Duration::from_millis(2)).await;
                observer.check()?;
            }
            let mut output = AssetOutput::new(observer.clone());
            write_texture_header(&mut output, info).await?;
            output
                .fill(bytes, |offset, destination| {
                    observer.check()?;
                    device
                        .borrow_mut()
                        .copy_texture_readback(
                            staging.readback.as_ref().unwrap(),
                            offset,
                            destination,
                        )
                        .map_err(|error| error.to_string())
                })
                .await?;
            observer.check()?;
            drop(staging);
            output.finish()
        }))
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
