//! Executed-target authoring metadata obtained from the core's canonical encoders.
//! This is export work, never a World evaluation path or a second format definition.

use ipp_core::systems::{constraints::*, data_bindings::encode_data_windows};
use ipp_core::{DynamicPropertyKind, DynamicValue, expressions::*, services::data::*};
use std::fmt::Write as _;
use std::sync::OnceLock;

pub(crate) fn metadata() -> &'static str {
    static METADATA: OnceLock<String> = OnceLock::new();
    METADATA.get_or_init(build)
}

fn encoded(inputs: Vec<ExpressionInput>, nodes: Vec<ExpressionNode>) -> Vec<u8> {
    ExpressionDeclaration {
        inputs,
        output: nodes.len() - 1,
        nodes,
    }
    .encode()
    .expect("valid format probe")
}

fn build() -> String {
    use BinaryOperator as B;
    use ExpressionNode as N;
    use UnaryOperator as U;
    let f = N::Constant(DynamicValue::F32(2.0));
    let b = N::Constant(DynamicValue::Bool(true));
    let t = N::Constant(DynamicValue::Text("x".into()));
    let base = encoded(vec![], vec![f.clone()]).len();
    let mut out = format!(
        "{{\"expressionMagic\":{:?},\"expressionVersion\":{},\"expressionMaxBytes\":{},\"expressionMaxItems\":{},\"expressionMaxStringBytes\":{},\"driverMaxInputs\":{},",
        EXPRESSION_FORMAT_MAGIC,
        EXPRESSION_FORMAT_VERSION,
        EXPRESSION_MAX_BYTES,
        EXPRESSION_MAX_ITEMS,
        EXPRESSION_MAX_STRING_BYTES,
        EXPRESSION_DRIVER_MAX_INPUTS
    );
    out.push_str("\"nodes\":{");
    let input = encoded(
        vec![ExpressionInput {
            name: "x".into(),
            kind: DynamicPropertyKind::F32,
        }],
        vec![N::Input(0)],
    );
    write!(
        out,
        "\"input\":{},\"constant\":{}",
        input[input.len() - 5],
        encoded(vec![], vec![f.clone()])[20]
    )
    .unwrap();
    for (name, node) in [
        (
            "unary",
            N::Unary {
                operator: U::Negate,
                operand: 0,
            },
        ),
        (
            "binary",
            N::Binary {
                operator: B::Add,
                left: 0,
                right: 0,
            },
        ),
        (
            "clamp",
            N::Clamp {
                value: 0,
                minimum: 0,
                maximum: 0,
            },
        ),
        (
            "fallback",
            N::Fallback {
                value: 0,
                replacement: 0,
            },
        ),
    ] {
        write!(
            out,
            ",\"{name}\":{}",
            encoded(vec![], vec![f.clone(), node])[base]
        )
        .unwrap();
    }
    let branch_base = encoded(vec![], vec![b.clone(), f.clone()]).len();
    write!(
        out,
        ",\"ternary\":{}}},\"unary\":{{",
        encoded(
            vec![],
            vec![
                b.clone(),
                f.clone(),
                N::Ternary {
                    condition: 0,
                    then_node: 1,
                    else_node: 1
                }
            ]
        )[branch_base]
    )
    .unwrap();
    for (i, (name, operator, literal)) in [
        ("negate", U::Negate, f.clone()),
        ("absolute", U::Absolute, f.clone()),
        ("not", U::Not, b.clone()),
        ("length", U::Length, t),
    ]
    .into_iter()
    .enumerate()
    {
        let at = encoded(vec![], vec![literal.clone()]).len();
        let tag = encoded(
            vec![],
            vec![
                literal,
                N::Unary {
                    operator,
                    operand: 0,
                },
            ],
        )[at + 1];
        write!(
            out,
            "{}\"{name}\":{tag}",
            if i == 0 {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    out.push_str("},\"binary\":{");
    for (i, (name, operator)) in [
        ("add", B::Add),
        ("subtract", B::Subtract),
        ("multiply", B::Multiply),
        ("divide", B::Divide),
        ("minimum", B::Minimum),
        ("maximum", B::Maximum),
        ("equal", B::Equal),
        ("less", B::Less),
        ("greater", B::Greater),
        ("and", B::And),
        ("or", B::Or),
    ]
    .into_iter()
    .enumerate()
    {
        let literal = if matches!(operator, B::And | B::Or) {
            b.clone()
        } else {
            f.clone()
        };
        let at = encoded(vec![], vec![literal.clone()]).len();
        let tag = encoded(
            vec![],
            vec![
                literal,
                N::Binary {
                    operator,
                    left: 0,
                    right: 0,
                },
            ],
        )[at + 1];
        write!(
            out,
            "{}\"{name}\":{tag}",
            if i == 0 {
                ""
            } else {
                ","
            }
        )
        .unwrap();
    }
    let count = encode_data_windows(&[DataWindow::Count(1)]).unwrap();
    let range = |anchor| {
        encode_data_windows(&[DataWindow::Range {
            column: "x".into(),
            width: 1.0,
            anchor,
        }])
        .unwrap()
    };
    let latest = range(DataWindowAnchor::Latest);
    let host = range(DataWindowAnchor::HostTime {
        units_per_second: 1.0,
    });
    let supplied = range(DataWindowAnchor::Supplied(DynamicValue::F32(1.0)));
    let driver = encode_expression_driver_inputs(&[ExpressionDriverInput {
        name: "x".into(),
        property: DriverProperty {
            component: 1,
            offset: 0,
        },
    }])
    .unwrap();
    write!(out, "}},\"windowHeader\":{:?},\"windowCount\":{},\"windowRange\":{},\"anchorLatest\":{},\"anchorHostTime\":{},\"anchorSupplied\":{},\"driverHeader\":{:?}}}", &count[..6], count[8], latest[8], latest[20], host[20], supplied[20], &driver[..6]).unwrap();
    out
}
