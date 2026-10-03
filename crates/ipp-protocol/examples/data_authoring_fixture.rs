//! Canonical core codec fixtures for generated native and executed-WASM authoring helpers.

use ipp_core::systems::{constraints::*, data_bindings::encode_data_windows};
use ipp_core::{DynamicPropertyKind as K, DynamicValue as V, expressions::*, services::data::*};

fn main() {
    use ExpressionNode as N;
    let expression = ExpressionDeclaration {
        inputs: vec![
            ExpressionInput {
                name: "x".into(),
                kind: K::F32,
            },
            ExpressionInput {
                name: "parameter".into(),
                kind: K::F32,
            },
        ],
        nodes: vec![
            N::Input(0),
            N::Input(1),
            N::Constant(V::F32(2.0)),
            N::Fallback {
                value: 1,
                replacement: 2,
            },
            N::Binary {
                operator: BinaryOperator::Multiply,
                left: 0,
                right: 3,
            },
            N::Constant(V::F32(0.0)),
            N::Constant(V::F32(10.0)),
            N::Clamp {
                value: 4,
                minimum: 5,
                maximum: 6,
            },
            N::Constant(V::Bool(true)),
            N::Ternary {
                condition: 8,
                then_node: 7,
                else_node: 5,
            },
            N::Constant(V::Text("α🙂".into())),
            N::Unary {
                operator: UnaryOperator::Length,
                operand: 10,
            },
            N::Constant(V::U32(u32::MAX)),
            N::Binary {
                operator: BinaryOperator::Equal,
                left: 11,
                right: 12,
            },
            N::Constant(V::Mat2([1.0, 2.0, 3.0, 4.0])),
            N::Unary {
                operator: UnaryOperator::Negate,
                operand: 0,
            },
            N::Unary {
                operator: UnaryOperator::Absolute,
                operand: 15,
            },
            N::Unary {
                operator: UnaryOperator::Not,
                operand: 8,
            },
            N::Constant(V::I32(i32::MIN)),
            N::Constant(V::Vec2([-0.0, 2.0])),
        ],
        output: 9,
    }
    .encode()
    .unwrap();
    let windows = encode_data_windows(&[
        DataWindow::Count(3),
        DataWindow::Range {
            column: "raw α".into(),
            width: 2.0,
            anchor: DataWindowAnchor::Latest,
        },
        DataWindow::Range {
            column: "raw α".into(),
            width: 1.0,
            anchor: DataWindowAnchor::HostTime {
                units_per_second: 1000.0,
            },
        },
        DataWindow::Range {
            column: "raw α".into(),
            width: 2.0,
            anchor: DataWindowAnchor::Supplied(V::I32(-3)),
        },
    ])
    .unwrap();
    let drivers = encode_expression_driver_inputs(&[
        ExpressionDriverInput {
            name: "𐀀".into(),
            property: DriverProperty {
                component: 100,
                offset: 1234,
            },
        },
        ExpressionDriverInput {
            name: "\u{e000}".into(),
            property: DriverProperty {
                component: 101,
                offset: 65536,
            },
        },
    ])
    .unwrap();
    println!("{{\"expression\":{expression:?},\"windows\":{windows:?},\"drivers\":{drivers:?}}}");
}
