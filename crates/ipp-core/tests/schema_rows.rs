//! Schema rows derived outside the core: allocation-free property access by offset.

use ipp_core::DynamicValue;
use ipp_core::components::rows::{Rows, SchemaRow};
use ipp_core::components::schema::{FieldValue, SchemaComponent};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct AllocationCounter;

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(previous) = count.get() {
            count.set(Some(previous + 1));
        }
    });
}

// SAFETY: Every allocation delegates to System with the caller's layout. The counter
// stores counts only, never retains pointers, and does not alter ownership or aliasing.
unsafe impl GlobalAlloc for AllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: Forward the valid allocation layout without retaining its pointer.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: This is the live System allocation and original layout supplied by
        // the caller; forwarding ends its lifetime without retaining an alias.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: Forward the uniquely owned live allocation and valid new size.
        // System handles invalidation of the old pointer when the allocation moves.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: AllocationCounter = AllocationCounter;

fn allocations(work: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|count| count.set(Some(0)));
    work();
    ALLOCATIONS.with(|count| count.replace(None)).unwrap()
}

#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
struct Joint {
    translation: Option<[f32; 3]>,
    #[schema(rotation)]
    rotation: Option<[f32; 4]>,
    weight: f32,
}

#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
struct Rig {
    label: std::sync::Arc<str>,
    #[schema(rows)]
    joints: Rows<Joint>,
}

#[test]
fn numeric_row_property_reads_and_writes_do_not_allocate() {
    let mut rig = Rig::default();
    for index in 0..64 {
        rig.joints
            .insert(
                index * 2,
                Joint {
                    translation: Some([index as f32, 0.0, 0.0]),
                    ..Joint::default()
                },
            )
            .unwrap();
    }
    rig.joints.remove(10);

    let count = Joint::LAYOUT.property_count();
    let translation = |slot: u32| 0x1000_0000 + slot * count;
    let weight = |slot: u32| 0x1000_0000 + slot * count + 2;
    let mut reads = Vec::with_capacity(4);

    let used = allocations(|| {
        reads.push(rig.field(translation(126)));
        reads.push(rig.field(translation(8)));
        reads.push(rig.field(translation(9)));
        reads.push(rig.field(translation(126) + 1));
        rig.set_field(weight(126), FieldValue::Dynamic(DynamicValue::F32(0.25)))
            .unwrap();
        rig.set_field(translation(12), FieldValue::Unset).unwrap();
    });

    assert_eq!(used, 0, "numeric row access must not allocate");
    assert_eq!(
        reads,
        [
            Ok(FieldValue::Dynamic(DynamicValue::Vec3([63.0, 0.0, 0.0]))),
            Ok(FieldValue::Dynamic(DynamicValue::Vec3([4.0, 0.0, 0.0]))),
            Err(ipp_core::components::schema::FieldError::UnknownField),
            Ok(FieldValue::Unset),
        ]
    );
    assert_eq!(rig.joints.get(126).unwrap().weight, 0.25);
    assert_eq!(rig.joints.get(12).unwrap().translation, None);
}
