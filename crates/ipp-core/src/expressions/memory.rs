//! Payload allocation accounting for a retained declaration and its prepared plan.

use super::*;
use std::{collections::BTreeSet, mem::size_of};

// Computed once by the immutable asset, never during evaluation. Names in the
// prepared input slice are independent String allocations. Text constants are
// Arc-shared through compilation, including expanded occurrences of one node.
pub(crate) fn resident_bytes(
    declaration: &ExpressionDeclaration,
    prepared: &PreparedExpression,
) -> usize {
    let mut bytes = declaration.inputs.capacity() * size_of::<ExpressionInput>()
        + declaration.nodes.capacity() * size_of::<ExpressionNode>()
        + std::mem::size_of_val(prepared.inputs.as_ref())
        + std::mem::size_of_val(prepared.instructions.as_ref());
    bytes += declaration
        .inputs
        .iter()
        .chain(prepared.inputs.iter())
        .map(|input| input.name.capacity())
        .sum::<usize>();

    let mut seen = BTreeSet::new();
    let literals = declaration
        .nodes
        .iter()
        .filter_map(|node| match node {
            ExpressionNode::Constant(value) => Some(value),
            _ => None,
        })
        .chain(
            prepared
                .instructions
                .iter()
                .filter_map(|instruction| match instruction {
                    Instruction::Constant {
                        value,
                        ..
                    } => Some(value),
                    _ => None,
                }),
        );
    for value in literals {
        if let DynamicValue::Text(text) = value
            && seen.insert(Arc::as_ptr(text) as *const ())
        {
            bytes += text.len();
        }
    }

    bytes
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
