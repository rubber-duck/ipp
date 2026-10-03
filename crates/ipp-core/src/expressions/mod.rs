//! Pure typed expressions. Consumers resolve input names and own binding lifetimes.
//! `Input` slots hold existing [`DynamicValue`] kinds; source and parameter
//! binding belongs to the consumer. Operations accept matching types without
//! implicit coercion. Add, subtract, multiply, divide, minimum, maximum and
//! clamp apply componentwise to float vectors. Integer arithmetic is checked;
//! division by zero, overflow, nonfinite float output and reversed clamp
//! bounds yield [`ExpressionInvalid::Calculation`]. Equality compares exact
//! values, including text and vectors; ordering compares scalar numbers only.
//! Length counts Unicode scalar values with `str::chars()`. Ternary evaluates
//! its condition and selected branch only. Fallback evaluates its replacement
//! only when its first value is invalid. Other binary operations evaluate both
//! operands, propagating the left invalid reason first.
//! Preparation uses iterative graph traversal and compilation. Declarations
//! permit at most 65,536 inputs/nodes, dependency paths of 256 nodes, and 65,536
//! expanded instructions. Shared nodes are expanded per occurrence to preserve
//! lazy control flow; preparation bounds that expansion before allocating a plan.
//! Supported values are core scalars, vectors, matrices and text; asset values
//! are excluded. Matrices and text support identity, equality and selection,
//! with Unicode length additionally available for text. Integers stay in their
//! exact core types; integer division truncates toward zero. Boolean And/Or
//! evaluate both operands. Invalid input values propagate only when reached.

use crate::{DynamicPropertyKind, DynamicValue};
use std::sync::Arc;

mod codec;
mod compilation;
mod declaration;
mod evaluation;
mod memory;
mod operations;
mod preparation;

pub(crate) use memory::resident_bytes;

pub use codec::{
    EXPRESSION_FORMAT_MAGIC, EXPRESSION_FORMAT_VERSION, EXPRESSION_MAX_BYTES, EXPRESSION_MAX_ITEMS,
    EXPRESSION_MAX_STRING_BYTES, ExpressionCodecError,
};
pub use declaration::{
    BinaryOperator, ExpressionDeclaration, ExpressionInput, ExpressionInputError,
    ExpressionInvalid, ExpressionNode, ExpressionPrepareError, ExpressionResult, UnaryOperator,
};

#[derive(Clone, Debug)]
enum Instruction {
    Input {
        dst: usize,
        slot: usize,
    },
    Constant {
        dst: usize,
        value: DynamicValue,
    },
    Unary {
        dst: usize,
        src: usize,
        operator: UnaryOperator,
    },
    Binary {
        dst: usize,
        left: usize,
        right: usize,
        operator: BinaryOperator,
    },
    Clamp {
        dst: usize,
        value: usize,
        minimum: usize,
        maximum: usize,
    },
    Copy {
        dst: usize,
        src: usize,
    },
    Branch {
        condition: usize,
        dst: usize,
        false_pc: usize,
        end_pc: usize,
    },
    Fallback {
        value: usize,
        dst: usize,
        end_pc: usize,
    },
    Jump {
        pc: usize,
    },
}

/// Immutable, shareable instruction plan. Each consumer owns a separate scratch instance.
#[derive(Clone, Debug)]
pub struct PreparedExpression {
    identity: Arc<()>,
    inputs: Arc<[ExpressionInput]>,
    instructions: Arc<[Instruction]>,
    slots: usize,
    output: usize,
    output_kind: DynamicPropertyKind,
}

/// Reused indexed results. Capacity is fixed by the prepared plan.
#[derive(Clone, Debug)]
pub struct ExpressionScratch {
    identity: Arc<()>,
    results: Vec<ExpressionResult>,
}

#[cfg(test)]
mod evaluation_tests;

#[cfg(test)]
mod preparation_tests;

#[cfg(test)]
mod operations_tests;

#[cfg(test)]
mod codec_tests;
