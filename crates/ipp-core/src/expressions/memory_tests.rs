use super::*;

fn storage_bytes(declaration: &ExpressionDeclaration, plan: &PreparedExpression) -> usize {
    declaration.inputs.capacity() * size_of::<ExpressionInput>()
        + declaration.nodes.capacity() * size_of::<ExpressionNode>()
        + std::mem::size_of_val(plan.input_slots())
        + plan.operation_count() * size_of::<Instruction>()
        + declaration
            .inputs
            .iter()
            .map(|input| input.name.capacity())
            .sum::<usize>()
        + plan
            .input_slots()
            .iter()
            .map(|input| input.name.capacity())
            .sum::<usize>()
}

#[test]
fn vector_capacity_and_independent_prepared_names_are_retained_storage() {
    let mut declaration = ExpressionDeclaration {
        inputs: Vec::with_capacity(8),
        nodes: Vec::with_capacity(12),
        output: 0,
    };
    let mut name = String::with_capacity(40);
    name.push_str("sample");
    declaration.inputs.push(ExpressionInput {
        name,
        kind: DynamicPropertyKind::F32,
    });
    declaration.nodes.push(ExpressionNode::Input(0));
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    let expected = 8 * size_of::<ExpressionInput>()
        + 12 * size_of::<ExpressionNode>()
        + size_of::<ExpressionInput>()
        + size_of::<Instruction>()
        + 40
        + plan.input_slots()[0].name.capacity();

    assert_eq!(resident_bytes(&declaration, &plan), expected);
    assert_ne!(
        declaration.inputs[0].name.as_ptr(),
        plan.input_slots()[0].name.as_ptr()
    );
}

#[test]
fn expanded_shared_text_and_unreachable_constants_count_each_allocation_once() {
    let shared: Arc<str> = "shared 🦀".into();
    let distinct: Arc<str> = shared.as_ref().into();
    assert!(!Arc::ptr_eq(&shared, &distinct));
    let declaration = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![
            ExpressionNode::Constant(DynamicValue::Text(shared.clone())),
            ExpressionNode::Binary {
                operator: BinaryOperator::Equal,
                left: 0,
                right: 0,
            },
            ExpressionNode::Constant(DynamicValue::Text(shared.clone())),
            ExpressionNode::Constant(DynamicValue::Text(distinct.clone())),
        ],
        output: 1,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    assert_eq!(plan.operation_count(), 3);
    let expected = storage_bytes(&declaration, &plan) + shared.len() + distinct.len();
    let clone = plan.clone();

    assert_eq!(resident_bytes(&declaration, &plan), expected);
    assert_eq!(resident_bytes(&declaration, &clone), expected);
    assert!(Arc::ptr_eq(&plan.inputs, &clone.inputs));
    assert!(Arc::ptr_eq(&plan.instructions, &clone.instructions));
}

#[test]
fn matrices_are_inline_and_empty_text_has_no_payload_bytes() {
    let declaration = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![
            ExpressionNode::Constant(DynamicValue::Mat4([0.0; 16])),
            ExpressionNode::Constant(DynamicValue::Text("".into())),
        ],
        output: 0,
    };
    let plan = PreparedExpression::prepare(&declaration).unwrap();

    assert_eq!(
        resident_bytes(&declaration, &plan),
        storage_bytes(&declaration, &plan)
    );
}
