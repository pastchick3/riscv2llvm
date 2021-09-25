use crate::cfg::{BasicBlock, Cfg, RiscvFunction};
use crate::llvm_isa::{
    LlvmFunction, LlvmInstruction, LlvmIntCondition, LlvmType, LlvmValue, Program,
};
use crate::riscv_isa::{RiscvAddress, RiscvImmediate, RiscvInstruction, RiscvRegister};
use regex::Regex;
use std::collections::HashMap;
use std::mem;

pub struct LlvmTranslator {
    cfg: Cfg,
    statics: HashMap<String, (String, LlvmType)>,
    temp: usize,
}

impl LlvmTranslator {
    pub fn new(cfg: Cfg, statics: HashMap<String, (String, LlvmType)>) -> Self {
        LlvmTranslator {
            cfg,
            statics,
            temp: 0,
        }
    }

    pub fn run(&mut self) -> Program {
        let functions = mem::take(&mut self.cfg)
            .into_iter()
            .map(|f| self.translate_function(f))
            .collect();
        Program {
            statics: mem::take(&mut self.statics),
            functions,
        }
    }

    fn get_temps(&mut self, n: usize) -> Vec<LlvmValue> {
        let temps = (self.temp..self.temp + n).map(|t| t.into()).collect();
        self.temp += n;
        temps
    }

    fn translate_function(
        &mut self,
        RiscvFunction {
            name,
            basic_blocks,
            indirect_targets,
        }: RiscvFunction,
    ) -> LlvmFunction {
        self.temp = 0;
        let mut body: Vec<_> = basic_blocks
            .into_iter()
            .enumerate()
            .map(|(i, block)| {
                let mut insts = vec![LlvmInstruction::Label(format!("L{}", i))];
                insts.extend(self.translate_basic_block(block, &indirect_targets));
                insts
            })
            .flatten()
            .collect();
        body.insert(0, LlvmInstruction::UnconBr(String::from("L0")));
        body.insert(0, LlvmInstruction::Label(String::from("Entry")));
        LlvmFunction { name, body }
    }

    fn translate_basic_block(
        &mut self,
        BasicBlock {
            instructions,
            continue_target,
            jump_target,
        }: BasicBlock,
        indirect_targets: &HashMap<RiscvAddress, usize>,
    ) -> Vec<LlvmInstruction> {
        use LlvmInstruction as LI;
        use RiscvInstruction as RI;

        instructions
            .into_iter()
            .map(|inst| match inst {
                // RV32I
                RI::Lui { rd, imm, .. } => {
                    let temps = self.get_temps(1);
                    vec![
                        LI::Shl {
                            result: temps[0].clone(),
                            ty: LlvmType::I64,
                            op1: imm.into(),
                            op2: 12_i64.into(),
                        },
                        LI::Store {
                            ty: LlvmType::I64,
                            value: temps[0].clone(),
                            pointer: rd.into(),
                        },
                    ]
                }
                RI::Auipc { .. } => Vec::new(),
                RI::Jal { comment, .. } | RI::Jalr { comment, .. } => {
                    lazy_static! {
                        static ref FUNCTION: Regex = Regex::new(r"<(.+)>").unwrap();
                    }
                    let caps = FUNCTION.captures(comment.as_ref().unwrap()).unwrap();
                    let name = caps[1].to_string();
                    vec![LI::Call(name)]
                }
                RI::Beq { rs1, rs2, .. } => self.build_branch(
                    rs1,
                    rs2,
                    LlvmIntCondition::Eq,
                    jump_target.unwrap(),
                    continue_target.unwrap(),
                ),
                RI::Bne { rs1, rs2, .. } => self.build_branch(
                    rs1,
                    rs2,
                    LlvmIntCondition::Ne,
                    jump_target.unwrap(),
                    continue_target.unwrap(),
                ),
                RI::Blt { rs1, rs2, .. } => self.build_branch(
                    rs1,
                    rs2,
                    LlvmIntCondition::Slt,
                    jump_target.unwrap(),
                    continue_target.unwrap(),
                ),
                RI::Bge { rs1, rs2, .. } => self.build_branch(
                    rs1,
                    rs2,
                    LlvmIntCondition::Sge,
                    jump_target.unwrap(),
                    continue_target.unwrap(),
                ),
                RI::Bltu { rs1, rs2, .. } => self.build_branch(
                    rs1,
                    rs2,
                    LlvmIntCondition::Ult,
                    jump_target.unwrap(),
                    continue_target.unwrap(),
                ),
                RI::Bgeu { rs1, rs2, .. } => self.build_branch(
                    rs1,
                    rs2,
                    LlvmIntCondition::Uge,
                    jump_target.unwrap(),
                    continue_target.unwrap(),
                ),
                RI::Lb {
                    rd,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_load(rd, imm, rs1, comment, LlvmType::I8, true),
                RI::Lh {
                    rd,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_load(rd, imm, rs1, comment, LlvmType::I16, true),
                RI::Lw {
                    rd,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_load(rd, imm, rs1, comment, LlvmType::I32, true),
                RI::Lbu {
                    rd,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_load(rd, imm, rs1, comment, LlvmType::I8, false),
                RI::Lhu {
                    rd,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_load(rd, imm, rs1, comment, LlvmType::I16, false),
                RI::Sb {
                    rs2,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_store(rs2, imm, rs1, comment, LlvmType::I8),
                RI::Sh {
                    rs2,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_store(rs2, imm, rs1, comment, LlvmType::I16),
                RI::Sw {
                    rs2,
                    imm,
                    rs1,
                    comment,
                    ..
                } => self.build_store(rs2, imm, rs1, comment, LlvmType::I32),
                RI::Addi { rd, rs1, imm, .. } => {
                    let temps = self.get_temps(2);
                    vec![
                        LI::Load {
                            result: temps[0].clone(),
                            ty: LlvmType::I64,
                            pointer: rs1.into(),
                        },
                        LI::Add {
                            result: temps[1].clone(),
                            ty: LlvmType::I64,
                            op1: temps[0].clone(),
                            op2: imm.into(),
                        },
                        LI::Store {
                            ty: LlvmType::I64,
                            value: temps[1].clone(),
                            pointer: rd.into(),
                        },
                    ]
                }

                RI::Ret { .. } => vec![LI::Ret],
                _ => vec![],
            })
            .flatten()
            .collect()
    }

    fn build_branch(
        &mut self,
        rs1: RiscvRegister,
        rs2: RiscvRegister,
        cond: LlvmIntCondition,
        jump_target: usize,
        continue_target: usize,
    ) -> Vec<LlvmInstruction> {
        use LlvmInstruction::*;
        let temps = self.get_temps(3);
        vec![
            Load {
                result: temps[0].clone(),
                ty: LlvmType::I64,
                pointer: rs1.into(),
            },
            Load {
                result: temps[1].clone(),
                ty: LlvmType::I64,
                pointer: rs2.into(),
            },
            Icmp {
                result: temps[2].clone(),
                cond,
                ty: LlvmType::I64,
                op1: temps[0].clone(),
                op2: temps[1].clone(),
            },
            ConBr {
                cond: temps[2].clone(),
                iftrue: format!("L{}", jump_target),
                iffalse: format!("L{}", continue_target),
            },
        ]
    }

    fn build_load(
        &mut self,
        rd: RiscvRegister,
        imm: RiscvImmediate,
        rs1: RiscvRegister,
        comment: Option<String>,
        ty: LlvmType,
        signed: bool,
    ) -> Vec<LlvmInstruction> {
        use LlvmInstruction::*;

        lazy_static! {
            static ref STATIC: Regex = Regex::new(r"<(.+)>").unwrap();
        }
        if let Some(comment) = comment {
            let caps = STATIC.captures(&comment).unwrap();
            if let Some(name) = caps.get(1) {
                let temps = self.get_temps(2);
                if signed {
                    return vec![
                        Load {
                            result: temps[0].clone(),
                            ty: ty.clone(),
                            pointer: LlvmValue::GlobalVar(name.as_str().to_string()),
                        },
                        Sext {
                            result: temps[1].clone(),
                            ty: ty.clone(),
                            value: temps[0].clone(),
                            ty2: LlvmType::I64,
                        },
                        Store {
                            ty: LlvmType::I64,
                            value: temps[1].clone(),
                            pointer: rd.into(),
                        },
                    ];
                } else {
                    return vec![
                        Load {
                            result: temps[0].clone(),
                            ty: ty.clone(),
                            pointer: LlvmValue::GlobalVar(name.as_str().to_string()),
                        },
                        Zext {
                            result: temps[1].clone(),
                            ty: ty.clone(),
                            value: temps[0].clone(),
                            ty2: LlvmType::I64,
                        },
                        Store {
                            ty: LlvmType::I64,
                            value: temps[1].clone(),
                            pointer: rd.into(),
                        },
                    ];
                }
            }
        }

        if let LlvmType::I8 = ty {
            let temps = self.get_temps(5);
            vec![
                Load {
                    result: temps[0].clone(),
                    ty: LlvmType::I64,
                    pointer: rs1.into(),
                },
                Add {
                    result: temps[1].clone(),
                    ty: LlvmType::I64,
                    op1: temps[0].clone(),
                    op2: imm.into(),
                },
                Getelementptr {
                    result: temps[2].clone(),
                    index: temps[1].clone(),
                },
                Load {
                    result: temps[3].clone(),
                    ty: ty.clone(),
                    pointer: temps[2].clone(),
                },
                match signed {
                    true => Sext {
                        result: temps[4].clone(),
                        ty: ty.clone(),
                        value: temps[3].clone(),
                        ty2: LlvmType::I64,
                    },
                    false => Zext {
                        result: temps[4].clone(),
                        ty: ty.clone(),
                        value: temps[3].clone(),
                        ty2: LlvmType::I64,
                    },
                },
                Store {
                    ty: LlvmType::I64,
                    value: temps[4].clone(),
                    pointer: rd.into(),
                },
            ]
        } else {
            let temps = self.get_temps(6);
            vec![
                Load {
                    result: temps[0].clone(),
                    ty: LlvmType::I64,
                    pointer: rs1.into(),
                },
                Add {
                    result: temps[1].clone(),
                    ty: LlvmType::I64,
                    op1: temps[0].clone(),
                    op2: imm.into(),
                },
                Getelementptr {
                    result: temps[2].clone(),
                    index: temps[1].clone(),
                },
                Bitcast {
                    result: temps[3].clone(),
                    ty: LlvmType::I8,
                    value: temps[2].clone(),
                    ty2: ty.clone(),
                },
                Load {
                    result: temps[4].clone(),
                    ty: ty.clone(),
                    pointer: temps[3].clone(),
                },
                match signed {
                    true => Sext {
                        result: temps[5].clone(),
                        ty: ty.clone(),
                        value: temps[4].clone(),
                        ty2: LlvmType::I64,
                    },
                    false => Zext {
                        result: temps[5].clone(),
                        ty: ty.clone(),
                        value: temps[4].clone(),
                        ty2: LlvmType::I64,
                    },
                },
                Store {
                    ty: LlvmType::I64,
                    value: temps[5].clone(),
                    pointer: rd.into(),
                },
            ]
        }
    }

    fn build_store(
        &mut self,
        rs2: RiscvRegister,
        imm: RiscvImmediate,
        rs1: RiscvRegister,
        comment: Option<String>,
        ty: LlvmType,
    ) -> Vec<LlvmInstruction> {
        use LlvmInstruction::*;

        lazy_static! {
            static ref STATIC: Regex = Regex::new(r"<(.+)>").unwrap();
        }
        if let Some(comment) = comment {
            let caps = STATIC.captures(&comment).unwrap();
            if let Some(name) = caps.get(1) {
                let temps = self.get_temps(2);
                return vec![
                    Load {
                        result: temps[0].clone(),
                        ty: LlvmType::I64,
                        pointer: rs2.into(),
                    },
                    Trunc {
                        result: temps[1].clone(),
                        ty: LlvmType::I64,
                        value: temps[0].clone(),
                        ty2: ty.clone(),
                    },
                    Store {
                        ty: ty.clone(),
                        value: temps[1].clone(),
                        pointer: LlvmValue::GlobalVar(name.as_str().to_string()),
                    },
                ];
            }
        }

        if let LlvmType::I8 = ty {
            let temps = self.get_temps(5);
            vec![
                Load {
                    result: temps[0].clone(),
                    ty: LlvmType::I64,
                    pointer: rs1.into(),
                },
                Add {
                    result: temps[1].clone(),
                    ty: LlvmType::I64,
                    op1: temps[0].clone(),
                    op2: imm.into(),
                },
                Getelementptr {
                    result: temps[2].clone(),
                    index: temps[1].clone(),
                },
                Load {
                    result: temps[3].clone(),
                    ty: LlvmType::I64,
                    pointer: rs2.into(),
                },
                Trunc {
                    result: temps[4].clone(),
                    ty: LlvmType::I64,
                    value: temps[3].clone(),
                    ty2: ty.clone(),
                },
                Store {
                    ty: ty.clone(),
                    value: temps[4].clone(),
                    pointer: temps[2].clone(),
                },
            ]
        } else {
            let temps = self.get_temps(6);
            vec![
                Load {
                    result: temps[0].clone(),
                    ty: LlvmType::I64,
                    pointer: rs1.into(),
                },
                Add {
                    result: temps[1].clone(),
                    ty: LlvmType::I64,
                    op1: temps[0].clone(),
                    op2: imm.into(),
                },
                Getelementptr {
                    result: temps[2].clone(),
                    index: temps[1].clone(),
                },
                Bitcast {
                    result: temps[3].clone(),
                    ty: LlvmType::I8,
                    value: temps[2].clone(),
                    ty2: ty.clone(),
                },
                Load {
                    result: temps[4].clone(),
                    ty: LlvmType::I64,
                    pointer: rs2.into(),
                },
                Trunc {
                    result: temps[5].clone(),
                    ty: LlvmType::I64,
                    value: temps[4].clone(),
                    ty2: ty.clone(),
                },
                Store {
                    ty: ty.clone(),
                    value: temps[5].clone(),
                    pointer: temps[3].clone(),
                },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LlvmTranslator;
    use crate::cfg_builder::CfgBuilder;
    use crate::llvm_isa::{
        LlvmFunction, LlvmInstruction::*, LlvmIntCondition, LlvmType, LlvmValue, Program,
    };
    use crate::riscv_isa::RiscvRegister::*;
    use crate::riscv_parser;
    use std::collections::HashMap;

    #[test]
    fn jal() {
        let source = "
            Disassembly of section .text:

            00000000000104e0 <f>:
                104e0:	8082                	ret

            00000000000104ee <main>:
                104ee:	febff0ef          	jal	ra,104e0 <f>
                10504:	8082                	ret
        ";
        let indirect_targets = riscv_parser::parse_rodata(source);
        let mut statics = riscv_parser::parse_sdata(source);
        statics.extend(riscv_parser::parse_sbss(source));
        let rv_insts = riscv_parser::parse_text(source);
        let cfg = CfgBuilder::new(rv_insts, indirect_targets).run();
        let ll_program = LlvmTranslator::new(cfg, statics).run();
        let expected = Program {
            statics: HashMap::new(),
            functions: vec![
                LlvmFunction {
                    name: String::from("f"),
                    body: vec![
                        Label(String::from("Entry")),
                        UnconBr(String::from("L0")),
                        Label(String::from("L0")),
                        Ret,
                    ],
                },
                LlvmFunction {
                    name: String::from("main"),
                    body: vec![
                        Label(String::from("Entry")),
                        UnconBr(String::from("L0")),
                        Label(String::from("L0")),
                        Call(String::from("f")),
                        Ret,
                    ],
                },
            ],
        };
        assert_eq!(ll_program, expected);
    }

    #[test]
    fn jalr() {
        let source = "
            Disassembly of section .text:

            0000000000010506 <f>:
                10506:	8082                	ret

            000000000001051c <main>:
                1051c:	00000097          	auipc	ra,0x0
                10520:	fea080e7          	jalr	-22(ra) # 10506 <f>
                1052e:	8082                	ret
        ";
        let indirect_targets = riscv_parser::parse_rodata(source);
        let mut statics = riscv_parser::parse_sdata(source);
        statics.extend(riscv_parser::parse_sbss(source));
        let rv_insts = riscv_parser::parse_text(source);
        let cfg = CfgBuilder::new(rv_insts, indirect_targets).run();
        let ll_program = LlvmTranslator::new(cfg, statics).run();
        let expected = Program {
            statics: HashMap::new(),
            functions: vec![
                LlvmFunction {
                    name: String::from("f"),
                    body: vec![
                        Label(String::from("Entry")),
                        UnconBr(String::from("L0")),
                        Label(String::from("L0")),
                        Ret,
                    ],
                },
                LlvmFunction {
                    name: String::from("main"),
                    body: vec![
                        Label(String::from("Entry")),
                        UnconBr(String::from("L0")),
                        Label(String::from("L0")),
                        Call(String::from("f")),
                        Ret,
                    ],
                },
            ],
        };
        assert_eq!(ll_program, expected);
    }

    macro_rules! build_test {
        ( $( $func:ident ( $source:literal, [ $( $inst:expr, )* ] ), )* ) => {
            $(
                #[test]
                fn $func() {
                    let source = format!("
                        Disassembly of section .text:
                        0000000000010556 <main>:
                            10556:	1141                	{}
                            10598:	8082                	ret
                    ", $source);
                    let indirect_targets = riscv_parser::parse_rodata(&source);
                    let mut statics = riscv_parser::parse_sdata(&source);
                    statics.extend(riscv_parser::parse_sbss(&source));
                    let rv_insts = riscv_parser::parse_text(&source);
                    let cfg = CfgBuilder::new(rv_insts, indirect_targets).run();
                    let ll_program = LlvmTranslator::new(cfg, statics).run();
                    let expected = Program{
                        statics: HashMap::new(),
                        functions: vec![LlvmFunction {
                            name: String::from("main"),
                            body: vec![
                                Label(String::from("Entry")),
                                UnconBr(String::from("L0")),
                                Label(String::from("L0")),
                                $(
                                    $inst,
                                )*
                                Ret,
                            ],
                        }]
                    };
                    assert_eq!(ll_program, expected);
                }
            )*
        };
    }

    build_test! {
        lui("lui	a0,0x12", [
            Shl {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                op1: 0x12_i64.into(),
                op2: 12_i64.into(),
            },
            Store {
                ty: LlvmType::I64,
                value: 0_usize.into(),
                pointer: A0.into(),
            },
        ]),
        auipc("auipc	a0,0x0", []),
        beq("beq	a4,a5,10556", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Load {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Icmp {
                result: 2_usize.into(),
                cond: LlvmIntCondition::Eq,
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 1_usize.into(),
            },
            ConBr {
                cond: 2_usize.into(),
                iftrue: String::from("L0"),
                iffalse: String::from("L1"),
            },
            Label(String::from("L1")),
        ]),
        bne("bne	a4,a5,10556", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Load {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Icmp {
                result: 2_usize.into(),
                cond: LlvmIntCondition::Ne,
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 1_usize.into(),
            },
            ConBr {
                cond: 2_usize.into(),
                iftrue: String::from("L0"),
                iffalse: String::from("L1"),
            },
            Label(String::from("L1")),
        ]),
        blt("blt	a4,a5,10556", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Load {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Icmp {
                result: 2_usize.into(),
                cond: LlvmIntCondition::Slt,
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 1_usize.into(),
            },
            ConBr {
                cond: 2_usize.into(),
                iftrue: String::from("L0"),
                iffalse: String::from("L1"),
            },
            Label(String::from("L1")),
        ]),
        bge("bge	a4,a5,10556", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Load {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Icmp {
                result: 2_usize.into(),
                cond: LlvmIntCondition::Sge,
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 1_usize.into(),
            },
            ConBr {
                cond: 2_usize.into(),
                iftrue: String::from("L0"),
                iffalse: String::from("L1"),
            },
            Label(String::from("L1")),
        ]),
        bltu("bltu	a4,a5,10556", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Load {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Icmp {
                result: 2_usize.into(),
                cond: LlvmIntCondition::Ult,
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 1_usize.into(),
            },
            ConBr {
                cond: 2_usize.into(),
                iftrue: String::from("L0"),
                iffalse: String::from("L1"),
            },
            Label(String::from("L1")),
        ]),
        bgeu("bgeu	a4,a5,10556", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Load {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Icmp {
                result: 2_usize.into(),
                cond: LlvmIntCondition::Uge,
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 1_usize.into(),
            },
            ConBr {
                cond: 2_usize.into(),
                iftrue: String::from("L0"),
                iffalse: String::from("L1"),
            },
            Label(String::from("L1")),
        ]),
        lb_global("lb	a4,48(a5) # 12030 <g1>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I8,
                pointer: LlvmValue::GlobalVar(String::from("g1")),
            },
            Sext {
                result: 1_usize.into(),
                ty: LlvmType::I8,
                value: 0_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 1_usize.into(),
                pointer: A4.into(),
            },
        ]),
        lb("lb	a5,-20(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: (-20_i64).into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Load {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                pointer: 2_usize.into(),
            },
            Sext {
                result: 4_usize.into(),
                ty: LlvmType::I8,
                value: 3_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 4_usize.into(),
                pointer: A5.into(),
            },
        ]),
        lh_global("lh	a4,48(a5) # 12030 <g1>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I16,
                pointer: LlvmValue::GlobalVar(String::from("g1")),
            },
            Sext {
                result: 1_usize.into(),
                ty: LlvmType::I16,
                value: 0_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 1_usize.into(),
                pointer: A4.into(),
            },
        ]),
        lh("lh	a5,-20(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: (-20_i64).into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Bitcast {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                value: 2_usize.into(),
                ty2: LlvmType::I16,
            },
            Load {
                result: 4_usize.into(),
                ty: LlvmType::I16,
                pointer: 3_usize.into(),
            },
            Sext {
                result: 5_usize.into(),
                ty: LlvmType::I16,
                value: 4_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 5_usize.into(),
                pointer: A5.into(),
            },
        ]),
        lw_global("lw	a4,48(a5) # 12030 <g1>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I32,
                pointer: LlvmValue::GlobalVar(String::from("g1")),
            },
            Sext {
                result: 1_usize.into(),
                ty: LlvmType::I32,
                value: 0_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 1_usize.into(),
                pointer: A4.into(),
            },
        ]),
        lw("lw	a5,-20(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: (-20_i64).into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Bitcast {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                value: 2_usize.into(),
                ty2: LlvmType::I32,
            },
            Load {
                result: 4_usize.into(),
                ty: LlvmType::I32,
                pointer: 3_usize.into(),
            },
            Sext {
                result: 5_usize.into(),
                ty: LlvmType::I32,
                value: 4_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 5_usize.into(),
                pointer: A5.into(),
            },
        ]),
        lbu_global("lbu	a4,48(a5) # 12030 <g1>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I8,
                pointer: LlvmValue::GlobalVar(String::from("g1")),
            },
            Zext {
                result: 1_usize.into(),
                ty: LlvmType::I8,
                value: 0_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 1_usize.into(),
                pointer: A4.into(),
            },
        ]),
        lbu("lbu	a5,-20(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: (-20_i64).into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Load {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                pointer: 2_usize.into(),
            },
            Zext {
                result: 4_usize.into(),
                ty: LlvmType::I8,
                value: 3_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 4_usize.into(),
                pointer: A5.into(),
            },
        ]),
        lhu_global("lhu	a4,48(a5) # 12030 <g1>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I16,
                pointer: LlvmValue::GlobalVar(String::from("g1")),
            },
            Zext {
                result: 1_usize.into(),
                ty: LlvmType::I16,
                value: 0_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 1_usize.into(),
                pointer: A4.into(),
            },
        ]),
        lhu("lhu	a5,-20(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: (-20_i64).into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Bitcast {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                value: 2_usize.into(),
                ty2: LlvmType::I16,
            },
            Load {
                result: 4_usize.into(),
                ty: LlvmType::I16,
                pointer: 3_usize.into(),
            },
            Zext {
                result: 5_usize.into(),
                ty: LlvmType::I16,
                value: 4_usize.into(),
                ty2: LlvmType::I64,
            },
            Store {
                ty: LlvmType::I64,
                value: 5_usize.into(),
                pointer: A5.into(),
            },
        ]),
        sb_global("sb	a4,52(a5) # 12034 <g2>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Trunc {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                value: 0_usize.into(),
                ty2: LlvmType::I8,
            },
            Store {
                ty: LlvmType::I8,
                value: 1_usize.into(),
                pointer: LlvmValue::GlobalVar(String::from("g2")),
            },
        ]),
        sb("sb	a5,48(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 48_i64.into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Load {
                result: 3_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Trunc {
                result: 4_usize.into(),
                ty: LlvmType::I64,
                value: 3_usize.into(),
                ty2: LlvmType::I8,
            },
            Store {
                ty: LlvmType::I8,
                value: 4_usize.into(),
                pointer: 2_usize.into(),
            },
        ]),
        sh_global("sh	a4,52(a5) # 12034 <g2>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Trunc {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                value: 0_usize.into(),
                ty2: LlvmType::I16,
            },
            Store {
                ty: LlvmType::I16,
                value: 1_usize.into(),
                pointer: LlvmValue::GlobalVar(String::from("g2")),
            },
        ]),
        sh("sh	a5,48(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 48_i64.into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Bitcast {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                value: 2_usize.into(),
                ty2: LlvmType::I16,
            },
            Load {
                result: 4_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Trunc {
                result: 5_usize.into(),
                ty: LlvmType::I64,
                value: 4_usize.into(),
                ty2: LlvmType::I16,
            },
            Store {
                ty: LlvmType::I16,
                value: 5_usize.into(),
                pointer:3_usize.into(),
            },
        ]),
        sw_global("sw	a4,52(a5) # 12034 <g2>", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: A4.into(),
            },
            Trunc {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                value: 0_usize.into(),
                ty2: LlvmType::I32,
            },
            Store {
                ty: LlvmType::I32,
                value: 1_usize.into(),
                pointer: LlvmValue::GlobalVar(String::from("g2")),
            },
        ]),
        sw("sw	a5,48(s0)", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: S0.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 48_i64.into(),
            },
            Getelementptr {
                result: 2_usize.into(),
                index: 1_usize.into(),
            },
            Bitcast {
                result: 3_usize.into(),
                ty: LlvmType::I8,
                value: 2_usize.into(),
                ty2: LlvmType::I32,
            },
            Load {
                result: 4_usize.into(),
                ty: LlvmType::I64,
                pointer: A5.into(),
            },
            Trunc {
                result: 5_usize.into(),
                ty: LlvmType::I64,
                value: 4_usize.into(),
                ty2: LlvmType::I32,
            },
            Store {
                ty: LlvmType::I32,
                value: 5_usize.into(),
                pointer:3_usize.into(),
            },
        ]),
        addi("addi	a2,sp,8", [
            Load {
                result: 0_usize.into(),
                ty: LlvmType::I64,
                pointer: Sp.into(),
            },
            Add {
                result: 1_usize.into(),
                ty: LlvmType::I64,
                op1: 0_usize.into(),
                op2: 8_i64.into(),
            },
            Store {
                ty: LlvmType::I64,
                value: 1_usize.into(),
                pointer: A2.into(),
            },
        ]),
    }
}
