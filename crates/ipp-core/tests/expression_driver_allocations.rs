//! Allocation evidence around actual prepared ConstraintSystem updates.

mod support;
use ipp_core::components::{CustomMaterial, ExpressionDriver, Scalar};
use ipp_core::expressions::*;
use ipp_core::services::asset_management::{
    AssetUpload, AssetUploadIdentity, expression::EXPRESSION_TYPE,
};
use ipp_core::systems::constraints::*;
use ipp_core::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use support::WorldTestDriver;
use support::selection::{ASSETS, CONSTRAINTS, RENDER, select};

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

// SAFETY: All operations delegate to System with the caller's valid layout.
// Only per-thread counts are retained, never pointers or aliases; allocation
// lifetimes and ownership remain exactly those of System.
unsafe impl GlobalAlloc for AllocationCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: Forward the valid layout; no pointer is retained by the counter.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: Forward the valid layout; System supplies initialized storage.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The caller owns this live System allocation and supplies its
        // original layout. Forwarding ends its lifetime without retaining aliases.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: The caller supplies the live, uniquely owned System allocation
        // and valid new size. System invalidates the old pointer if it moves.
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

#[test]
fn actual_prepared_scalar_dynamic_and_invalid_drivers_allocate_nothing() {
    let mut host = crate::support::task_scheduler::host();
    let id = host
        .create_world(
            WorldLimits::default(),
            &select(&[ASSETS, CONSTRAINTS, RENDER]),
        )
        .unwrap();
    let mut world = host.world_mut(id).unwrap();
    let mut material = CustomMaterial::default();
    let dynamic = material
        .properties
        .set("output", DynamicValue::F32(-9.0))
        .unwrap();
    let dynamic_input = material
        .properties
        .set("input", DynamicValue::F32(0.0))
        .unwrap();
    world
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Scalar(Scalar {
                        value: 0.0,
                    }),
                ),
                Command::Create {
                    alias: 2,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(2),
                    ComponentValue::Scalar(Scalar {
                        value: -7.0,
                    }),
                ),
                Command::Create {
                    alias: 3,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(3),
                    ComponentValue::CustomMaterial(material),
                ),
            ],
        })
        .unwrap();
    let created = world
        .update_for_test(0.0)
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap();
    let source = created[0].1;
    let scalar = created[1].1;
    let dynamic_target = created[2].1;
    for (asset, fallback) in [(1, false), (2, true)] {
        let mut nodes = vec![
            ExpressionNode::Constant(DynamicValue::F32(12.0)),
            ExpressionNode::Input(0),
            ExpressionNode::Binary {
                operator: BinaryOperator::Divide,
                left: 0,
                right: 1,
            },
        ];
        if fallback {
            nodes.extend([
                ExpressionNode::Constant(DynamicValue::F32(17.0)),
                ExpressionNode::Fallback {
                    value: 2,
                    replacement: 3,
                },
            ]);
        }
        let declaration = ExpressionDeclaration {
            inputs: vec![ExpressionInput {
                name: "x".into(),
                kind: DynamicPropertyKind::F32,
            }],
            output: nodes.len() - 1,
            nodes,
        };
        world
            .enqueue_asset(AssetUpload {
                id: asset,
                key: AssetUploadIdentity {
                    kind: EXPRESSION_TYPE,
                    asset,
                    variant: 0,
                },
                bytes: declaration.encode().unwrap(),
            })
            .unwrap();
        assert!(world.await_upload_for_test().assets[0].result.is_ok());
    }
    let inputs = encode_expression_driver_inputs(&[ExpressionDriverInput {
        name: "x".into(),
        property: DriverProperty {
            component: ComponentValue::SCALAR,
            offset: 0,
        },
    }])
    .unwrap();
    let dynamic_inputs = encode_expression_driver_inputs(&[ExpressionDriverInput {
        name: "x".into(),
        property: DriverProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            offset: dynamic_input,
        },
    }])
    .unwrap();
    world
        .enqueue(Batch {
            id: 2,
            operations: vec![
                Command::insert_value(
                    EntityRef::Handle(scalar),
                    ComponentValue::ExpressionDriver(ExpressionDriver {
                        source,
                        expression_source: "asset://19/1".into(),
                        inputs: inputs.clone(),
                        ..Default::default()
                    }),
                ),
                Command::insert_value(
                    EntityRef::Handle(dynamic_target),
                    ComponentValue::ExpressionDriver(ExpressionDriver {
                        source: dynamic_target,
                        expression_source: "asset://19/2".into(),
                        inputs: dynamic_inputs,
                        target_component: u32::from(ComponentValue::CUSTOM_MATERIAL),
                        target_offset: dynamic,
                        ..Default::default()
                    }),
                ),
            ],
        })
        .unwrap();
    for _ in 0..16 {
        world.update_for_test(0.0).unwrap();
    }
    assert_eq!(
        world.expression_driver_status(scalar).unwrap().state,
        ExpressionDriverState::Retained(ExpressionDriverReason::Calculation)
    );
    assert_eq!(
        world
            .expression_driver_status(dynamic_target)
            .unwrap()
            .state,
        ExpressionDriverState::Written
    );
    let count = allocations(|| {
        for _ in 0..1024 {
            world.step(0.0).unwrap();
        }
    });
    assert_eq!(
        count, 0,
        "actual prepared World updates allocated {count} times"
    );
    let material = world
        .inspect(dynamic_target)
        .unwrap()
        .components
        .into_iter()
        .find_map(|v| {
            if let ComponentValue::CustomMaterial(material) = v {
                Some(material)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        material.properties.get("output"),
        Some(DynamicValue::F32(17.0))
    );
}
