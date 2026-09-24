use ipp_schema_derive::SchemaComponent;

/// Linear constraint authored inputs. Runtime bindings remain outside this value.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct LinearDriver {
    /// Source scalar's generational entity identity.
    pub source: crate::EntityId,
    /// Multiplicative factor.
    pub scale: f32,
    /// Additive bias.
    pub bias: f32,
}

impl Default for LinearDriver {
    fn default() -> Self {
        Self {
            source: crate::EntityId::from_bits(0),
            scale: 1.0,
            bias: 0.0,
        }
    }
}

impl crate::components::schema::ComponentLifecycle for LinearDriver {
    fn validate_field(&self, offset: u32) -> Result<(), crate::ErrorReason> {
        let finite = if offset == std::mem::offset_of!(Self, scale) as u32 {
            self.scale.is_finite()
        } else if offset == std::mem::offset_of!(Self, bias) as u32 {
            self.bias.is_finite()
        } else {
            true
        };
        if finite {
            Ok(())
        } else {
            Err(crate::ErrorReason::InvalidValue)
        }
    }

    /// Scale and bias are finite. Evaluated results are not validated: a finite
    /// driver may still overflow, and dependency cycles are diagnosed by the
    /// ConstraintSystem instead of rejecting the batch.
    fn validate(&self) -> Result<(), crate::ErrorReason> {
        if self.scale.is_finite() && self.bias.is_finite() {
            Ok(())
        } else {
            Err(crate::ErrorReason::InvalidValue)
        }
    }
}
