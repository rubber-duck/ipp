//! Allocation-free evaluation of a prepared plan into consumer-owned scratch.

use super::operations::{apply_binary, apply_clamp, apply_unary};
use super::*;

impl PreparedExpression {
    /// Evaluates borrowed typed inputs. `None` is a missing input, not a binding error.
    /// Only selected paths are written. Every read follows a write in this call,
    /// so skipped scratch slots cannot contribute a previous sample. Wrong input
    /// shape/types and scratch from another plan fail before evaluation.
    /// Nonfinite inputs invalidate their reached node; lazy branches can skip them.
    /// Evaluation allocates no memory; text values share their existing `Arc<str>`.
    pub fn evaluate<'a>(
        &self,
        scratch: &'a mut ExpressionScratch,
        inputs: &[Option<&DynamicValue>],
    ) -> Result<&'a ExpressionResult, ExpressionInputError> {
        if !Arc::ptr_eq(&scratch.identity, &self.identity) {
            return Err(ExpressionInputError::StaleScratch);
        }
        if inputs.len() != self.inputs.len() || scratch.results.len() != self.slots {
            return Err(ExpressionInputError::WrongCount);
        }
        for (slot, (input, declaration)) in inputs.iter().zip(self.inputs.iter()).enumerate() {
            if input.is_some_and(|value| value.kind() != declaration.kind) {
                return Err(ExpressionInputError::WrongType {
                    slot,
                });
            }
        }

        let results = &mut scratch.results;
        let mut pc = 0;
        while pc < self.instructions.len() {
            match &self.instructions[pc] {
                Instruction::Input {
                    dst,
                    slot,
                } => {
                    results[*dst] = inputs[*slot].map_or(
                        ExpressionResult::Invalid(ExpressionInvalid::MissingInput {
                            slot: *slot,
                        }),
                        |value| {
                            if value.validate().is_ok() {
                                ExpressionResult::Valid((*value).clone())
                            } else {
                                ExpressionResult::Invalid(ExpressionInvalid::InvalidInput {
                                    slot: *slot,
                                })
                            }
                        },
                    );
                }
                Instruction::Constant {
                    dst,
                    value,
                } => results[*dst] = ExpressionResult::Valid(value.clone()),
                Instruction::Unary {
                    dst,
                    src,
                    operator,
                } => results[*dst] = apply_unary(*operator, &results[*src]),
                Instruction::Binary {
                    dst,
                    left,
                    right,
                    operator,
                } => results[*dst] = apply_binary(*operator, &results[*left], &results[*right]),
                Instruction::Clamp {
                    dst,
                    value,
                    minimum,
                    maximum,
                } => {
                    results[*dst] =
                        apply_clamp(&results[*value], &results[*minimum], &results[*maximum])
                }
                Instruction::Copy {
                    dst,
                    src,
                } => results[*dst] = results[*src].clone(),
                Instruction::Branch {
                    condition,
                    dst,
                    false_pc,
                    end_pc,
                } => match &results[*condition] {
                    ExpressionResult::Valid(DynamicValue::Bool(false)) => {
                        pc = *false_pc;
                        continue;
                    }
                    ExpressionResult::Valid(DynamicValue::Bool(true)) => {}
                    ExpressionResult::Invalid(reason) => {
                        results[*dst] = ExpressionResult::Invalid(reason.clone());
                        pc = *end_pc;
                        continue;
                    }
                    _ => unreachable!("prepared condition is boolean"),
                },
                Instruction::Fallback {
                    value,
                    dst,
                    end_pc,
                } => {
                    if let ExpressionResult::Valid(value) = &results[*value] {
                        results[*dst] = ExpressionResult::Valid(value.clone());
                        pc = *end_pc;
                        continue;
                    }
                }
                Instruction::Jump {
                    pc: destination,
                } => {
                    pc = *destination;
                    continue;
                }
            }
            pc += 1;
        }
        Ok(&results[self.output])
    }
}

#[cfg(test)]
#[path = "evaluation_tests.rs"]
mod tests;
