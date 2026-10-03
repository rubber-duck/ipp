use crate::{DynamicPropertyKind, DynamicValue};

/// Named typed input resolved once by the consuming System, then supplied by slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionInput {
    /// Consumer-defined binding name; must be nonempty and unique in the declaration.
    pub name: String,
    /// Exact core value type; evaluation never coerces supplied values.
    pub kind: DynamicPropertyKind,
}

/// Pure unary expression operation, checked against its operand during preparation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    /// Arithmetic negation of signed integers, floats or float vector lanes.
    Negate,
    /// Absolute value of signed integers, floats or float vector lanes.
    Absolute,
    /// Boolean inversion.
    Not,
    /// Unicode scalar-value count of text, returned as a checked `U32`.
    Length,
}

/// Pure binary expression operation; operands must have the same exact core type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    /// Numeric addition, componentwise for float vectors.
    Add,
    /// Numeric subtraction, componentwise for float vectors.
    Subtract,
    /// Numeric multiplication, componentwise for float vectors.
    Multiply,
    /// Numeric division, componentwise for float vectors; integer division truncates.
    Divide,
    /// Numeric minimum, componentwise for float vectors.
    Minimum,
    /// Numeric maximum, componentwise for float vectors.
    Maximum,
    /// Exact value equality, including text, matrices and vectors.
    Equal,
    /// Scalar numeric ordering without integer-to-float conversion.
    Less,
    /// Scalar numeric ordering without integer-to-float conversion.
    Greater,
    /// Boolean conjunction; both operands are evaluated.
    And,
    /// Boolean disjunction; both operands are evaluated.
    Or,
}

/// One declaration node. Child references index [`ExpressionDeclaration::nodes`].
/// Forward references and shared nodes are accepted; cycles are rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpressionNode {
    /// Read the indexed declared input, or produce an invalid result if unavailable.
    Input(usize),
    /// A typed finite literal, validated during preparation.
    Constant(DynamicValue),
    /// Apply one pure unary operation.
    Unary {
        /// Operation to apply.
        operator: UnaryOperator,
        /// Operand node index.
        operand: usize,
    },
    /// Apply one pure binary operation, propagating the left invalid reason first.
    Binary {
        /// Operation to apply.
        operator: BinaryOperator,
        /// Left operand node index.
        left: usize,
        /// Right operand node index.
        right: usize,
    },
    /// Explicit numeric clamp; matching vector lanes clamp independently.
    Clamp {
        /// Value node index.
        value: usize,
        /// Inclusive lower bound node index.
        minimum: usize,
        /// Inclusive upper bound node index; reversed bounds produce invalid results.
        maximum: usize,
    },
    /// Lazy selection: prepare both branch types, evaluate only the selected branch.
    Ternary {
        /// Boolean condition node index; an invalid condition invalidates selection.
        condition: usize,
        /// Node selected when the condition is true.
        then_node: usize,
        /// Node selected when the condition is false; must match the true branch type.
        else_node: usize,
    },
    /// Explicit invalid-value replacement, evaluated only when needed.
    Fallback {
        /// Preferred value node index.
        value: usize,
        /// Replacement node index; must match the preferred value type.
        replacement: usize,
    },
}

impl ExpressionNode {
    pub(super) fn children(&self) -> [Option<usize>; 3] {
        match *self {
            Self::Input(_) | Self::Constant(_) => [None; 3],
            Self::Unary {
                operand,
                ..
            } => [Some(operand), None, None],
            Self::Binary {
                left,
                right,
                ..
            } => [Some(left), Some(right), None],
            Self::Clamp {
                value,
                minimum,
                maximum,
            } => [Some(value), Some(minimum), Some(maximum)],
            Self::Ternary {
                condition,
                then_node,
                else_node,
            } => [Some(condition), Some(then_node), Some(else_node)],
            Self::Fallback {
                value,
                replacement,
            } => [Some(value), Some(replacement), None],
        }
    }

    pub(super) fn own_operation_count(&self) -> usize {
        match self {
            Self::Ternary {
                ..
            } => 4, // Branch, two copies and a jump.
            Self::Fallback {
                ..
            } => 2, // Fallback and replacement copy.
            _ => 1,
        }
    }
}

/// Pure typed graph, independent of World storage, asset encoding and scheduling.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionDeclaration {
    /// Input slots in evaluation order; consumers resolve their names and lifetimes.
    pub inputs: Vec<ExpressionInput>,
    /// Graph nodes; all nodes are validated, including unreachable branches/nodes.
    pub nodes: Vec<ExpressionNode>,
    /// Index of the single output node.
    pub output: usize,
}

/// Malformed declaration or bounded-preparation refusal; distinct from sample invalidity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionPrepareError {
    /// An input binding name is empty.
    EmptyInputName,
    /// Two declared inputs have the same binding name.
    DuplicateInputName,
    /// An input or constant uses an unsupported core value type.
    UnsupportedType,
    /// A literal fails core value validation, such as containing a nonfinite lane.
    InvalidConstant,
    /// A node, output or input slot reference is outside its declaration.
    InvalidReference,
    /// Node dependencies contain a cycle.
    Cycle,
    /// An operator, condition or pair of operands has incompatible types.
    WrongType,
    /// Declaration size, dependency height or expanded instruction count exceeds limits.
    TooComplex,
}

/// Consumer binding contract failure, returned before any scratch results are changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionInputError {
    /// Supplied input count differs from the plan's declared slot count.
    WrongCount,
    /// Scratch belongs to a different preparation, even if its declaration is identical.
    StaleScratch,
    /// A supplied input has a different exact core type from its declaration.
    WrongType {
        /// Index of the mismatched input slot, including inputs on unused branches.
        slot: usize,
    },
}

/// Invalidity of the current sample; consumers decide whether to omit output or report it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionInvalid {
    /// A reached input slot has no value.
    MissingInput {
        /// Index of the missing input slot.
        slot: usize,
    },
    /// A reached typed input fails core value validation.
    InvalidInput {
        /// Index of the invalid input slot, such as a nonfinite scalar/vector/matrix.
        slot: usize,
    },
    /// Arithmetic overflow, zero division, nonfinite output or reversed clamp bounds.
    Calculation,
}

/// Explicit typed value or invalid sample; no implicit zero or prior result is substituted.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpressionResult {
    /// Successfully evaluated value, retaining its exact core type.
    Valid(DynamicValue),
    /// Invalid result, recoverable on the next evaluation or through authored fallback.
    Invalid(ExpressionInvalid),
}
