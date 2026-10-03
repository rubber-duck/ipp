use super::*;
use std::collections::HashSet;

fn supported(kind: DynamicPropertyKind) -> bool {
    matches!(
        kind,
        DynamicPropertyKind::F32
            | DynamicPropertyKind::I32
            | DynamicPropertyKind::U32
            | DynamicPropertyKind::Bool
            | DynamicPropertyKind::Vec2
            | DynamicPropertyKind::Vec3
            | DynamicPropertyKind::Vec4
            | DynamicPropertyKind::Mat2
            | DynamicPropertyKind::Mat3
            | DynamicPropertyKind::Mat4
            | DynamicPropertyKind::Text
    )
}

fn numeric(kind: DynamicPropertyKind) -> bool {
    matches!(
        kind,
        DynamicPropertyKind::F32
            | DynamicPropertyKind::I32
            | DynamicPropertyKind::U32
            | DynamicPropertyKind::Vec2
            | DynamicPropertyKind::Vec3
            | DynamicPropertyKind::Vec4
    )
}

fn signed_numeric(kind: DynamicPropertyKind) -> bool {
    numeric(kind) && kind != DynamicPropertyKind::U32
}

fn scalar_numeric(kind: DynamicPropertyKind) -> bool {
    matches!(
        kind,
        DynamicPropertyKind::F32 | DynamicPropertyKind::I32 | DynamicPropertyKind::U32
    )
}

fn check_type(
    declaration: &ExpressionDeclaration,
    index: usize,
    kinds: &[Option<DynamicPropertyKind>],
) -> Result<DynamicPropertyKind, ExpressionPrepareError> {
    let node = &declaration.nodes[index];
    let child = |reference: usize| kinds[reference].expect("dependency validated first");
    let kind = match node {
        ExpressionNode::Input(slot) => {
            declaration
                .inputs
                .get(*slot)
                .ok_or(ExpressionPrepareError::InvalidReference)?
                .kind
        }
        ExpressionNode::Constant(value) => {
            let kind = value.kind();
            if !supported(kind) {
                return Err(ExpressionPrepareError::UnsupportedType);
            }
            if value.validate().is_err() {
                return Err(ExpressionPrepareError::InvalidConstant);
            }
            kind
        }
        ExpressionNode::Unary {
            operator,
            operand,
        } => {
            let kind = child(*operand);
            match operator {
                UnaryOperator::Negate | UnaryOperator::Absolute if signed_numeric(kind) => kind,
                UnaryOperator::Not if kind == DynamicPropertyKind::Bool => kind,
                UnaryOperator::Length if kind == DynamicPropertyKind::Text => {
                    DynamicPropertyKind::U32
                }
                _ => return Err(ExpressionPrepareError::WrongType),
            }
        }
        ExpressionNode::Binary {
            operator,
            left,
            right,
        } => {
            let a = child(*left);
            let b = child(*right);
            if a != b {
                return Err(ExpressionPrepareError::WrongType);
            }
            match operator {
                BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Divide
                | BinaryOperator::Minimum
                | BinaryOperator::Maximum
                    if numeric(a) =>
                {
                    a
                }
                BinaryOperator::Equal => DynamicPropertyKind::Bool,
                BinaryOperator::Less | BinaryOperator::Greater if scalar_numeric(a) => {
                    DynamicPropertyKind::Bool
                }
                BinaryOperator::And | BinaryOperator::Or if a == DynamicPropertyKind::Bool => {
                    DynamicPropertyKind::Bool
                }
                _ => return Err(ExpressionPrepareError::WrongType),
            }
        }
        ExpressionNode::Clamp {
            value,
            minimum,
            maximum,
        } => {
            let a = child(*value);
            let b = child(*minimum);
            let c = child(*maximum);
            if a != b || a != c || !numeric(a) {
                return Err(ExpressionPrepareError::WrongType);
            }
            a
        }
        ExpressionNode::Ternary {
            condition,
            then_node,
            else_node,
        } => {
            let condition_kind = child(*condition);
            let true_kind = child(*then_node);
            let false_kind = child(*else_node);
            if condition_kind != DynamicPropertyKind::Bool || true_kind != false_kind {
                return Err(ExpressionPrepareError::WrongType);
            }
            true_kind
        }
        ExpressionNode::Fallback {
            value,
            replacement,
        } => {
            let a = child(*value);
            let b = child(*replacement);
            if a != b {
                return Err(ExpressionPrepareError::WrongType);
            }
            a
        }
    };
    Ok(kind)
}

pub(super) const MAX_ITEMS: usize = 65_536;
const MAX_DEPTH: usize = 256;

impl PreparedExpression {
    /// Validate every declared node and compile the output to bounded flat instructions.
    /// Inputs remain indexed bindings owned by the consumer. Clones share immutable
    /// instructions and input declarations, and accept scratch from the same preparation.
    pub fn prepare(declaration: &ExpressionDeclaration) -> Result<Self, ExpressionPrepareError> {
        let (kinds, operations) = validate_declaration(declaration)?;
        let operation_count = operations[declaration.output];
        let (instructions, slots, output) =
            super::compilation::compile(declaration, operation_count);

        Ok(Self {
            identity: Arc::new(()),
            inputs: declaration.inputs.clone().into(),
            instructions: instructions.into(),
            slots,
            output,
            output_kind: kinds[declaration.output].expect("output validated"),
        })
    }

    /// Prepared input names and exact types in slot order; no evaluation-time lookup.
    pub fn input_slots(&self) -> &[ExpressionInput] {
        &self.inputs
    }

    /// Exact core type produced by valid evaluations.
    pub fn output_kind(&self) -> DynamicPropertyKind {
        self.output_kind
    }

    /// Flat instruction count, including branch/copy/jump instructions and shared occurrences.
    pub fn operation_count(&self) -> usize {
        self.instructions.len()
    }

    /// Allocate independent fixed-capacity intermediate results for one consumer.
    /// Create scratch outside repeated evaluation; a cloned plan accepts this same scratch.
    pub fn scratch(&self) -> ExpressionScratch {
        ExpressionScratch {
            identity: Arc::clone(&self.identity),
            results: vec![ExpressionResult::Invalid(ExpressionInvalid::Calculation); self.slots],
        }
    }
}

// Shared with the declaration codec: validate semantics without compiling or
// cloning constants/input names into a reconstructible execution plan.
pub(super) fn validate_declaration(
    declaration: &ExpressionDeclaration,
) -> Result<(Vec<Option<DynamicPropertyKind>>, Vec<usize>), ExpressionPrepareError> {
    if declaration.inputs.len() > MAX_ITEMS || declaration.nodes.len() > MAX_ITEMS {
        return Err(ExpressionPrepareError::TooComplex);
    }

    let mut names = HashSet::new();
    for input in &declaration.inputs {
        if input.name.is_empty() {
            return Err(ExpressionPrepareError::EmptyInputName);
        }
        if !names.insert(input.name.as_str()) {
            return Err(ExpressionPrepareError::DuplicateInputName);
        }
        if !supported(input.kind) {
            return Err(ExpressionPrepareError::UnsupportedType);
        }
    }
    if declaration.output >= declaration.nodes.len() {
        return Err(ExpressionPrepareError::InvalidReference);
    }

    let (kinds, operations) = validate_graph(declaration)?;
    if operations[declaration.output] > MAX_ITEMS {
        return Err(ExpressionPrepareError::TooComplex);
    }
    Ok((kinds, operations))
}

fn validate_graph(
    declaration: &ExpressionDeclaration,
) -> Result<(Vec<Option<DynamicPropertyKind>>, Vec<usize>), ExpressionPrepareError> {
    let count = declaration.nodes.len();
    let mut kinds = vec![None; count];
    let mut visiting = vec![false; count];
    let mut heights = vec![0; count];
    let mut operations = vec![0; count];
    let mut stack = Vec::new();

    for root in 0..count {
        if kinds[root].is_some() {
            continue;
        }
        visiting[root] = true;
        stack.push((root, 0));

        while let Some(&(index, cursor)) = stack.last() {
            let node = &declaration.nodes[index];
            let children = node.children();
            if let Some(child) = children.get(cursor).copied().flatten() {
                stack.last_mut().expect("active node").1 += 1;
                if child >= count {
                    return Err(ExpressionPrepareError::InvalidReference);
                }
                if visiting[child] {
                    return Err(ExpressionPrepareError::Cycle);
                }
                if kinds[child].is_none() {
                    if stack.len() >= MAX_DEPTH {
                        return Err(ExpressionPrepareError::TooComplex);
                    }
                    visiting[child] = true;
                    stack.push((child, 0));
                }
                continue;
            }

            // Cache subtree height as well as type. Cached children can conceal
            // long paths when declaration order already matches dependencies.
            let height = 1 + children
                .iter()
                .flatten()
                .map(|&i| heights[i])
                .max()
                .unwrap_or(0);
            if height > MAX_DEPTH {
                return Err(ExpressionPrepareError::TooComplex);
            }
            heights[index] = height;
            kinds[index] = Some(check_type(declaration, index, &kinds)?);

            // The compiler emits each occurrence, including both lazy branches.
            // Saturation makes even an exponentially shared DAG cheap to refuse
            // before compilation without overflowing arithmetic or growing a plan.
            operations[index] = children
                .iter()
                .flatten()
                .fold(node.own_operation_count(), |total, &i| {
                    (total + operations[i]).min(MAX_ITEMS + 1)
                });
            visiting[index] = false;
            stack.pop();
        }
    }
    Ok((kinds, operations))
}
