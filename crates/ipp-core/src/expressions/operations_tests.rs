use super::*;

fn binary(operator: BinaryOperator, a: DynamicValue, b: DynamicValue) -> ExpressionResult {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            ExpressionInput {
                name: "a".into(),
                kind: a.kind(),
            },
            ExpressionInput {
                name: "b".into(),
                kind: b.kind(),
            },
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Binary {
                operator,
                left: 0,
                right: 1,
            },
        ],
        output: 2,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    plan.evaluate(&mut plan.scratch(), &[Some(&a), Some(&b)])
        .unwrap()
        .clone()
}

#[test]
fn integer_comparison_and_arithmetic_preserve_all_bits() {
    use BinaryOperator as O;
    use DynamicValue as V;
    for (a, b) in [
        (V::I32(16_777_217), V::I32(16_777_216)),
        (V::U32(u32::MAX), V::U32(u32::MAX - 1)),
    ] {
        assert_eq!(
            binary(O::Equal, a.clone(), b.clone()),
            ExpressionResult::Valid(V::Bool(false))
        );
        assert_eq!(
            binary(O::Greater, a.clone(), b.clone()),
            ExpressionResult::Valid(V::Bool(true))
        );
        assert_eq!(
            binary(O::Less, b.clone(), a.clone()),
            ExpressionResult::Valid(V::Bool(true))
        );
        let expected = match a {
            V::I32(_) => V::I32(1),
            _ => V::U32(1),
        };
        assert_eq!(binary(O::Subtract, a, b), ExpressionResult::Valid(expected));
    }
    assert_eq!(
        binary(O::Add, V::U32(16_777_216), V::U32(1)),
        ExpressionResult::Valid(V::U32(16_777_217))
    );
    assert_eq!(
        binary(O::Divide, V::I32(-7), V::I32(2)),
        ExpressionResult::Valid(V::I32(-3))
    );
    assert_eq!(
        binary(O::Divide, V::U32(u32::MAX), V::U32(2)),
        ExpressionResult::Valid(V::U32(2_147_483_647))
    );

    for (operator, a, b) in [
        (O::Add, V::I32(i32::MAX), V::I32(1)),
        (O::Subtract, V::I32(i32::MIN), V::I32(1)),
        (O::Multiply, V::I32(i32::MAX), V::I32(2)),
        (O::Divide, V::I32(i32::MIN), V::I32(-1)),
        (O::Divide, V::I32(1), V::I32(0)),
        (O::Add, V::U32(u32::MAX), V::U32(1)),
        (O::Subtract, V::U32(0), V::U32(1)),
        (O::Multiply, V::U32(u32::MAX), V::U32(2)),
        (O::Divide, V::U32(1), V::U32(0)),
        (O::Multiply, V::F32(f32::MAX), V::F32(2.0)),
        (O::Divide, V::F32(1.0), V::F32(-0.0)),
        (O::Multiply, V::Vec2([1.0, f32::MAX]), V::Vec2([2.0, 2.0])),
    ] {
        assert_eq!(
            binary(operator, a, b),
            ExpressionResult::Invalid(ExpressionInvalid::Calculation)
        );
    }
}

#[test]
fn signed_unary_overflow_is_invalid_and_recovers() {
    for operator in [UnaryOperator::Absolute, UnaryOperator::Negate] {
        let declaration = ExpressionDeclaration {
            inputs: vec![ExpressionInput {
                name: "x".into(),
                kind: DynamicPropertyKind::I32,
            }],
            nodes: vec![
                ExpressionNode::Input(0),
                ExpressionNode::Unary {
                    operator,
                    operand: 0,
                },
            ],
            output: 1,
        };
        let plan = PreparedExpression::prepare(&declaration).unwrap();
        let mut scratch = plan.scratch();
        assert_eq!(
            plan.evaluate(&mut scratch, &[Some(&DynamicValue::I32(i32::MIN))]),
            Ok(&ExpressionResult::Invalid(ExpressionInvalid::Calculation))
        );
        assert_eq!(
            plan.evaluate(&mut scratch, &[Some(&DynamicValue::I32(-7))]),
            Ok(&ExpressionResult::Valid(DynamicValue::I32(7)))
        );
    }
}

#[test]
fn nonfinite_input_lanes_are_invalid_before_identity_equality_or_minimum() {
    use DynamicValue as V;
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for (value, good) in [
            (V::F32(bad), V::F32(3.0)),
            (V::Vec2([1.0, bad]), V::Vec2([1.0, 2.0])),
            (V::Vec3([1.0, bad, 3.0]), V::Vec3([1.0, 2.0, 3.0])),
            (V::Vec4([1.0, 2.0, 3.0, bad]), V::Vec4([1.0, 2.0, 3.0, 4.0])),
            (V::Mat2([1.0, 2.0, bad, 4.0]), V::Mat2([1.0; 4])),
            (V::Mat3([bad; 9]), V::Mat3([1.0; 9])),
            (V::Mat4([bad; 16]), V::Mat4([1.0; 16])),
        ] {
            assert_eq!(
                binary(BinaryOperator::Equal, value.clone(), good.clone()),
                ExpressionResult::Invalid(ExpressionInvalid::InvalidInput {
                    slot: 0
                })
            );
            let mut declaration = ExpressionDeclaration {
                inputs: vec![ExpressionInput {
                    name: "x".into(),
                    kind: value.kind(),
                }],
                nodes: vec![ExpressionNode::Input(0)],
                output: 0,
            };
            let plan = PreparedExpression::prepare(&declaration).unwrap();
            let mut scratch = plan.scratch();
            assert_eq!(
                plan.evaluate(&mut scratch, &[Some(&value)]),
                Ok(&ExpressionResult::Invalid(
                    ExpressionInvalid::InvalidInput {
                        slot: 0
                    }
                ))
            );
            assert_eq!(
                plan.evaluate(&mut scratch, &[Some(&good)]),
                Ok(&ExpressionResult::Valid(good))
            );
            declaration.nodes[0] = ExpressionNode::Constant(value);
            assert_eq!(
                PreparedExpression::prepare(&declaration).unwrap_err(),
                ExpressionPrepareError::InvalidConstant
            );
        }
        assert_eq!(
            binary(BinaryOperator::Minimum, V::F32(1.0), V::F32(bad)),
            ExpressionResult::Invalid(ExpressionInvalid::InvalidInput {
                slot: 1
            })
        );
    }
}
