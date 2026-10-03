use super::*;

fn chain(length: usize, forward_references: bool) -> ExpressionDeclaration {
    let mut nodes = vec![ExpressionNode::Constant(DynamicValue::I32(-3))];
    for index in 1..length {
        nodes.push(ExpressionNode::Unary {
            operator: UnaryOperator::Absolute,
            operand: index - 1,
        });
    }
    if forward_references {
        nodes.reverse();
        for node in &mut nodes {
            if let ExpressionNode::Unary {
                operand,
                ..
            } = node
            {
                *operand = length - 1 - *operand;
            }
        }
    }
    ExpressionDeclaration {
        inputs: vec![],
        nodes,
        output: if forward_references {
            0
        } else {
            length - 1
        },
    }
}

#[test]
fn dependency_depth_is_bounded_in_both_orders_on_a_small_stack() {
    // An unbounded recursive compiler can abort the process. After reproducing
    // the validation hole alone, exercise the fixed public API on a small stack.
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            for forward in [false, true] {
                let plan = PreparedExpression::prepare(&chain(256, forward)).unwrap();
                let mut scratch = plan.scratch();
                assert_eq!(plan.operation_count(), 256);
                assert_eq!(
                    plan.evaluate(&mut scratch, &[]),
                    Ok(&ExpressionResult::Valid(DynamicValue::I32(3)))
                );
                for length in [257, 65_536] {
                    assert_eq!(
                        PreparedExpression::prepare(&chain(length, forward)).unwrap_err(),
                        ExpressionPrepareError::TooComplex
                    );
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn exponentially_shared_dags_are_bounded_before_compilation() {
    let mut declaration = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Constant(DynamicValue::U32(u32::MAX))],
        output: 0,
    };
    for index in 1..16 {
        declaration.nodes.push(ExpressionNode::Binary {
            operator: BinaryOperator::Minimum,
            left: index - 1,
            right: index - 1,
        });
    }
    declaration.output = 15;
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    assert_eq!(plan.operation_count(), 65_535);
    let mut scratch = plan.scratch();
    assert_eq!(
        plan.evaluate(&mut scratch, &[]),
        Ok(&ExpressionResult::Valid(DynamicValue::U32(u32::MAX)))
    );

    declaration.nodes.push(ExpressionNode::Binary {
        operator: BinaryOperator::Minimum,
        left: 15,
        right: 15,
    });
    declaration.output = 16;
    assert_eq!(
        PreparedExpression::prepare(&declaration).unwrap_err(),
        ExpressionPrepareError::TooComplex
    );
}

#[test]
fn validates_unused_nodes_and_declared_bindings() {
    let base = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Constant(DynamicValue::F32(1.0))],
        output: 0,
    };
    for (node, expected) in [
        (
            ExpressionNode::Constant(DynamicValue::F32(f32::NAN)),
            ExpressionPrepareError::InvalidConstant,
        ),
        (
            ExpressionNode::Input(0),
            ExpressionPrepareError::InvalidReference,
        ),
        (
            ExpressionNode::Unary {
                operator: UnaryOperator::Not,
                operand: 0,
            },
            ExpressionPrepareError::WrongType,
        ),
        (
            ExpressionNode::Unary {
                operator: UnaryOperator::Absolute,
                operand: 1,
            },
            ExpressionPrepareError::Cycle,
        ),
    ] {
        let mut declaration = base.clone();
        declaration.nodes.push(node);
        assert_eq!(
            PreparedExpression::prepare(&declaration).unwrap_err(),
            expected
        );
    }
    for (inputs, expected) in [
        (
            vec![ExpressionInput {
                name: String::new(),
                kind: DynamicPropertyKind::F32,
            }],
            ExpressionPrepareError::EmptyInputName,
        ),
        (
            vec![
                ExpressionInput {
                    name: "x".into(),
                    kind: DynamicPropertyKind::F32
                };
                2
            ],
            ExpressionPrepareError::DuplicateInputName,
        ),
        (
            vec![ExpressionInput {
                name: "asset".into(),
                kind: DynamicPropertyKind::Asset,
            }],
            ExpressionPrepareError::UnsupportedType,
        ),
    ] {
        let mut declaration = base.clone();
        declaration.inputs = inputs;
        assert_eq!(
            PreparedExpression::prepare(&declaration).unwrap_err(),
            expected
        );
    }
    let mut declaration = base.clone();
    declaration.nodes.resize(65_537, base.nodes[0].clone());
    assert_eq!(
        PreparedExpression::prepare(&declaration).unwrap_err(),
        ExpressionPrepareError::TooComplex
    );
    declaration.nodes.clear();
    assert_eq!(
        PreparedExpression::prepare(&declaration).unwrap_err(),
        ExpressionPrepareError::InvalidReference
    );
}

#[test]
fn prepared_clones_share_storage_and_scratch_identity() {
    let declaration = ExpressionDeclaration {
        inputs: vec![ExpressionInput {
            name: "x".into(),
            kind: DynamicPropertyKind::I32,
        }],
        nodes: vec![ExpressionNode::Input(0)],
        output: 0,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let clone = plan.clone();
    assert!(Arc::ptr_eq(&plan.instructions, &clone.instructions));
    assert!(Arc::ptr_eq(&plan.inputs, &clone.inputs));
    let mut scratch = plan.scratch();
    assert_eq!(
        clone.evaluate(&mut scratch, &[Some(&DynamicValue::I32(17))]),
        Ok(&ExpressionResult::Valid(DynamicValue::I32(17)))
    );
    let before = scratch.results.clone();
    assert_eq!(
        clone.evaluate(&mut scratch, &[Some(&DynamicValue::U32(17))]),
        Err(ExpressionInputError::WrongType {
            slot: 0
        })
    );
    assert_eq!(scratch.results, before);
    assert_eq!(
        clone.evaluate(&mut scratch, &[]),
        Err(ExpressionInputError::WrongCount)
    );
    assert_eq!(scratch.results, before);

    let replacement = PreparedExpression::prepare(&declaration).unwrap();
    assert_eq!(
        replacement.evaluate(&mut scratch, &[None]),
        Err(ExpressionInputError::StaleScratch)
    );
    assert_eq!(scratch.results, before);
    std::thread::spawn(move || {
        let mut independent = clone.scratch();
        assert_eq!(
            clone.evaluate(&mut independent, &[Some(&DynamicValue::I32(21))]),
            Ok(&ExpressionResult::Valid(DynamicValue::I32(21)))
        );
    })
    .join()
    .unwrap();
}
