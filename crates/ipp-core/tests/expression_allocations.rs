//! Public pure evaluator allocation evidence; not a driver or dataset integration harness.

use ipp_core::expressions::{
    BinaryOperator, ExpressionDeclaration, ExpressionInput, ExpressionInvalid, ExpressionNode,
    ExpressionResult, PreparedExpression, UnaryOperator,
};
use ipp_core::{DynamicPropertyKind, DynamicValue};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

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
fn scalar_and_row_samples_allocate_nothing_after_preparation() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            ExpressionInput {
                name: "x".into(),
                kind: DynamicPropertyKind::F32,
            },
            ExpressionInput {
                name: "scale".into(),
                kind: DynamicPropertyKind::F32,
            },
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Binary {
                operator: BinaryOperator::Multiply,
                left: 0,
                right: 1,
            },
            ExpressionNode::Constant(DynamicValue::F32(2.0)),
            ExpressionNode::Binary {
                operator: BinaryOperator::Add,
                left: 2,
                right: 3,
            },
        ],
        output: 4,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scalar = plan.scratch();
    let mut rows = plan.scratch();
    let scale = DynamicValue::F32(3.0);
    let source_rows: Vec<_> = (0..10_000).map(|i| DynamicValue::F32(i as f32)).collect();
    let start = Instant::now();
    let scalar_allocations = allocations(|| {
        let shared = plan.clone();
        for i in 0..10_000 {
            let x = DynamicValue::F32(i as f32);
            assert_eq!(
                shared
                    .evaluate(&mut scalar, &[Some(&x), Some(&scale)])
                    .unwrap(),
                &ExpressionResult::Valid(DynamicValue::F32(i as f32 * 3.0 + 2.0))
            );
        }
    });
    let scalar_time = start.elapsed();
    let start = Instant::now();
    let row_allocations = allocations(|| {
        for (i, x) in source_rows.iter().enumerate() {
            assert_eq!(
                plan.evaluate(&mut rows, &[Some(x), Some(&scale)]).unwrap(),
                &ExpressionResult::Valid(DynamicValue::F32(i as f32 * 3.0 + 2.0))
            );
        }
    });
    let row_time = start.elapsed();
    assert_eq!(scalar_allocations, 0);
    assert_eq!(row_allocations, 0);
    eprintln!(
        "expression public API 10k scalar={scalar_time:?}, row={row_time:?}, allocations scalar={scalar_allocations}, row={row_allocations}"
    );
}

#[test]
fn text_selection_fallback_and_invalid_recovery_allocate_nothing() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            ExpressionInput {
                name: "choose".into(),
                kind: DynamicPropertyKind::Bool,
            },
            ExpressionInput {
                name: "text".into(),
                kind: DynamicPropertyKind::Text,
            },
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Unary {
                operator: UnaryOperator::Length,
                operand: 1,
            },
            ExpressionNode::Constant(DynamicValue::U32(42)),
            ExpressionNode::Fallback {
                value: 2,
                replacement: 3,
            },
            ExpressionNode::Ternary {
                condition: 0,
                then_node: 2,
                else_node: 4,
            },
        ],
        output: 5,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let text = DynamicValue::Text(Arc::from("é🙂e\u{301}"));
    let yes = DynamicValue::Bool(true);
    let no = DynamicValue::Bool(false);
    let used = allocations(|| {
        for _ in 0..10_000 {
            for (condition, value, expected) in [
                (
                    Some(&yes),
                    Some(&text),
                    ExpressionResult::Valid(DynamicValue::U32(4)),
                ),
                (
                    Some(&yes),
                    None,
                    ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
                        slot: 1,
                    }),
                ),
                (
                    Some(&no),
                    None,
                    ExpressionResult::Valid(DynamicValue::U32(42)),
                ),
                (
                    None,
                    Some(&text),
                    ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
                        slot: 0,
                    }),
                ),
            ] {
                assert_eq!(
                    plan.evaluate(&mut scratch, &[condition, value]).unwrap(),
                    &expected
                );
            }
        }
    });
    assert_eq!(used, 0);
    eprintln!("expression public API 40k text/branch/recovery evaluations allocations={used}");
}
