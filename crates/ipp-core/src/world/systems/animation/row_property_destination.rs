//! Compiled numeric destination for one schema-row property.
//!
//! A row property is addressed by its field offset, which encodes the rows
//! field, slot and property index. The destination retains the component's
//! stable cell and that offset, never a pointer into the table: tables grow
//! without relocating the component, but their row storage may move. Every
//! write resolves the row through the component's derived `set_field`, so one
//! shape serves every component with rows fields.

use crate::components::registry::{ComponentCellVisitor, ComponentStorage};
use crate::components::rows::row_region;
use crate::components::schema::{ComponentLifecycle, FieldValue, SchemaComponent};
use crate::world::component_binding::ComponentBinding;
use crate::{DynamicValue, EntityId, ErrorReason};
use std::ptr::NonNull;

type RowWrite =
    fn(ComponentBinding<()>, &mut ComponentStorage, u32, DynamicValue) -> Result<(), ErrorReason>;

/// A numeric row property of one component incarnation.
///
/// Liveness is the slot being live and the property being present; lifecycle
/// invalidation drops the owning driver before a row is removed, an optional
/// property is cleared or the incarnation ends. Writes still pass the
/// component's numeric property validation, so a value outside a property's
/// range is rejected instead of published.
#[derive(Clone, Copy, Debug)]
pub(in crate::world) struct RowPropertyDestination {
    cell: ComponentBinding<()>,
    offset: u32,
    write: RowWrite,
}

impl RowPropertyDestination {
    /// Bind a present numeric row property of the component at `entity`, or
    /// None when the offset is not a row property the component opts into
    /// numeric writes for.
    pub(super) fn bind(
        storage: &ComponentStorage,
        entity: EntityId,
        component: u16,
        offset: u32,
    ) -> Option<Self> {
        if row_region(offset).is_none()
            || !ComponentStorage::supports_numeric_property(component, offset)
        {
            return None;
        }

        let index = entity.index() as usize;
        match storage.field(component, index, offset)? {
            FieldValue::Dynamic(value)
                if !matches!(value, DynamicValue::Bool(_) | DynamicValue::Asset(_)) => {}
            _ => return None,
        }

        storage.visit_cell(
            component,
            index,
            RowPropertyBinder {
                offset,
            },
        )
    }

    /// Validate and write one sampled value; nothing changes on error.
    pub(super) fn write(
        self,
        storage: &mut ComponentStorage,
        value: DynamicValue,
    ) -> Result<(), ErrorReason> {
        (self.write)(self.cell, storage, self.offset, value)
    }
}

struct RowPropertyBinder {
    offset: u32,
}

impl ComponentCellVisitor for RowPropertyBinder {
    type Output = RowPropertyDestination;

    fn visit<C>(self, cell: NonNull<C>) -> RowPropertyDestination
    where
        C: SchemaComponent + ComponentLifecycle + 'static,
    {
        RowPropertyDestination {
            // SAFETY: the pointer comes from the occupied stable cell of this
            // component incarnation. Animation lifecycle hooks drop the owning
            // driver before the incarnation is replaced or removed, and every
            // access borrows this World's component storage for its phase. The
            // erased type is restored only by `write_row::<C>` below.
            cell: unsafe { ComponentBinding::new(cell.cast::<()>()) },
            offset: self.offset,
            write: write_row::<C>,
        }
    }
}

fn write_row<C>(
    cell: ComponentBinding<()>,
    storage: &mut ComponentStorage,
    offset: u32,
    value: DynamicValue,
) -> Result<(), ErrorReason>
where
    C: SchemaComponent + ComponentLifecycle,
{
    // SAFETY: `RowPropertyBinder::visit::<C>` erased a cell of exactly `C` and
    // stored this function monomorphized for the same `C`, so the cast restores
    // the original type, alignment and initialized value.
    let component = unsafe { cell.cast::<C>() }.get_mut(storage);
    let field = [(offset, FieldValue::Dynamic(value))];
    component.validate_numeric_properties(&field)?;

    let [(_, value)] = field;
    component
        .set_field(offset, value)
        .map_err(|_| ErrorReason::InvalidField)
}

#[cfg(all(test, feature = "gui"))]
#[path = "row_property_destination_tests.rs"]
mod tests;
