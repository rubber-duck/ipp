use super::super::Asset;
use crate::services::io::IoCancellation;
use std::{any::Any, rc::Rc};

/// Independent working representations; source registration lifetime belongs to I/O.
#[derive(Clone, Default)]
pub struct AssetWorkingAvailability {
    /// Explicit unload/replacement cancels unfinished CPU encoding.
    pub cpu: IoCancellation,
    /// Graphics invalidation additionally cancels unfinished staged GPU output.
    pub gpu: IoCancellation,
}

impl AssetWorkingAvailability {
    pub(crate) fn invalidate_gpu(&mut self) {
        self.gpu.cancel();
        self.gpu = IoCancellation::default();
    }

    pub(crate) fn invalidate_all(&mut self) {
        self.cpu.cancel();
        self.cpu = IoCancellation::default();
        self.invalidate_gpu();
    }
}

/// One immutable typed CPU payload shared by its actual consumers, without copying.
/// Retention is memory safety; availability still must hold through publication.
#[derive(Clone)]
pub struct AssetCpuSnapshot {
    /// The shared complete typed payload, independent of availability.
    pub data: Rc<dyn Any>,
    /// Explicit working-representation lifetime through successful publication.
    pub available: IoCancellation,
}

struct SharedCpuAsset<T>(Rc<T>);

impl<T: Asset> Asset for SharedCpuAsset<T> {
    fn as_any(&self) -> &dyn Any {
        self.0.as_any()
    }

    fn decoded(&self) -> &dyn Any {
        self.0.decoded()
    }

    fn metadata(&self) -> &dyn Any {
        self.0.metadata()
    }

    fn cpu_snapshot(&self) -> Option<Rc<dyn Any>> {
        Some(self.0.clone())
    }

    fn resident_bytes(&self) -> usize {
        self.0.resident_bytes()
    }
}

/// CPU-only payloads are immutable; GPU owners retain exclusive mutable handles.
pub(in crate::services::asset_management) fn shared_cpu_data<T: Asset>(data: T) -> Box<dyn Asset> {
    if data.graphics_ready().is_none() {
        Box::new(SharedCpuAsset(Rc::new(data)))
    } else {
        Box::new(data)
    }
}
