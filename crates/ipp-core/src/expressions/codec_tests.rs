use super::*;

// These fixtures specify the format independently of the production encoder,
// its tag mappings and its public constants. Whitespace only aids review.
fn hex(source: &str) -> Vec<u8> {
    source
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

fn payload(inputs: u32, nodes: u32, output: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = b"IPPE\x01\x00\x00\x00".to_vec();
    for count in [inputs, nodes, output] {
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    bytes.extend_from_slice(body);
    bytes
}

fn golden() -> Vec<u8> {
    hex("49 50 50 45 01 00 00 00 02 00 00 00 0a 00 00 00 00 00 00 00
         06 00 00 00 63 68 6f 6f 73 65 04
         05 00 00 00 76 61 6c 75 65 03
         05 01 00 00 00 02 00 00 00 03 00 00 00
         00 00 00 00 00
         03 00 04 00 00 00 04 00 00 00
         06 05 00 00 00 06 00 00 00
         01 03 01 00 00 01
         00 01 00 00 00
         01 03 ff ff ff ff
         01 0d 06 00 00 00 c3 a9 f0 9f 99 82
         01 08 00 00 80 3f 00 00 00 40 00 00 40 40 00 00 80 40
         02 03 07 00 00 00")
}

#[test]
fn independent_golden_preserves_forward_shared_and_lazy_declarations() {
    let expected = ExpressionDeclaration {
        inputs: vec![
            ExpressionInput {
                name: "choose".into(),
                kind: DynamicPropertyKind::Bool,
            },
            ExpressionInput {
                name: "value".into(),
                kind: DynamicPropertyKind::U32,
            },
        ],
        nodes: vec![
            ExpressionNode::Ternary {
                condition: 1,
                then_node: 2,
                else_node: 3,
            },
            ExpressionNode::Input(0),
            ExpressionNode::Binary {
                operator: BinaryOperator::Add,
                left: 4,
                right: 4,
            },
            ExpressionNode::Fallback {
                value: 5,
                replacement: 6,
            },
            ExpressionNode::Constant(DynamicValue::U32(16_777_217)),
            ExpressionNode::Input(1),
            ExpressionNode::Constant(DynamicValue::U32(u32::MAX)),
            ExpressionNode::Constant(DynamicValue::Text(Arc::from("é🙂"))),
            ExpressionNode::Constant(DynamicValue::Mat2([1.0, 2.0, 3.0, 4.0])),
            ExpressionNode::Unary {
                operator: UnaryOperator::Length,
                operand: 7,
            },
        ],
        output: 0,
    };
    let bytes = golden();
    assert_eq!(expected.encode().unwrap(), bytes);
    let decoded = ExpressionDeclaration::decode(&bytes).unwrap();
    assert_eq!(decoded, expected);
    let original = PreparedExpression::prepare(&expected).unwrap();
    let plan = PreparedExpression::prepare(&decoded).unwrap();
    assert_eq!(plan.input_slots(), original.input_slots());
    assert_eq!(plan.output_kind(), DynamicPropertyKind::U32);
    assert_eq!(plan.operation_count(), original.operation_count());
    let mut scratch = plan.scratch();
    let mut original_scratch = original.scratch();
    let yes = DynamicValue::Bool(true);
    let no = DynamicValue::Bool(false);
    let seven = DynamicValue::U32(7);
    for (inputs, result) in [
        (
            [Some(&yes), None],
            ExpressionResult::Valid(DynamicValue::U32(33_554_434)),
        ),
        (
            [Some(&no), None],
            ExpressionResult::Valid(DynamicValue::U32(u32::MAX)),
        ),
        (
            [Some(&no), Some(&seven)],
            ExpressionResult::Valid(DynamicValue::U32(7)),
        ),
        (
            [None, Some(&seven)],
            ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
                slot: 0,
            }),
        ),
    ] {
        assert_eq!(plan.evaluate(&mut scratch, &inputs).unwrap(), &result);
        assert_eq!(
            original.evaluate(&mut original_scratch, &inputs).unwrap(),
            &result
        );
    }
    // Preparing/evaluating does not change logical bytes or serialize scratch.
    assert_eq!(decoded.encode().unwrap(), bytes);
}

fn constant_cases() -> Vec<(DynamicValue, Vec<u8>)> {
    vec![
        (DynamicValue::F32(-0.0), hex("01 00 00 00 80")),
        (DynamicValue::I32(i32::MIN), hex("02 00 00 00 80")),
        (DynamicValue::I32(i32::MAX), hex("02 ff ff ff 7f")),
        (DynamicValue::I32(-16_777_217), hex("02 ff ff ff fe")),
        (DynamicValue::U32(u32::MAX), hex("03 ff ff ff ff")),
        (DynamicValue::Bool(true), hex("04 01 00 00 00")),
        (DynamicValue::Bool(false), hex("04 00 00 00 00")),
        (
            DynamicValue::Vec2([1.0, -2.0]),
            hex("05 00 00 80 3f 00 00 00 c0"),
        ),
        (
            DynamicValue::Vec3([1.0, -2.0, 0.5]),
            hex("06 00 00 80 3f 00 00 00 c0 00 00 00 3f"),
        ),
        (
            DynamicValue::Vec4([1.0, -2.0, 0.5, 4.0]),
            hex("07 00 00 80 3f 00 00 00 c0 00 00 00 3f 00 00 80 40"),
        ),
        (
            DynamicValue::Mat2([1.0, -2.0, 0.5, 4.0]),
            hex("08 00 00 80 3f 00 00 00 c0 00 00 00 3f 00 00 80 40"),
        ),
        (
            DynamicValue::Mat3([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]),
            hex(
                "09 00 00 80 3f 00 00 00 40 00 00 40 40 00 00 80 40 00 00 a0 40 00 00 c0 40 00 00 e0 40 00 00 00 41 00 00 10 41",
            ),
        ),
        (
            DynamicValue::Mat4([
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
                16.0,
            ]),
            hex(
                "0a 00 00 80 3f 00 00 00 40 00 00 40 40 00 00 80 40 00 00 a0 40 00 00 c0 40 00 00 e0 40 00 00 00 41 00 00 10 41 00 00 20 41 00 00 30 41 00 00 40 41 00 00 50 41 00 00 60 41 00 00 70 41 00 00 80 41",
            ),
        ),
        (DynamicValue::Text(Arc::from("")), hex("0d 00 00 00 00")),
        (
            DynamicValue::Text(Arc::from("é🙂\0")),
            hex("0d 07 00 00 00 c3 a9 f0 9f 99 82 00"),
        ),
    ]
}

#[test]
fn independent_constants_and_input_tags_cover_every_exact_core_type() {
    for (value, wire) in constant_cases() {
        let mut body = vec![1]; // Constant node, followed by exact typed value bytes.
        body.extend_from_slice(&wire);
        let bytes = payload(0, 1, 0, &body);
        let declaration = ExpressionDeclaration {
            inputs: vec![],
            nodes: vec![ExpressionNode::Constant(value.clone())],
            output: 0,
        };
        assert_eq!(declaration.encode().unwrap(), bytes);
        let decoded = ExpressionDeclaration::decode(&bytes).unwrap();
        assert_eq!(decoded, declaration);
        // Bit identity (including negative zero) is stronger than float equality.
        assert_eq!(decoded.encode().unwrap(), bytes);
        let plan = PreparedExpression::prepare(&decoded).unwrap();
        assert_eq!(
            plan.evaluate(&mut plan.scratch(), &[]).unwrap(),
            &ExpressionResult::Valid(value.clone())
        );

        let mut input_body = hex("06 00 00 00 c3 a9 f0 9f 99 82"); // Unicode input name.
        input_body.push(wire[0]);
        input_body.extend_from_slice(&[0, 0, 0, 0, 0]);
        let input_bytes = payload(1, 1, 0, &input_body);
        let decoded = ExpressionDeclaration::decode(&input_bytes).unwrap();
        assert_eq!(
            decoded.inputs,
            vec![ExpressionInput {
                name: "é🙂".into(),
                kind: value.kind()
            }]
        );
        assert_eq!(decoded.nodes, vec![ExpressionNode::Input(0)]);
        assert_eq!(decoded.encode().unwrap(), input_bytes);
        let plan = PreparedExpression::prepare(&decoded).unwrap();
        assert_eq!(
            plan.evaluate(&mut plan.scratch(), &[Some(&value)]).unwrap(),
            &ExpressionResult::Valid(value)
        );
    }
}

#[test]
fn independent_operator_tags_and_clamp_have_expected_results() {
    let literals = hex(
        "01 02 fd ff ff ff 01 02 02 00 00 00 01 04 01 00 00 00 01 04 00 00 00 00 01 0d 05 00 00 00 65 f0 9f 99 82",
    );
    let operators = [
        (
            hex("02 00 00 00 00 00"),
            ExpressionNode::Unary {
                operator: UnaryOperator::Negate,
                operand: 0,
            },
            DynamicValue::I32(3),
        ),
        (
            hex("02 01 00 00 00 00"),
            ExpressionNode::Unary {
                operator: UnaryOperator::Absolute,
                operand: 0,
            },
            DynamicValue::I32(3),
        ),
        (
            hex("02 02 02 00 00 00"),
            ExpressionNode::Unary {
                operator: UnaryOperator::Not,
                operand: 2,
            },
            DynamicValue::Bool(false),
        ),
        (
            hex("02 03 04 00 00 00"),
            ExpressionNode::Unary {
                operator: UnaryOperator::Length,
                operand: 4,
            },
            DynamicValue::U32(2),
        ),
    ];
    for (wire, node, expected) in operators {
        check_operator(&literals, &wire, node, expected);
    }
    for (tag, operator, expected) in [
        (0, BinaryOperator::Add, DynamicValue::I32(-1)),
        (1, BinaryOperator::Subtract, DynamicValue::I32(-5)),
        (2, BinaryOperator::Multiply, DynamicValue::I32(-6)),
        (3, BinaryOperator::Divide, DynamicValue::I32(-1)),
        (4, BinaryOperator::Minimum, DynamicValue::I32(-3)),
        (5, BinaryOperator::Maximum, DynamicValue::I32(2)),
        (6, BinaryOperator::Equal, DynamicValue::Bool(false)),
        (7, BinaryOperator::Less, DynamicValue::Bool(true)),
        (8, BinaryOperator::Greater, DynamicValue::Bool(false)),
        (9, BinaryOperator::And, DynamicValue::Bool(false)),
        (10, BinaryOperator::Or, DynamicValue::Bool(true)),
    ] {
        let (left, right) = if tag >= 9 {
            (2, 3)
        } else {
            (0, 1)
        };
        let wire = [3, tag, left, 0, 0, 0, right, 0, 0, 0];
        check_operator(
            &literals,
            &wire,
            ExpressionNode::Binary {
                operator,
                left: left as usize,
                right: right as usize,
            },
            expected,
        );
    }
    check_operator(
        &literals,
        &hex("04 00 00 00 00 01 00 00 00 01 00 00 00"),
        ExpressionNode::Clamp {
            value: 0,
            minimum: 1,
            maximum: 1,
        },
        DynamicValue::I32(2),
    );
}

fn check_operator(literals: &[u8], wire: &[u8], node: ExpressionNode, expected: DynamicValue) {
    let mut body = literals.to_vec();
    body.extend_from_slice(wire);
    let bytes = payload(0, 6, 5, &body);
    let declaration = ExpressionDeclaration::decode(&bytes).unwrap();
    assert_eq!(declaration.nodes[5], node);
    assert_eq!(declaration.encode().unwrap(), bytes);
    let plan = PreparedExpression::prepare(&declaration).unwrap();
    assert_eq!(
        plan.evaluate(&mut plan.scratch(), &[]).unwrap(),
        &ExpressionResult::Valid(expected)
    );
}

#[test]
fn every_truncated_golden_prefix_and_trailing_byte_is_rejected() {
    let bytes = golden();
    for length in 0..bytes.len() {
        assert!(
            ExpressionDeclaration::decode(&bytes[..length]).is_err(),
            "prefix {length}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(
        ExpressionDeclaration::decode(&trailing),
        Err(ExpressionCodecError::TrailingBytes)
    );
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert_eq!(
        ExpressionDeclaration::decode(&bad_magic),
        Err(ExpressionCodecError::InvalidHeader)
    );
    let mut version = bytes;
    version[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        ExpressionDeclaration::decode(&version),
        Err(ExpressionCodecError::UnsupportedVersion)
    );
}

#[test]
fn invalid_tags_utf8_booleans_and_nonfinite_lanes_are_rejected() {
    for tag in [0, 11, 12, 14, 255] {
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, 1, 0, &[1, tag, 0, 0, 0, 0])),
            Err(ExpressionCodecError::InvalidTag)
        );
        assert_eq!(
            ExpressionDeclaration::decode(&payload(
                1,
                1,
                0,
                &[1, 0, 0, 0, b'x', tag, 0, 0, 0, 0, 0]
            )),
            Err(ExpressionCodecError::InvalidTag)
        );
    }
    for body in [
        hex("ff 00 00 00 00"),
        hex("02 04 00 00 00 00"),
        hex("03 0b 00 00 00 00 00 00 00 00"),
    ] {
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, 1, 0, &body)),
            Err(ExpressionCodecError::InvalidTag)
        );
    }
    for body in [hex("01 0d 01 00 00 00 ff"), hex("01 0d 02 00 00 00 c3 00")] {
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, 1, 0, &body)),
            Err(ExpressionCodecError::InvalidUtf8)
        );
    }
    assert_eq!(
        ExpressionDeclaration::decode(&payload(1, 1, 0, &hex("01 00 00 00 ff 03 00 00 00 00 00"))),
        Err(ExpressionCodecError::InvalidUtf8)
    );
    for boolean in [2u32, u32::MAX] {
        let mut body = vec![1, 4];
        body.extend_from_slice(&boolean.to_le_bytes());
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, 1, 0, &body)),
            Err(ExpressionCodecError::InvalidValue)
        );
    }
    for (value, wire) in constant_cases() {
        if value.floats().is_none() {
            continue;
        }
        for bits in [0x7f800000u32, 0xff800000, 0x7fc00001] {
            for lane in 0..value.floats().unwrap().len() {
                let mut body = vec![1];
                body.extend_from_slice(&wire);
                body[2 + lane * 4..6 + lane * 4].copy_from_slice(&bits.to_le_bytes());
                assert_eq!(
                    ExpressionDeclaration::decode(&payload(0, 1, 0, &body)),
                    Err(ExpressionCodecError::InvalidValue)
                );
            }
        }
    }
}

#[test]
fn portable_counts_lengths_and_references_refuse_overflow_before_allocation() {
    for count in [65_537, u32::MAX] {
        assert_eq!(
            ExpressionDeclaration::decode(&payload(count, 1, 0, &[])),
            Err(ExpressionCodecError::LimitExceeded)
        );
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, count, 0, &[])),
            Err(ExpressionCodecError::LimitExceeded)
        );
    }
    assert_eq!(
        ExpressionDeclaration::decode(&payload(65_536, 65_536, 0, &[])),
        Err(ExpressionCodecError::Truncated)
    );
    for length in [1_048_577u32, u32::MAX] {
        let mut text = vec![1, 13];
        text.extend_from_slice(&length.to_le_bytes());
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, 1, 0, &text)),
            Err(ExpressionCodecError::LimitExceeded)
        );
        let mut name = length.to_le_bytes().to_vec();
        name.extend_from_slice(&hex("00 03 00 00 00 00 00"));
        assert_eq!(
            ExpressionDeclaration::decode(&payload(1, 1, 0, &name)),
            Err(ExpressionCodecError::LimitExceeded)
        );
    }
    assert_eq!(
        ExpressionDeclaration::decode(&payload(0, 1, u32::MAX, &hex("01 03 00 00 00 00"))),
        Err(ExpressionCodecError::InvalidDeclaration(
            ExpressionPrepareError::InvalidReference
        ))
    );
    for body in [hex("00 ff ff ff ff"), hex("02 00 ff ff ff ff")] {
        assert_eq!(
            ExpressionDeclaration::decode(&payload(0, 1, 0, &body)),
            Err(ExpressionCodecError::InvalidDeclaration(
                ExpressionPrepareError::InvalidReference
            ))
        );
    }
    if usize::BITS > 32 {
        let declaration = ExpressionDeclaration {
            inputs: vec![],
            nodes: vec![ExpressionNode::Input(usize::MAX)],
            output: 0,
        };
        assert_eq!(
            declaration.encode(),
            Err(ExpressionCodecError::LimitExceeded)
        );
    }
}

#[test]
fn decoded_unused_graphs_use_existing_semantic_and_complexity_validation() {
    for (bytes, error) in [
        (
            payload(0, 1, 0, &hex("02 00 00 00 00 00")),
            ExpressionPrepareError::Cycle,
        ),
        (
            payload(0, 2, 0, &hex("01 03 00 00 00 00 02 02 00 00 00 00")),
            ExpressionPrepareError::WrongType,
        ),
        (
            payload(1, 1, 0, &hex("00 00 00 00 03 00 00 00 00 00")),
            ExpressionPrepareError::EmptyInputName,
        ),
        (
            payload(
                2,
                1,
                0,
                &hex("01 00 00 00 78 03 01 00 00 00 78 03 00 00 00 00 00"),
            ),
            ExpressionPrepareError::DuplicateInputName,
        ),
    ] {
        assert_eq!(
            ExpressionDeclaration::decode(&bytes),
            Err(ExpressionCodecError::InvalidDeclaration(error))
        );
    }
    let mut body = hex("01 02 fd ff ff ff");
    for index in 1..257u32 {
        body.extend_from_slice(&[2, 1]);
        body.extend_from_slice(&(index - 1).to_le_bytes());
    }
    assert!(ExpressionDeclaration::decode(&payload(0, 256, 255, &body[..256 * 6])).is_ok());
    assert_eq!(
        ExpressionDeclaration::decode(&payload(0, 257, 256, &body)),
        Err(ExpressionCodecError::InvalidDeclaration(
            ExpressionPrepareError::TooComplex
        ))
    );
    let mut body = hex("01 03 01 00 00 00");
    for index in 1..17u32 {
        body.extend_from_slice(&[3, 4]);
        for _ in 0..2 {
            body.extend_from_slice(&(index - 1).to_le_bytes());
        }
    }
    assert!(ExpressionDeclaration::decode(&payload(0, 16, 15, &body[..6 + 15 * 10])).is_ok());
    assert_eq!(
        ExpressionDeclaration::decode(&payload(0, 17, 16, &body)),
        Err(ExpressionCodecError::InvalidDeclaration(
            ExpressionPrepareError::TooComplex
        ))
    );
}

#[test]
fn format_limits_accept_the_boundary_and_refuse_larger_payloads() {
    let text = "x".repeat(1_048_576);
    let declaration = ExpressionDeclaration {
        inputs: vec![ExpressionInput {
            name: text.clone(),
            kind: DynamicPropertyKind::Text,
        }],
        nodes: vec![ExpressionNode::Constant(DynamicValue::Text(Arc::from(
            text.as_str(),
        )))],
        output: 0,
    };
    let bytes = declaration.encode().unwrap();
    assert_eq!(ExpressionDeclaration::decode(&bytes).unwrap(), declaration);
    let mut too_long = declaration.clone();
    too_long.inputs[0].name.push('x');
    assert_eq!(too_long.encode(), Err(ExpressionCodecError::LimitExceeded));
    too_long = declaration.clone();
    too_long.nodes[0] = ExpressionNode::Constant(DynamicValue::Text(Arc::from(format!("{text}x"))));
    assert_eq!(too_long.encode(), Err(ExpressionCodecError::LimitExceeded));

    let body = hex("01 03 00 00 00 00").repeat(65_536);
    assert_eq!(
        ExpressionDeclaration::decode(&payload(0, 65_536, 0, &body))
            .unwrap()
            .nodes
            .len(),
        65_536
    );
    let mut body = Vec::new();
    for index in 0..65_536 {
        let name = index.to_string();
        body.extend_from_slice(&(name.len() as u32).to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.push(3);
    }
    body.extend_from_slice(&hex("01 03 00 00 00 00"));
    let decoded = ExpressionDeclaration::decode(&payload(65_536, 1, 0, &body)).unwrap();
    assert_eq!(decoded.inputs.len(), 65_536);
    assert_eq!(decoded.inputs[65_535].name, "65535");
    assert_eq!(decoded.encode().unwrap(), payload(65_536, 1, 0, &body));

    // Fifteen maximum-sized texts plus the remaining bytes exactly fill 16 MiB.
    let mut body = Vec::new();
    for _ in 0..15 {
        body.extend_from_slice(&[1, 13]);
        body.extend_from_slice(&1_048_576u32.to_le_bytes());
        body.extend_from_slice(text.as_bytes());
    }
    let last = 16_777_216 - 20 - body.len() - 6;
    body.extend_from_slice(&[1, 13]);
    body.extend_from_slice(&(last as u32).to_le_bytes());
    body.extend_from_slice(&text.as_bytes()[..last]);
    let mut full = payload(0, 16, 0, &body);
    let decoded = ExpressionDeclaration::decode(&full).unwrap();
    assert_eq!(decoded.encode().unwrap(), full);
    full.push(0);
    assert_eq!(
        ExpressionDeclaration::decode(&full),
        Err(ExpressionCodecError::LimitExceeded)
    );
    let mut too_big = decoded;
    too_big
        .nodes
        .push(ExpressionNode::Constant(DynamicValue::Bool(false)));
    assert_eq!(too_big.encode(), Err(ExpressionCodecError::LimitExceeded));
}

#[test]
fn encoding_refuses_semantically_invalid_authored_graphs() {
    let base = ExpressionDeclaration {
        inputs: vec![],
        nodes: vec![ExpressionNode::Constant(DynamicValue::U32(1))],
        output: 0,
    };
    for (node, error) in [
        (
            ExpressionNode::Constant(DynamicValue::F32(f32::INFINITY)),
            ExpressionPrepareError::InvalidConstant,
        ),
        (
            ExpressionNode::Unary {
                operator: UnaryOperator::Not,
                operand: 0,
            },
            ExpressionPrepareError::WrongType,
        ),
        (
            ExpressionNode::Input(0),
            ExpressionPrepareError::InvalidReference,
        ),
        (
            ExpressionNode::Fallback {
                value: 1,
                replacement: 0,
            },
            ExpressionPrepareError::Cycle,
        ),
    ] {
        let mut declaration = base.clone();
        declaration.nodes.push(node);
        assert_eq!(
            declaration.encode(),
            Err(ExpressionCodecError::InvalidDeclaration(error))
        );
    }
    let mut declaration = base;
    declaration.inputs.push(ExpressionInput {
        name: "asset".into(),
        kind: DynamicPropertyKind::Asset,
    });
    assert_eq!(declaration.encode(), Err(ExpressionCodecError::InvalidTag));
}

#[test]
fn arbitrary_single_byte_mutations_are_bounded_and_never_panic() {
    let bytes = golden();
    for index in 0..bytes.len() {
        for replacement in [0, 1, 0x7f, 0xff] {
            let mut mutated = bytes.clone();
            mutated[index] = replacement;
            if let Ok(declaration) = ExpressionDeclaration::decode(&mutated) {
                assert_eq!(declaration.encode().unwrap(), mutated);
                assert!(PreparedExpression::prepare(&declaration).is_ok());
            }
        }
    }
}
