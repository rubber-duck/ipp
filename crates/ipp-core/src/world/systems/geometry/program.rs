//! Resource selections and local transforms compiled at lifecycle boundaries.

use super::{CompoundGeometryShape, GeometryShape, TransformedGeometryShape};
use crate::{
    ErrorReason, components::registry::ComponentStorage, systems::hierarchy::ObjectTransformBinding,
};

pub(super) enum GeometryProgram {
    Rigid {
        shape: GeometryShape,
        model: ObjectTransformBinding,
    },
    Parts {
        parts: Vec<TransformedGeometryShape>,
        model: ObjectTransformBinding,
    },
    Dynamic,
}

impl std::fmt::Debug for GeometryProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GeometryProgram")
    }
}

impl GeometryProgram {
    #[inline]
    pub(super) fn model(
        &self,
        storage: &ComponentStorage,
    ) -> Option<Result<super::GeometryShapeTransform, ErrorReason>> {
        match self {
            Self::Rigid {
                model,
                ..
            }
            | Self::Parts {
                model,
                ..
            } => Some(model.evaluate(storage)),
            Self::Dynamic => None,
        }
    }

    pub(super) fn evaluate(
        &self,
        model: &super::GeometryShapeTransform,
        output: &mut CompoundGeometryShape,
    ) -> Result<(), ErrorReason> {
        match self {
            Self::Rigid {
                shape,
                ..
            } => {
                output.parts.clear();
                output.parts.push(TransformedGeometryShape {
                    shape: *shape,
                    transform: *model,
                });
                Ok(())
            }
            Self::Parts {
                parts,
                ..
            } => {
                output.parts.clear();
                output.parts.reserve(parts.len());
                for part in parts {
                    output.parts.push(TransformedGeometryShape {
                        shape: part.shape,
                        transform: part.transform.then(model)?,
                    });
                }
                Ok(())
            }
            Self::Dynamic => unreachable!("dynamic programs have no compiled model"),
        }
    }
}
