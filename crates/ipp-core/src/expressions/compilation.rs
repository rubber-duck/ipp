use super::*;

pub(super) fn compile(
    declaration: &ExpressionDeclaration,
    operation_count: usize,
) -> (Vec<Instruction>, usize, usize) {
    let mut compiler = Compiler {
        declaration,
        instructions: Vec::with_capacity(operation_count),
        slots: 0,
    };
    let output = compiler.emit(declaration.output);
    debug_assert_eq!(compiler.instructions.len(), operation_count);
    debug_assert!(compiler.slots <= operation_count);

    (compiler.instructions, compiler.slots, output)
}

struct Compiler<'a> {
    declaration: &'a ExpressionDeclaration,
    instructions: Vec<Instruction>,
    slots: usize,
}

// Explicit continuations avoid using the native/WASM call stack for graph depth.
enum CompileTask {
    Emit {
        index: usize,
        dst: usize,
    },
    Write(Instruction),
    Branch {
        condition: usize,
        dst: usize,
        then_node: usize,
        else_node: usize,
    },
    Else {
        branch: usize,
        dst: usize,
        else_node: usize,
    },
    EndBranch {
        branch: usize,
        jump: usize,
        false_pc: usize,
    },
    Fallback {
        value: usize,
        dst: usize,
        replacement: usize,
    },
    EndFallback {
        branch: usize,
    },
}

impl Compiler<'_> {
    fn slot(&mut self) -> usize {
        let slot = self.slots;
        self.slots += 1;
        slot
    }

    fn emit(&mut self, index: usize) -> usize {
        let output = self.slot();
        let mut tasks = vec![CompileTask::Emit {
            index,
            dst: output,
        }];

        while !tasks.is_empty() {
            self.step(&mut tasks);
        }
        output
    }

    fn step(&mut self, tasks: &mut Vec<CompileTask>) {
        let task = tasks.pop().expect("active compiler task");
        match task {
            CompileTask::Emit {
                index,
                dst,
            } => match self.declaration.nodes[index].clone() {
                ExpressionNode::Input(slot) => self.instructions.push(Instruction::Input {
                    dst,
                    slot,
                }),
                ExpressionNode::Constant(value) => self.instructions.push(Instruction::Constant {
                    dst,
                    value,
                }),
                ExpressionNode::Unary {
                    operator,
                    operand,
                } => {
                    let src = self.slot();
                    tasks.push(CompileTask::Write(Instruction::Unary {
                        dst,
                        src,
                        operator,
                    }));
                    tasks.push(CompileTask::Emit {
                        index: operand,
                        dst: src,
                    });
                }
                ExpressionNode::Binary {
                    operator,
                    left,
                    right,
                } => {
                    let left_slot = self.slot();
                    let right_slot = self.slot();
                    tasks.push(CompileTask::Write(Instruction::Binary {
                        dst,
                        left: left_slot,
                        right: right_slot,
                        operator,
                    }));
                    tasks.push(CompileTask::Emit {
                        index: right,
                        dst: right_slot,
                    });
                    tasks.push(CompileTask::Emit {
                        index: left,
                        dst: left_slot,
                    });
                }
                ExpressionNode::Clamp {
                    value,
                    minimum,
                    maximum,
                } => {
                    let value_slot = self.slot();
                    let minimum_slot = self.slot();
                    let maximum_slot = self.slot();
                    tasks.push(CompileTask::Write(Instruction::Clamp {
                        dst,
                        value: value_slot,
                        minimum: minimum_slot,
                        maximum: maximum_slot,
                    }));
                    tasks.push(CompileTask::Emit {
                        index: maximum,
                        dst: maximum_slot,
                    });
                    tasks.push(CompileTask::Emit {
                        index: minimum,
                        dst: minimum_slot,
                    });
                    tasks.push(CompileTask::Emit {
                        index: value,
                        dst: value_slot,
                    });
                }
                ExpressionNode::Ternary {
                    condition,
                    then_node,
                    else_node,
                } => {
                    let condition_slot = self.slot();
                    tasks.push(CompileTask::Branch {
                        condition: condition_slot,
                        dst,
                        then_node,
                        else_node,
                    });
                    tasks.push(CompileTask::Emit {
                        index: condition,
                        dst: condition_slot,
                    });
                }
                ExpressionNode::Fallback {
                    value,
                    replacement,
                } => {
                    let value_slot = self.slot();
                    tasks.push(CompileTask::Fallback {
                        value: value_slot,
                        dst,
                        replacement,
                    });
                    tasks.push(CompileTask::Emit {
                        index: value,
                        dst: value_slot,
                    });
                }
            },
            CompileTask::Write(instruction) => self.instructions.push(instruction),
            CompileTask::Branch {
                condition,
                dst,
                then_node,
                else_node,
            } => {
                let branch = self.instructions.len();
                self.instructions.push(Instruction::Branch {
                    condition,
                    dst,
                    false_pc: 0,
                    end_pc: 0,
                });
                let src = self.slot();
                tasks.push(CompileTask::Else {
                    branch,
                    dst,
                    else_node,
                });
                tasks.push(CompileTask::Write(Instruction::Copy {
                    dst,
                    src,
                }));
                tasks.push(CompileTask::Emit {
                    index: then_node,
                    dst: src,
                });
            }
            CompileTask::Else {
                branch,
                dst,
                else_node,
            } => {
                let jump = self.instructions.len();
                self.instructions.push(Instruction::Jump {
                    pc: 0,
                });
                let false_pc = self.instructions.len();
                let src = self.slot();
                tasks.push(CompileTask::EndBranch {
                    branch,
                    jump,
                    false_pc,
                });
                tasks.push(CompileTask::Write(Instruction::Copy {
                    dst,
                    src,
                }));
                tasks.push(CompileTask::Emit {
                    index: else_node,
                    dst: src,
                });
            }
            CompileTask::EndBranch {
                branch,
                jump,
                false_pc,
            } => {
                let end_pc = self.instructions.len();
                let Instruction::Branch {
                    false_pc: destination,
                    end_pc: end,
                    ..
                } = &mut self.instructions[branch]
                else {
                    unreachable!("branch continuation patches its own instruction")
                };
                *destination = false_pc;
                *end = end_pc;
                self.instructions[jump] = Instruction::Jump {
                    pc: end_pc,
                };
            }
            CompileTask::Fallback {
                value,
                dst,
                replacement,
            } => {
                let branch = self.instructions.len();
                self.instructions.push(Instruction::Fallback {
                    value,
                    dst,
                    end_pc: 0,
                });
                let src = self.slot();
                tasks.push(CompileTask::EndFallback {
                    branch,
                });
                tasks.push(CompileTask::Write(Instruction::Copy {
                    dst,
                    src,
                }));
                tasks.push(CompileTask::Emit {
                    index: replacement,
                    dst: src,
                });
            }
            CompileTask::EndFallback {
                branch,
            } => {
                let end_pc = self.instructions.len();
                let Instruction::Fallback {
                    end_pc: end,
                    ..
                } = &mut self.instructions[branch]
                else {
                    unreachable!("fallback continuation patches its own instruction")
                };
                *end = end_pc;
            }
        }
    }
}

pub(super) async fn compile_async(
    declaration: &ExpressionDeclaration,
    operation_count: usize,
) -> (Vec<Instruction>, usize, usize) {
    let mut compiler = Compiler {
        declaration,
        instructions: Vec::with_capacity(operation_count),
        slots: 0,
    };
    let output = compiler.slot();
    let mut tasks = vec![CompileTask::Emit {
        index: declaration.output,
        dst: output,
    }];
    let mut budget = crate::services::asset_management::decode::DecodeBudget::default();
    while !tasks.is_empty() {
        compiler.step(&mut tasks);
        budget.advance(0).await;
    }
    debug_assert_eq!(compiler.instructions.len(), operation_count);
    (compiler.instructions, compiler.slots, output)
}
