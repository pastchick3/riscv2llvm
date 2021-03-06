# riscv2llvm

A binary translator that translates RISC-V to LLVM IR.

## Requirements

- RISC-V Linux ABI and system calls
- Little-endian statically-linked executable files
- Arch: RV64GC, ABI: LP64D

## Testing Commands

``` Bash

riscv64-unknown-linux-gnu-gcc -static test.c -o test
riscv64-unknown-linux-gnu-strip -g test -o test.stripped
llvm-objdump --disassemble-all --disassemble-zeroes test.stripped > test.dump

llvm-objdump
--all-headers
--section-headers
--syms
```

## RISC-V Notes

- The base integer instruction sets use a two’s-complement representation for signed integer values.
- RV32I: 40 insts including `ecall`, `ebreak`, and `fence`
- The program counter `pc` holds the address of the current instruction
- Except for the 5-bit immediates used in CSR instructions (Chapter 9), immediates are always sign-extended
- The shift amount held in the lower 5 bits
- The target of `JALR` address is obtained by adding the sign-extended 12-bit I-immediate to the register rs1, then setting the least-significant bit of the result to zero.
- In RV64I, only the low 6 bits of rs2 are considered for the shift amount
- Unlike RISC-V, taking the remainder of a division by zero in LLVM is undefined behavior.
- All CSR instructions atomically read-modify-write a single CSR

CSR
- If rd=x0, then CSRRW shall not read the CSR and shall not cause any of the side effects that might occur on a CSR read.
- For both CSRRS and CSRRC, if rs1=x0, then the instruction will not write to the CSR at all, and so shall not cause any of the side effects that might otherwise occur on a CSR write.
- Both CSRRS and CSRRC always read the addressed CSR and cause any read side effects regardless of rs1 and rd fields.
-  For CSRRSI and CSRRCI, if the uimm[4:0] field is zero, then these instructions will not write to the CSR, and shall not cause any of the side effects that might otherwise occur on a CSR write. For CSRRWI, if rd=x0, then the instruction shall not read the CSR and shall not cause any of the side effects that might occur on a CSR read. Both CSRRSI and CSRRCI will always read the CSR and cause any read side effects regardless of rd and rs1 fields.

Counter
- RDCYCLE: The execution environment should provide a means to determine the current rate (cycles/second) at which the cycle counter is incrementing.
- RDTIME: The execution environment should provide a means of determining the period of the real-time counter (seconds/tick). The environment should provide a means to determine the accuracy of the clock.

## Todo

- read entry from elf?
- Trap `argv` in address 1?

- Force x0 always to be 0
- Map RV inst to LLVM comment for easy debugging
```
    105ac:	4e078793          	addi	a5,a5,1248 # 104e0 <s>
    105b0:	fef43023          	sd	a5,-32(s0)
    105b4:	fec42783          	lw	a5,-20(s0)
    105b8:	fe043703          	ld	a4,-32(s0)
    105bc:	853e                	mv	a0,a5
    105be:	9702                	jalr	a4
```

- argv is trapped for access to addr 0.
```
   109a8:	f2040713          	addi	a4,s0,-224
   109ac:	9ae1b823          	sd	a4,-1616(gp) # 70a78 <Ptr_Glob>
   109b0:	9b01b783          	ld	a5,-1616(gp) # 70a78 <Ptr_Glob>
   109b4:	9b81b703          	ld	a4,-1608(gp) # 70a80 <Next_Ptr_Glob>
   109b8:	e398                	sd	a4,0(a5)
```

- `1c6b6:	8e1bc9bf 00004034 	0x40348e1bc9bf`

## Reference

``` llvm
declare dso_local void @exit(i32)
declare dso_local i32 @printf(i8*, ...)
@.str.d = private unnamed_addr constant [14 x i8] c"#value: %ld#\0A\00", align 1
@.str.f = private unnamed_addr constant [13 x i8] c"#value: %f#\0A\00", align 1
@.str.s = private unnamed_addr constant [13 x i8] c"#value: %s#\0A\00", align 1

%ptr_a0 = getelementptr %struct.reg, %struct.reg* %reg, i32 0, i32 10
%val_a0 = load i64, i64* %ptr_a0
call i32 (i8*, ...) @printf(i8* getelementptr inbounds ([14 x i8], [14 x i8]* @.str.d, i64 0, i64 0), i64 %val_a0)
%val_fa0 = load double, double* %fa0
call i32 (i8*, ...) @printf(i8* getelementptr inbounds ([13 x i8], [13 x i8]* @.str.f, i64 0, i64 0), double %val_fa0)
%val_s = load i64, i64* %a0
%ptr_s = call i8* @get_data_ptr(i64 %val_s)
call i32 (i8*, ...) @printf(i8* getelementptr inbounds ([13 x i8], [13 x i8]* @.str.s, i64 0, i64 0), i8* %ptr_s)

call void @exit(i32 0)

opt --mem2reg -S dry2.ll -o dry2.o.ll
```

## syscall

The Linux system call number is different for each architecture.
    - `SYS_write` in spike pk is 64, but in x64 is 1.
    - <https://github.com/westerndigitalcorporation/RISC-V-Linux/blob/master/riscv-pk/pk/syscall.h>
    - <https://chromium.googlesource.com/chromiumos/docs/+/refs/heads/main/constants/syscalls.md>
To call a generic `syscall` in LLVM IR, we must recover the type for each argument, possibly with some other processing, in each system call.

## Endianness

```
000000000001c2e0 <etens>:
   1c2e0:	6576                	ld	a0,344(sp)
   1c2e2:	4a92                	lw	s5,4(sp)
   1c2e4:	804a                	c.mv	zero,s2
   1c2e6:	c94c153f 8a20979a 	0x8a20979ac94c153f
   1c2ee:	5202                	lw	tp,32(sp)
   1c2f0:	c460                	sw	s0,76(s0)
   1c2f2:	7525                	lui	a0,0xfffe9
```
