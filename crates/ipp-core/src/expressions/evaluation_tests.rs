use super::*;
use std::sync::Arc;

fn input(name: &str, kind: DynamicPropertyKind) -> ExpressionInput {
    ExpressionInput {
        name: name.into(),
        kind,
    }
}

fn run<'a>(
    plan: &PreparedExpression,
    scratch: &'a mut ExpressionScratch,
    values: &[Option<&DynamicValue>],
) -> &'a ExpressionResult {
    plan.evaluate(scratch, values).unwrap()
}

#[test]
fn scalar_and_vector_results_are_checked_and_recover() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("a", DynamicPropertyKind::Vec3),
            input("b", DynamicPropertyKind::Vec3),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Binary {
                operator: BinaryOperator::Divide,
                left: 0,
                right: 1,
            },
        ],
        output: 2,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let a = DynamicValue::Vec3([8.0, 6.0, 12.0]);
    let zero = DynamicValue::Vec3([2.0, 0.0, 4.0]);
    let b = DynamicValue::Vec3([2.0, 3.0, 4.0]);
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&a), Some(&zero)]),
        &ExpressionResult::Invalid(ExpressionInvalid::Calculation)
    );
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&a), Some(&b)]),
        &ExpressionResult::Valid(DynamicValue::Vec3([4.0, 2.0, 3.0]))
    );
    assert_eq!(
        run(&plan, &mut scratch, &[None, Some(&b)]),
        &ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })
    );
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&a), Some(&b)]),
        &ExpressionResult::Valid(DynamicValue::Vec3([4.0, 2.0, 3.0]))
    );

    let int = ExpressionDeclaration {
        inputs: vec![
            input("a", DynamicPropertyKind::I32),
            input("b", DynamicPropertyKind::I32),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Binary {
                operator: BinaryOperator::Add,
                left: 0,
                right: 1,
            },
        ],
        output: 2,
    };
    let plan = PreparedExpression::prepare(&int).unwrap();
    let mut scratch = plan.scratch();
    assert_eq!(
        run(
            &plan,
            &mut scratch,
            &[
                Some(&DynamicValue::I32(i32::MAX)),
                Some(&DynamicValue::I32(1))
            ]
        ),
        &ExpressionResult::Invalid(ExpressionInvalid::Calculation)
    );
}

#[test]
fn ternary_and_fallback_skip_unused_invalid_branches() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("choose", DynamicPropertyKind::Bool),
            input("value", DynamicPropertyKind::F32),
            input("denominator", DynamicPropertyKind::F32),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Input(2),
            ExpressionNode::Binary {
                operator: BinaryOperator::Divide,
                left: 1,
                right: 2,
            },
            ExpressionNode::Constant(DynamicValue::F32(7.0)),
            ExpressionNode::Ternary {
                condition: 0,
                then_node: 4,
                else_node: 3,
            },
            ExpressionNode::Fallback {
                value: 5,
                replacement: 4,
            },
        ],
        output: 6,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let yes = DynamicValue::Bool(true);
    let no = DynamicValue::Bool(false);
    let value = DynamicValue::F32(12.0);
    let zero = DynamicValue::F32(0.0);
    let three = DynamicValue::F32(3.0);
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&yes), None, None]),
        &ExpressionResult::Valid(DynamicValue::F32(7.0))
    );
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&no), Some(&value), Some(&zero)]),
        &ExpressionResult::Valid(DynamicValue::F32(7.0))
    );
    assert_eq!(
        run(
            &plan,
            &mut scratch,
            &[Some(&no), Some(&value), Some(&three)]
        ),
        &ExpressionResult::Valid(DynamicValue::F32(4.0))
    );
    assert_eq!(
        run(&plan, &mut scratch, &[None, None, None]),
        &ExpressionResult::Valid(DynamicValue::F32(7.0))
    );

    let mut plain = declaration;
    plain.output = 5;
    let plan = PreparedExpression::prepare(&plain).unwrap();
    let mut scratch = plan.scratch();
    assert_eq!(
        run(&plan, &mut scratch, &[None, None, None]),
        &ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })
    );
}

#[test]
fn unicode_scalar_length_and_type_errors() {
    let declaration = ExpressionDeclaration {
        inputs: vec![input("text", DynamicPropertyKind::Text)],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Unary {
                operator: UnaryOperator::Length,
                operand: 0,
            },
        ],
        output: 1,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let text = DynamicValue::Text(Arc::from("é🙂e\u{301}"));
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&text)]),
        &ExpressionResult::Valid(DynamicValue::U32(4))
    );
    assert_eq!(
        plan.evaluate(&mut scratch, &[Some(&DynamicValue::F32(2.0))]),
        Err(ExpressionInputError::WrongType {
            slot: 0
        })
    );
    assert_eq!(
        plan.evaluate(&mut scratch, &[]),
        Err(ExpressionInputError::WrongCount)
    );
}

#[test]
fn identity_preserves_existing_matrix_value() {
    let declaration = ExpressionDeclaration {
        inputs: vec![input("matrix", DynamicPropertyKind::Mat2)],
        nodes: vec![ExpressionNode::Input(0)],
        output: 0,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let matrix = DynamicValue::Mat2([1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&matrix)]),
        &ExpressionResult::Valid(matrix)
    );
}

#[test]
fn rejects_bad_graphs_and_isolates_scratch_instances() {
    let declaration = ExpressionDeclaration {
        inputs: vec![input("x", DynamicPropertyKind::F32)],
        nodes: vec![ExpressionNode::Input(0)],
        output: 0,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut one = plan.scratch();
    let mut two = plan.scratch();
    assert_eq!(
        run(&plan, &mut one, &[Some(&DynamicValue::F32(5.0))]),
        &ExpressionResult::Valid(DynamicValue::F32(5.0))
    );
    let replacement_plan = PreparedExpression::prepare(&declaration).unwrap();
    assert_eq!(
        replacement_plan.evaluate(&mut one, &[Some(&DynamicValue::F32(5.0))]),
        Err(ExpressionInputError::StaleScratch)
    );
    assert_eq!(
        run(&plan, &mut two, &[None]),
        &ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })
    );
    assert_eq!(
        run(&plan, &mut one, &[Some(&DynamicValue::F32(5.0))]),
        &ExpressionResult::Valid(DynamicValue::F32(5.0))
    );

    let cycle = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Unary {
            operator: UnaryOperator::Negate,
            operand: 0,
        }],
        output: 0,
    };
    assert_eq!(
        PreparedExpression::prepare(&cycle).unwrap_err(),
        ExpressionPrepareError::Cycle
    );
    let wrong = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![
            ExpressionNode::Constant(DynamicValue::Bool(true)),
            ExpressionNode::Unary {
                operator: UnaryOperator::Length,
                operand: 0,
            },
        ],
        output: 1,
    };
    assert_eq!(
        PreparedExpression::prepare(&wrong).unwrap_err(),
        ExpressionPrepareError::WrongType
    );
    let reference = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Input(3)],
        output: 0,
    };
    assert_eq!(
        PreparedExpression::prepare(&reference).unwrap_err(),
        ExpressionPrepareError::InvalidReference
    );
}

#[test]
fn repeated_scalar_and_row_evaluation_keep_fixed_scratch_capacity() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("x", DynamicPropertyKind::F32),
            input("y", DynamicPropertyKind::F32),
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
    let scalar_capacity = scalar.results.capacity();
    let row_capacity = rows.results.capacity();
    let y = DynamicValue::F32(3.0);
    let start = std::time::Instant::now();
    for i in 0..10_000 {
        let x = DynamicValue::F32(i as f32);
        let actual = run(&plan, &mut scalar, &[Some(&x), Some(&y)]);
        assert_eq!(
            actual,
            &ExpressionResult::Valid(DynamicValue::F32(i as f32 * 3.0 + 2.0))
        );
    }
    let scalar_time = start.elapsed();
    let source_rows: Vec<_> = (0..10_000).map(|i| DynamicValue::F32(i as f32)).collect();
    let start = std::time::Instant::now();
    for (i, x) in source_rows.iter().enumerate() {
        let actual = run(&plan, &mut rows, &[Some(x), Some(&y)]);
        assert_eq!(
            actual,
            &ExpressionResult::Valid(DynamicValue::F32(i as f32 * 3.0 + 2.0))
        );
    }
    let row_time = start.elapsed();
    assert_eq!(scalar.results.capacity(), scalar_capacity);
    assert_eq!(rows.results.capacity(), row_capacity);
    eprintln!("expression 10k scalar={scalar_time:?} row={row_time:?}");
}

#[test]
fn shared_nodes_in_branches_do_not_read_skipped_results() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("choose", DynamicPropertyKind::Bool),
            input("x", DynamicPropertyKind::F32),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Constant(DynamicValue::F32(2.0)),
            ExpressionNode::Binary {
                operator: BinaryOperator::Multiply,
                left: 1,
                right: 2,
            },
            ExpressionNode::Ternary {
                condition: 0,
                then_node: 3,
                else_node: 1,
            },
            ExpressionNode::Binary {
                operator: BinaryOperator::Add,
                left: 4,
                right: 1,
            },
        ],
        output: 5,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let x = DynamicValue::F32(3.0);
    let yes = DynamicValue::Bool(true);
    let no = DynamicValue::Bool(false);
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&yes), Some(&x)]),
        &ExpressionResult::Valid(DynamicValue::F32(9.0))
    );
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&no), Some(&x)]),
        &ExpressionResult::Valid(DynamicValue::F32(6.0))
    );
}

#[test]
fn clamp_requires_authored_bounds_and_rejects_reversed_bounds() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("x", DynamicPropertyKind::F32),
            input("lo", DynamicPropertyKind::F32),
            input("hi", DynamicPropertyKind::F32),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Input(2),
            ExpressionNode::Clamp {
                value: 0,
                minimum: 1,
                maximum: 2,
            },
        ],
        output: 3,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let x = DynamicValue::F32(10.0);
    let lo = DynamicValue::F32(0.0);
    let hi = DynamicValue::F32(5.0);
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&x), Some(&lo), Some(&hi)]),
        &ExpressionResult::Valid(DynamicValue::F32(5.0))
    );
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&x), Some(&hi), Some(&lo)]),
        &ExpressionResult::Invalid(ExpressionInvalid::Calculation)
    );
}

#[test]
fn nonfinite_inputs_cannot_turn_into_valid_comparisons() {
    let declaration = ExpressionDeclaration {
        inputs: vec![input("x", DynamicPropertyKind::F32)],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Constant(DynamicValue::F32(0.0)),
            ExpressionNode::Binary {
                operator: BinaryOperator::Less,
                left: 0,
                right: 1,
            },
        ],
        output: 2,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    assert_eq!(
        run(&plan, &mut scratch, &[Some(&DynamicValue::F32(f32::NAN))]),
        &ExpressionResult::Invalid(ExpressionInvalid::InvalidInput {
            slot: 0
        })
    );
}

#[test]
fn lazy_paths_skip_text_reads_and_preserve_exact_selected_values() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("choose", DynamicPropertyKind::Bool),
            input("text", DynamicPropertyKind::Text),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Constant(DynamicValue::Text(Arc::from("other"))),
            ExpressionNode::Ternary {
                condition: 0,
                then_node: 1,
                else_node: 2,
            },
            ExpressionNode::Fallback {
                value: 3,
                replacement: 1,
            },
        ],
        output: 4,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let text = Arc::<str>::from("selected");
    let value = DynamicValue::Text(Arc::clone(&text));
    let original_count = Arc::strong_count(&text);
    assert_eq!(
        run(
            &plan,
            &mut scratch,
            &[Some(&DynamicValue::Bool(false)), Some(&value)]
        ),
        &ExpressionResult::Valid(DynamicValue::Text(Arc::from("other")))
    );
    // No retained clone proves the skipped input/replacement was never read.
    assert_eq!(Arc::strong_count(&text), original_count);
    let ExpressionResult::Valid(DynamicValue::Text(selected)) = run(
        &plan,
        &mut scratch,
        &[Some(&DynamicValue::Bool(true)), Some(&value)],
    ) else {
        panic!("selected text")
    };
    assert!(Arc::ptr_eq(selected, &text));
    assert_eq!(
        run(
            &plan,
            &mut scratch,
            &[Some(&DynamicValue::Bool(false)), None]
        ),
        &ExpressionResult::Valid(DynamicValue::Text(Arc::from("other")))
    );
    assert_eq!(
        run(&plan, &mut scratch, &[None, None]),
        &ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
            slot: 1
        })
    );
}

#[test]
fn invalid_inputs_follow_lazy_selection_and_explicit_fallback() {
    let declaration = ExpressionDeclaration {
        inputs: vec![
            input("choose", DynamicPropertyKind::Bool),
            input("x", DynamicPropertyKind::F32),
        ],
        nodes: vec![
            ExpressionNode::Input(0),
            ExpressionNode::Input(1),
            ExpressionNode::Constant(DynamicValue::F32(9.0)),
            ExpressionNode::Fallback {
                value: 1,
                replacement: 2,
            },
            ExpressionNode::Ternary {
                condition: 0,
                then_node: 2,
                else_node: 3,
            },
        ],
        output: 4,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let mut scratch = plan.scratch();
    let nan = DynamicValue::F32(f32::NAN);
    for choose in [true, false, true, false] {
        assert_eq!(
            run(
                &plan,
                &mut scratch,
                &[Some(&DynamicValue::Bool(choose)), Some(&nan)]
            ),
            &ExpressionResult::Valid(DynamicValue::F32(9.0))
        );
    }
    assert_eq!(
        run(&plan, &mut scratch, &[None, Some(&nan)]),
        &ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
            slot: 0
        })
    );
    assert_eq!(
        run(
            &plan,
            &mut scratch,
            &[
                Some(&DynamicValue::Bool(false)),
                Some(&DynamicValue::F32(5.0))
            ]
        ),
        &ExpressionResult::Valid(DynamicValue::F32(5.0))
    );
    // Binding type errors remain eager, including an unselected input.
    assert_eq!(
        plan.evaluate(
            &mut scratch,
            &[Some(&DynamicValue::Bool(true)), Some(&DynamicValue::I32(5))]
        ),
        Err(ExpressionInputError::WrongType {
            slot: 1
        })
    );
}
