//! Explicit relative-layer endpoints; physical placement is derived by Canvas.

use crate::ErrorReason;
use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;

/// Moves this entity and its ordinary descendants from an explicit previous
/// relative layer to the destination in [`super::CanvasStyle::layer`].
/// Descendant transition roots retain their independent progress. Removing
/// this component immediately resumes ordinary destination stack resolution.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, SchemaComponent)]
pub struct CanvasLayerTransition {
    /// Previous parent-relative offset. Structural; animation never writes it.
    pub previous_layer: u32,
    /// Progress from the previous endpoint to the destination, finite in 0..=1.
    /// Ordinary animation drivers may write this field.
    pub progress: f32,
}

impl ComponentLifecycle for CanvasLayerTransition {
    fn animatable_field(offset: u32) -> bool {
        offset == std::mem::offset_of!(Self, progress) as u32
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if self.progress.is_finite() && (0.0..=1.0).contains(&self.progress) {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_the_only_animatable_field() {
        assert!(CanvasLayerTransition::animatable_field(
            std::mem::offset_of!(CanvasLayerTransition, progress) as u32
        ));
        assert!(!CanvasLayerTransition::animatable_field(
            std::mem::offset_of!(CanvasLayerTransition, previous_layer) as u32
        ));
    }
}
