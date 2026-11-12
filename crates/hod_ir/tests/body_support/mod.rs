//! Support shared by the function-body tests: a text dump of a body made
//! only with the read API, LLVM spellings of hod types, and a bounded
//! `llvm-as`.
//!
//! Each test file uses part of this module.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use hod_ir::{Body, Module, TypeId, TypeKind, TypeTable};

/// Every fact the read API gives about `body`, as text: two bodies with
/// the same dump are the same to code generation.
pub fn dump(body: &Body, module: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "fn {:?} {:?} args={:?} entry={:?} locs={}",
        body.symbol(),
        body.fn_type(),
        body.arg_types(module),
        body.entry(),
        body.has_locs()
    );
    for index in 0..body.arg_count() {
        let _ = writeln!(out, "  arg{index}: {:?}", body.arg_uses(index));
    }
    for (index, slot) in body.slots().iter().enumerate() {
        let _ = writeln!(out, "  slot{index}: {slot:?}");
    }
    for var in body.slot_vars() {
        let _ = writeln!(out, "  {var:?}");
    }
    for block in body.blocks() {
        let successors: Vec<_> = block.successors().collect();
        let _ = writeln!(
            out,
            "{:?}: phis={} has_phis={} -> {successors:?}",
            block.id(),
            block.phis().len(),
            block.has_phis()
        );
        for inst in block.insts() {
            let _ = writeln!(
                out,
                "  {inst:?} ops={:?} [{:?}] loc={:?}",
                inst.operands(),
                inst.uses(),
                inst.loc()
            );
        }
    }
    for call in body.calls() {
        let _ = writeln!(out, "  call {call:?}");
    }
    for pad in body.landing_pads() {
        let _ = writeln!(out, "  pad {pad:?}");
    }
    out
}

/// `ty` as LLVM writes it.
///
/// # Panics
/// Panics for a type LLVM text cannot spell without a name.
pub fn type_text(types: &TypeTable, ty: TypeId) -> String {
    let list = |fields: &[TypeId]| {
        fields.iter().map(|&field| type_text(types, field)).collect::<Vec<_>>().join(", ")
    };
    match types.kind(ty) {
        TypeKind::Void => "void".to_string(),
        TypeKind::Label => "label".to_string(),
        TypeKind::Metadata => "metadata".to_string(),
        TypeKind::Token => "token".to_string(),
        TypeKind::Half => "half".to_string(),
        TypeKind::BFloat => "bfloat".to_string(),
        TypeKind::Float => "float".to_string(),
        TypeKind::Double => "double".to_string(),
        TypeKind::Fp128 => "fp128".to_string(),
        TypeKind::X86Fp80 => "x86_fp80".to_string(),
        TypeKind::Int(bits) => format!("i{bits}"),
        TypeKind::Ptr { addr_space: 0 } => "ptr".to_string(),
        TypeKind::Ptr { addr_space } => format!("ptr addrspace({addr_space})"),
        TypeKind::Vector { elem, len } => format!("<{len} x {}>", type_text(types, elem)),
        TypeKind::ScalableVector { elem, min_len } => {
            format!("<vscale x {min_len} x {}>", type_text(types, elem))
        }
        TypeKind::Array { elem, len } => format!("[{len} x {}]", type_text(types, elem)),
        TypeKind::Struct { fields, packed: false } => format!("{{ {} }}", list(fields)),
        TypeKind::Struct { fields, packed: true } => format!("<{{ {} }}>", list(fields)),
        TypeKind::Function { ret, params, variadic } => {
            let dots = match (variadic, params.is_empty()) {
                (false, _) => "",
                (true, true) => "...",
                (true, false) => ", ...",
            };
            format!("{} ({}{dots})", type_text(types, ret), list(params))
        }
        other => panic!("{other:?} has no spelling"),
    }
}

/// Whether `llvm_as` accepts `text`; `Err` holds its diagnostics.
pub fn assembles(llvm_as: &Path, text: &str) -> Result<(), String> {
    let outcome = hod_testutil::run_with(
        Command::new(llvm_as).args(["-o", "/dev/null", "-"]),
        text.as_bytes(),
        hod_testutil::DEFAULT_TIMEOUT,
    );
    assert!(!outcome.timed_out(), "llvm-as timed out on:\n{text}");
    if outcome.success() { Ok(()) } else { Err(outcome.stderr_text()) }
}

/// The path of LLVM `major`'s `name`, or `None` (after a note) when tools
/// are optional and it is missing.
pub fn llvm_tool(major: u32, name: &str) -> Option<std::path::PathBuf> {
    hod_testutil::tool(&format!("{name}-{major}"))
}

/// A small deterministic generator (splitmix64).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number below `bound`.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}
