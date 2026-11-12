//! The frame (WP-033, `spec/codegen.md` §5.4, §5.5, §9.3): the prologue
//! and the epilogue, their bytes and their CFI rows, for frames at the
//! boundaries of the immediate forms; the incoming base; a dynamic slot;
//! the frame-size limit. The bytes are also read back by `llvm-objdump`.

mod common;

use common::{define, emit, int, module, objdump, write_object};
use hod_a64::A64;
use hod_a64::frame::MAX_A64_FRAME;
use hod_cg::{CfiOp, CfiRow, CgError, Chunk, LIMIT_FRAME_SIZE, Options, Scratch, compile_function};
use hod_ir::{Body, FunctionBuilder, Module, TypeId};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, Reg, RegSp, Size};

const SP: RegSp = RegSp::SP;

/// `i32 f() { ret i32 7 }` with a body slot of `slot` bytes aligned to
/// `align`.
fn with_slot(slot: u64, align: u64) -> (Module, Body) {
    let mut m = module();
    let (sym, ty) = define(&mut m, "f", TypeId::I32, &[]);
    let seven = int(&mut m, TypeId::I32, 7);
    let mut b = FunctionBuilder::new(&m, sym, ty).unwrap();
    if slot != 0 {
        b.add_slot(slot, align).unwrap();
    }
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    b.ret(Some(seven)).unwrap();
    (m, b.finish().unwrap())
}

fn compile(m: &Module, body: &Body) -> Result<Chunk, CgError> {
    let mut chunk = Chunk::new();
    compile_function(&A64, m, body, &Options::default(), &mut Scratch::new(), &mut chunk)?;
    Ok(chunk)
}

fn words(chunk: &Chunk) -> Vec<u32> {
    chunk.code.chunks(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect()
}

/// The prologue §5.5 specifies for `frame`.
fn prologue(frame: u32) -> [u32; 4] {
    [
        mc::sub_imm(Size::X, SP, SP, u64::from(frame & 0xfff)),
        mc::sub_imm(Size::X, SP, SP, u64::from(frame & 0xff_f000)),
        mc::stp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), SP, 0),
        mc::mov_sp(Size::X, Reg::X29.sp(), SP),
    ]
}

/// The epilogue §5.5 specifies for `frame`, without `mov sp, x29`.
fn epilogue(frame: u32) -> Vec<u32> {
    let mut out = vec![mc::ldp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), SP, 0)];
    let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
    if lo == 0 || hi == 0 {
        out.push(mc::add_imm(Size::X, SP, SP, u64::from(frame)));
    } else {
        out.push(mc::add_imm(Size::X, SP, SP, u64::from(hi)));
        out.push(mc::add_imm(Size::X, SP, SP, u64::from(lo)));
    }
    out.push(mc::ret(Reg::X30.zr()));
    out
}

/// The rows §5.5 and §9.3 specify: prologue at `p`, epilogue at `e`.
fn rows(frame: u32, p: u32, e: u32) -> Vec<CfiRow> {
    let f = frame as i32;
    let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
    let mut out = Vec::new();
    let row = |offset, op| CfiRow { offset, op };
    if lo != 0 {
        out.push(row(p + 4, CfiOp::DefCfaOffset(lo as i32)));
    }
    if hi != 0 {
        out.push(row(p + 8, CfiOp::DefCfaOffset(f)));
    }
    out.push(row(p + 12, CfiOp::Offset { reg: 29, cfa_offset: -f }));
    out.push(row(p + 12, CfiOp::Offset { reg: 30, cfa_offset: -f + 8 }));
    out.push(row(p + 16, CfiOp::DefCfaRegister(29)));
    out.push(row(e, CfiOp::DefCfa { reg: 31, offset: f }));
    out.push(row(e + 4, CfiOp::Restore { reg: 29 }));
    out.push(row(e + 4, CfiOp::Restore { reg: 30 }));
    if lo == 0 || hi == 0 {
        out.push(row(e + 8, CfiOp::DefCfaOffset(0)));
    } else {
        out.push(row(e + 8, CfiOp::DefCfaOffset(lo as i32)));
        out.push(row(e + 12, CfiOp::DefCfaOffset(0)));
    }
    out
}

/// What `llvm-objdump` prints for the prologue and epilogue of `frame`.
fn disassembly(frame: u32) -> Vec<String> {
    let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
    let mut out = vec![
        format!("sub sp, sp, #{lo:#x}"),
        if hi == 0 {
            "sub sp, sp, #0x0".into()
        } else {
            format!("sub sp, sp, #{:#x}, lsl #12", hi >> 12)
        },
        "stp x29, x30, [sp]".into(),
        "mov x29, sp".into(),
        "mov w0, #0x7".into(),
        "ldp x29, x30, [sp]".into(),
    ];
    if lo == 0 || hi == 0 {
        out.push(if hi == 0 {
            format!("add sp, sp, #{lo:#x}")
        } else {
            format!("add sp, sp, #{:#x}, lsl #12", hi >> 12)
        });
    } else {
        out.push(format!("add sp, sp, #{:#x}, lsl #12", hi >> 12));
        out.push(format!("add sp, sp, #{lo:#x}"));
    }
    out.push("ret".into());
    out
}

/// §5.5, §9.3: the prologue's immediates are patched in `end_function`,
/// and its rows are added there (`FnCx::cfi_at`), after the body's code:
/// for frames of 16 bytes (no body slot), 32, 4080, 4096 (12-bit
/// immediate empty), 4112 (both immediates) and the largest, 2^24 - 16.
#[test]
fn cfi_at_adds_the_prologue_rows_after_the_body() {
    let dir = hod_testutil::Scratch::new("a64-frame");
    for (slot, frame) in [
        (0, 16),
        (16, 32),
        (4064, 4080),
        (4080, 4096),
        (4096, 4112),
        (u64::from(MAX_A64_FRAME) - 16, MAX_A64_FRAME),
    ] {
        let (m, body) = with_slot(slot, 16);
        let chunk = compile(&m, &body).unwrap();
        let mut expect = prologue(frame).to_vec();
        expect.push(mc::movz(Size::W, Reg::X0.zr(), 7, 0));
        expect.extend(epilogue(frame));
        assert_eq!(words(&chunk), expect, "frame {frame}");
        assert_eq!(chunk.cfi, rows(frame, 0, 20), "frame {frame}");
        let emitted = emit(&m, std::slice::from_ref(&body), &Options::default());
        let object = write_object(&dir, &format!("frame-{frame}.o"), &emitted);
        if let Some(lines) = objdump(&object) {
            assert_eq!(lines, disassembly(frame), "frame {frame}");
        }
    }
}

/// §5.4, §10: one 16-byte step beyond the largest frame is the named
/// limit, and leaves no trace.
#[test]
fn a_frame_beyond_the_prologue_is_the_frame_size_limit() {
    let (m, body) = with_slot(u64::from(MAX_A64_FRAME), 16);
    let mut chunk = Chunk::new();
    let result =
        compile_function(&A64, &m, &body, &Options::default(), &mut Scratch::new(), &mut chunk);
    assert_eq!(result, Err(CgError::unsupported(LIMIT_FRAME_SIZE)));
    assert!(chunk.code.is_empty() && chunk.cfi.is_empty());
}

/// §5.5: a function with a stack-passed argument keeps the incoming base:
/// `mov x16, sp` before the prologue, stored after it, and its stack
/// argument read through it.
#[test]
fn a_stack_argument_is_read_through_the_incoming_base() {
    let mut m = module();
    let params = [TypeId::I64; 9];
    let (sym, ty) = define(&mut m, "ninth", TypeId::I64, &params);
    let mut b = FunctionBuilder::new(&m, sym, ty).unwrap();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    let ninth = b.arg(8).unwrap();
    b.ret(Some(ninth)).unwrap();
    let body = b.finish().unwrap();
    let chunk = compile(&m, &body).unwrap();
    let fp = Reg::X29.sp();
    let mut expect = vec![mc::mov_sp(Size::X, Reg::X16.sp(), SP)];
    expect.extend(prologue(32));
    expect.extend([
        mc::str_off(mc::MemSize::X, Reg::X16.zr(), fp, 16),
        mc::ldr_off(Load::X, Reg::X16.zr(), fp, 16),
        mc::ldr_off(Load::X, Reg::X0.zr(), Reg::X16.sp(), 0),
    ]);
    expect.extend(epilogue(32));
    assert_eq!(words(&chunk), expect, "{:08x?}", words(&chunk));
    assert_eq!(chunk.cfi, rows(32, 4, 32));
}

/// §5.1, §5.2: an over-aligned slot is allocated below the frame by the
/// entry code, and the epilogue then restores the stack pointer from the
/// frame pointer.
#[test]
fn a_dynamic_slot_restores_the_stack_pointer() {
    let (m, body) = with_slot(40, 64);
    let chunk = compile(&m, &body).unwrap();
    let fp = Reg::X29.sp();
    let x16 = Reg::X16;
    let mut expect = prologue(32).to_vec();
    expect.extend([
        mc::sub_imm(Size::X, x16.sp(), SP, 48),
        mc::and_imm(Size::X, x16.sp(), x16.zr(), !63),
        mc::mov_sp(Size::X, SP, x16.sp()),
        mc::str_off(mc::MemSize::X, x16.zr(), fp, 16),
        mc::movz(Size::W, Reg::X0.zr(), 7, 0),
        mc::mov_sp(Size::X, SP, fp),
    ]);
    expect.extend(epilogue(32));
    assert_eq!(words(&chunk), expect, "{:08x?}", words(&chunk));
    // The epilogue's rows start after `mov sp, x29`: until then the CFA
    // is still defined from x29.
    let last = &chunk.cfi[4];
    assert_eq!(*last, CfiRow { offset: 40, op: CfiOp::DefCfa { reg: 31, offset: 32 } });
}
