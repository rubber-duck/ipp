use crate::codec::{ProtocolError, Reader, Writer};
use crate::wire::{OUTPUT_TARGET_CAMERA, OUTPUT_TARGET_CANVAS};

/// Untrusted wire identity, resolved only by the receiving Host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldReference {
    /// Host-local World identity.
    pub id: u64,
    /// Exact runtime lifetime, not durable metadata.
    pub incarnation: u64,
}

impl WorldReference {
    /// Validate the complete lifetime without following a replacement.
    pub fn resolve(
        self,
        host: &ipp_core::HostRuntime,
    ) -> Result<ipp_core::WorldRef, ProtocolError> {
        host.resolve_world_ref(ipp_core::WorldId(self.id), self.incarnation)
            .ok_or(ProtocolError::InvalidReference)
    }
}

impl From<ipp_core::WorldRef> for WorldReference {
    fn from(value: ipp_core::WorldRef) -> Self {
        Self {
            id: value.id().0,
            incarnation: value.incarnation(),
        }
    }
}

/// What an output reference selects within its World.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputTarget {
    /// The World-level canvas of a World that selects the Canvas System; its
    /// lifetime is the World's.
    Canvas,
    /// A Camera entity's view, fenced by its Camera component lifetime.
    Camera {
        /// Generational Camera entity within the World.
        entity: u64,
        /// Exact Camera component lifetime.
        incarnation: u64,
    },
}

/// Untrusted exact output selection, separate from an explicit new binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputReference {
    /// Exact owning World lifetime.
    pub world: WorldReference,
    /// Selected output within that World.
    pub target: OutputTarget,
}

impl OutputReference {
    /// The World-level canvas of an exact World lifetime.
    pub fn canvas(world: WorldReference) -> Self {
        Self {
            world,
            target: OutputTarget::Canvas,
        }
    }

    /// Explicit output domain; unavailability never changes it.
    pub fn kind(self) -> ipp_core::OutputKind {
        match self.target {
            OutputTarget::Canvas => ipp_core::OutputKind::Canvas,
            OutputTarget::Camera {
                ..
            } => ipp_core::OutputKind::Camera,
        }
    }

    /// Validate both lifetimes without rebinding a replacement producer.
    pub fn resolve(
        self,
        host: &ipp_core::HostRuntime,
    ) -> Result<ipp_core::OutputRef, ProtocolError> {
        let world = self.world.resolve(host)?;
        host.resolve_output_ref(world, self.target.core())
            .map_err(|_| ProtocolError::InvalidReference)
    }
}

impl OutputTarget {
    /// The untrusted core output target this wire target names.
    pub fn core(self) -> ipp_core::OutputTarget {
        match self {
            Self::Canvas => ipp_core::OutputTarget::Canvas,
            Self::Camera {
                entity,
                incarnation,
            } => ipp_core::OutputTarget::Camera {
                entity: ipp_core::EntityId::from_bits(entity),
                incarnation,
            },
        }
    }
}

impl From<ipp_core::OutputRef> for OutputReference {
    fn from(value: ipp_core::OutputRef) -> Self {
        Self {
            world: value.world().into(),
            target: match value.target() {
                ipp_core::OutputTarget::Canvas => OutputTarget::Canvas,
                ipp_core::OutputTarget::Camera {
                    entity,
                    incarnation,
                } => OutputTarget::Camera {
                    entity: entity.to_bits(),
                    incarnation,
                },
            },
        }
    }
}

impl Reader<'_> {
    pub(crate) fn world_reference(&mut self) -> Result<WorldReference, ProtocolError> {
        Ok(WorldReference {
            id: self.u64()?,
            incarnation: self.u64()?,
        })
    }

    pub(crate) fn output_kind(&mut self) -> Result<ipp_core::OutputKind, ProtocolError> {
        match self.u8()? {
            0 => Ok(ipp_core::OutputKind::Canvas),
            1 => Ok(ipp_core::OutputKind::Camera),
            _ => Err(ProtocolError::Malformed("output kind")),
        }
    }

    pub(crate) fn output_reference(&mut self) -> Result<OutputReference, ProtocolError> {
        let world = self.world_reference()?;
        let target = match self.u8()? {
            OUTPUT_TARGET_CANVAS => OutputTarget::Canvas,
            OUTPUT_TARGET_CAMERA => OutputTarget::Camera {
                entity: self.u64()?,
                incarnation: self.u64()?,
            },
            _ => return Err(ProtocolError::Malformed("output target")),
        };
        Ok(OutputReference {
            world,
            target,
        })
    }
}

impl Writer {
    pub(crate) fn world_reference(&mut self, value: WorldReference) -> Result<(), ProtocolError> {
        self.u64(value.id)?;
        self.u64(value.incarnation)
    }

    pub(crate) fn output_reference(&mut self, value: OutputReference) -> Result<(), ProtocolError> {
        self.world_reference(value.world)?;
        match value.target {
            OutputTarget::Canvas => self.u8(OUTPUT_TARGET_CANVAS),
            OutputTarget::Camera {
                entity,
                incarnation,
            } => {
                self.u8(OUTPUT_TARGET_CAMERA)?;
                self.u64(entity)?;
                self.u64(incarnation)
            }
        }
    }
}
