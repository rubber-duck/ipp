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

impl crate::components::schema::ComponentLifecycle for LinearDriver {}
