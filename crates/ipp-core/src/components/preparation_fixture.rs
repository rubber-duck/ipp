//! Registered fixture proving staged resource ownership without a creation factory.
#![allow(missing_docs)]
use ipp_schema_derive::SchemaComponent;

/// Test-only registered native component with owned effective activation data.
#[repr(C)]
#[derive(Debug, PartialEq, SchemaComponent)]
#[schema(no_create)]
pub struct PreparedBuffer {
    pub length: u32,
    #[schema(ignore)]
    pub counters: std::rc::Rc<BufferCounters>,
    #[schema(ignore)]
    pub allocation: Option<std::rc::Rc<PreparedAllocation>>,
}

#[derive(Debug, Default, PartialEq)]
pub struct BufferCounters {
    pub clones: std::cell::Cell<usize>,
    pub prepared: std::cell::Cell<usize>,
    pub released: std::cell::Cell<usize>,
}

#[derive(Debug, PartialEq)]
pub struct PreparedAllocation {
    pub bytes: Vec<u8>,
    counters: std::rc::Rc<BufferCounters>,
}

impl Drop for PreparedAllocation {
    fn drop(&mut self) {
        self.counters.released.set(self.counters.released.get() + 1);
    }
}

impl Clone for PreparedBuffer {
    fn clone(&self) -> Self {
        self.counters.clones.set(self.counters.clones.get() + 1);
        Self {
            length: self.length,
            counters: self.counters.clone(),
            allocation: self.allocation.clone(),
        }
    }
}

impl crate::components::schema::ComponentLifecycle for PreparedBuffer {
    fn defers_preparation() -> bool {
        false
    }

    fn activation_bytes(&self) -> usize {
        self.allocation
            .as_ref()
            .map_or(0, |allocation| allocation.bytes.capacity())
    }

    fn prepare_effective(&self, max_activation_bytes: usize) -> Result<Self, crate::ErrorReason> {
        if self.length > 65_536 || self.length as usize > max_activation_bytes {
            return Err(crate::ErrorReason::Capacity);
        }
        self.counters.prepared.set(self.counters.prepared.get() + 1);
        let allocation = std::rc::Rc::new(PreparedAllocation {
            bytes: vec![0; self.length as usize],
            counters: self.counters.clone(),
        });
        if self.length == 13 {
            return Err(crate::ErrorReason::InvalidValue);
        }
        Ok(Self {
            length: self.length,
            counters: self.counters.clone(),
            allocation: Some(allocation),
        })
    }
}
