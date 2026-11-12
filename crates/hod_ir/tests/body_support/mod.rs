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

/// The type LLVM spells `text`, interned in `types`: the inverse of
/// [`type_text`], and a function type as `ret (params)`.
///
/// # Panics
/// Panics if `text` is not such a spelling.
pub fn parse_type(types: &mut TypeTable, text: &str) -> TypeId {
    let mut rest = text;
    let ty = parse(types, &mut rest);
    assert!(rest.trim().is_empty(), "{rest:?} follows the type in {text:?}");
    ty
}

/// Consumes `token` if it is next.
fn eat(rest: &mut &str, token: &str) -> bool {
    match rest.trim_start().strip_prefix(token) {
        Some(after) => {
            *rest = after;
            true
        }
        None => false,
    }
}

fn expect(rest: &mut &str, token: &str) {
    assert!(eat(rest, token), "expected {token:?} at {rest:?}");
}

fn number(rest: &mut &str) -> u64 {
    let text = rest.trim_start();
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    *rest = &text[end..];
    text[..end].parse().unwrap_or_else(|_| panic!("expected a number at {text:?}"))
}

/// Types up to `close`, separated by commas; `...` sets the flag.
fn list(types: &mut TypeTable, rest: &mut &str, close: &str) -> (Vec<TypeId>, bool) {
    let (mut items, mut dots) = (Vec::new(), false);
    if eat(rest, close) {
        return (items, dots);
    }
    loop {
        if eat(rest, "...") {
            dots = true;
        } else {
            items.push(parse(types, rest));
        }
        if eat(rest, close) {
            return (items, dots);
        }
        expect(rest, ",");
    }
}

fn parse(types: &mut TypeTable, rest: &mut &str) -> TypeId {
    let ty = if eat(rest, "<{") {
        let fields = list(types, rest, "}>").0;
        types.struct_type(&fields, true).unwrap()
    } else if eat(rest, "{") {
        let fields = list(types, rest, "}").0;
        types.struct_type(&fields, false).unwrap()
    } else if eat(rest, "[") {
        let len = number(rest);
        expect(rest, "x");
        let elem = parse(types, rest);
        expect(rest, "]");
        types.array(elem, len).unwrap()
    } else if eat(rest, "<") {
        let scalable = eat(rest, "vscale");
        if scalable {
            expect(rest, "x");
        }
        let len = u32::try_from(number(rest)).unwrap();
        expect(rest, "x");
        let elem = parse(types, rest);
        expect(rest, ">");
        if scalable {
            types.scalable_vector(elem, len).unwrap()
        } else {
            types.vector(elem, len).unwrap()
        }
    } else {
        let text = rest.trim_start();
        let end = text.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(text.len());
        let (word, after) = text.split_at(end);
        *rest = after;
        match word {
            "void" => TypeId::VOID,
            "label" => TypeId::LABEL,
            "metadata" => TypeId::METADATA,
            "token" => TypeId::TOKEN,
            "half" => TypeId::HALF,
            "bfloat" => TypeId::BFLOAT,
            "float" => TypeId::FLOAT,
            "double" => TypeId::DOUBLE,
            "fp128" => TypeId::FP128,
            "x86_fp80" => TypeId::X86_FP80,
            "ptr" if eat(rest, "addrspace(") => {
                let addr_space = u32::try_from(number(rest)).unwrap();
                expect(rest, ")");
                types.ptr(addr_space).unwrap()
            }
            "ptr" => TypeId::PTR,
            _ => {
                let bits = word.strip_prefix('i').and_then(|bits| bits.parse().ok());
                types.int(bits.unwrap_or_else(|| panic!("no type {word:?}"))).unwrap()
            }
        }
    };
    if eat(rest, "(") {
        let (params, variadic) = list(types, rest, ")");
        return types.function(ty, &params, variadic).unwrap();
    }
    ty
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
