//! Support shared by `hod_bc`'s tests: the corpus, the LLVM tools, and the
//! structural dump the differential compares (`spec/bc.md` §9).
#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod_ir::{Const, ConstId, Module, ParamAttrs, SymId, SymbolKind, TypeId, TypeKind};
use hod_testutil::{DEFAULT_TIMEOUT, run_with};

/// The `bin` directory of LLVM `major`.
pub fn llvm_bin(major: u32) -> Option<PathBuf> {
    let bin = PathBuf::from(format!("/usr/lib/llvm-{major}/bin"));
    bin.join("llvm-as").exists().then_some(bin)
}

/// `llvm-as` of `major` on `text`: the bitcode.
pub fn assemble(major: u32, text: &[u8]) -> Result<Vec<u8>, String> {
    let bin = llvm_bin(major).ok_or("no llvm-as")?;
    let out =
        run_with(Command::new(bin.join("llvm-as")).args(["-", "-o", "-"]), text, DEFAULT_TIMEOUT);
    if out.success() { Ok(out.stdout) } else { Err(out.stderr_text()) }
}

/// `llvm-dis` of `major` on `bitcode`: the text.
pub fn disassemble(major: u32, bitcode: &[u8]) -> Result<Vec<u8>, String> {
    let bin = llvm_bin(major).ok_or("no llvm-dis")?;
    let out = run_with(
        Command::new(bin.join("llvm-dis")).args(["-", "-o", "-"]),
        bitcode,
        DEFAULT_TIMEOUT,
    );
    if out.success() { Ok(out.stdout) } else { Err(out.stderr_text()) }
}

/// Every `*.ll` under `dir`, sorted.
pub fn modules(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "ll") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// The LLVM major a corpus path is under (`…/llvm19/…`).
pub fn major_of(path: &Path) -> Option<u32> {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .find_map(|part| part.strip_prefix("llvm").and_then(|n| n.parse().ok()))
}

/// A type, written out in full.
pub fn ty(module: &Module, id: TypeId) -> String {
    let types = &module.types;
    match types.kind(id) {
        TypeKind::Int(bits) => format!("i{bits}"),
        TypeKind::Ptr { addr_space } => format!("ptr{addr_space}"),
        TypeKind::Vector { elem, len } => format!("<{len} x {}>", ty(module, elem)),
        TypeKind::ScalableVector { elem, min_len } => {
            format!("<vscale {min_len} x {}>", ty(module, elem))
        }
        TypeKind::Array { elem, len } => format!("[{len} x {}]", ty(module, elem)),
        TypeKind::Struct { fields, packed } => {
            let fields: Vec<String> = fields.iter().map(|&f| ty(module, f)).collect();
            format!("{}{{{}}}", if packed { "packed" } else { "" }, fields.join(", "))
        }
        // Opaque structs are numbered in the order they are made, which
        // the two readers need not share.
        TypeKind::Opaque(_) => "opaque".into(),
        TypeKind::Function { ret, params, variadic } => {
            let params: Vec<String> = params.iter().map(|&p| ty(module, p)).collect();
            format!(
                "{} ({}{})",
                ty(module, ret),
                params.join(", "),
                if variadic { ", ..." } else { "" }
            )
        }
        other => format!("{other:?}"),
    }
}

/// A constant, written out in full.
pub fn constant(module: &Module, id: ConstId, name: &dyn Fn(SymId) -> String) -> String {
    let c = |id| constant(module, id, name);
    let t = |id| ty(module, id);
    match module.consts.get(id) {
        Const::Int { ty, value } => format!("{} {value}", t(ty)),
        Const::WideInt { ty, words } => format!("{} {words:x?}", t(ty)),
        Const::Float { ty, bits } => format!("{} f{bits:x}", t(ty)),
        Const::WideFloat { ty, bits } => format!("{} f{bits:x}", t(ty)),
        Const::Null(ty) => format!("{} null", t(ty)),
        Const::Zero(ty) => format!("{} zero", t(ty)),
        Const::Undef(ty) => format!("{} undef", t(ty)),
        Const::Poison(ty) => format!("{} poison", t(ty)),
        Const::NoneToken => "none".into(),
        Const::Aggregate { ty, elems } => {
            let elems: Vec<String> = elems.iter().map(|&e| c(e)).collect();
            format!("{} [{}]", t(ty), elems.join(", "))
        }
        Const::Splat { ty, elem } => format!("{} splat({})", t(ty), c(elem)),
        Const::Bytes { ty, bytes } => format!("{} c{:?}", t(ty), String::from_utf8_lossy(bytes)),
        Const::Addr { sym, addend } => format!("@{}{addend:+}", name(sym)),
        Const::BlockAddr { func, block } => format!("blockaddress(@{}, {block:?})", name(func)),
        Const::SymDiff { ty, a, b, addend, plt } => {
            format!("{} @{}-@{}{addend:+} plt={plt}", t(ty), name(a), name(b))
        }
        Const::IntOfAddr { ty, addr } => format!("{} int({})", t(ty), c(addr)),
        Const::AddrOfInt { value } => format!("addr({value})"),
        Const::DsoLocalEquivalent(sym) => format!("dso_local_equivalent @{}", name(sym)),
        Const::NoCfi(sym) => format!("no_cfi @{}", name(sym)),
        Const::Expr { op, ty, source_ty, args } => {
            let args: Vec<String> = args.iter().map(|&a| c(a)).collect();
            let source = source_ty.map(t).unwrap_or_default();
            format!("{} {op:?}<{source}>({})", t(ty), args.join(", "))
        }
    }
}

fn params(module: &Module, attrs: ParamAttrs) -> String {
    format!(
        "{:?}/{:?}/{:?}/{:?}/{}",
        attrs.mem(),
        attrs.mem_type().map(|id| ty(module, id)),
        attrs.align(),
        attrs.stack_align(),
        [
            ParamAttrs::ZEROEXT,
            ParamAttrs::SIGNEXT,
            ParamAttrs::INREG,
            ParamAttrs::NEST,
            ParamAttrs::SWIFT_SELF,
            ParamAttrs::SWIFT_ERROR,
            ParamAttrs::SWIFT_ASYNC
        ]
        .map(|flag| if attrs.has(flag) { '1' } else { '0' })
        .iter()
        .collect::<String>()
    )
}

/// Everything delivered for a module, as lines that do not depend on the
/// order things were made in: symbols by name (an unnamed one by its
/// place in `numbered`) with every field, types and constants written
/// out structurally, notes, and the module-level lists.
pub fn dump(module: &Module, numbered: &[SymId]) -> String {
    let name = |sym: SymId| {
        let symbol = module.symbol(sym);
        if symbol.is_anonymous() {
            let at = numbered.iter().position(|&other| other == sym);
            return format!("<anonymous {at:?}>");
        }
        String::from_utf8_lossy(symbol.name()).into_owned()
    };
    let mut lines = Vec::new();
    for symbol in module.symbols() {
        let mut line = format!(
            "{} {:?} {:?} dso_local={} {:?} section={:?} comdat={:?} align={:?} as={} tls={:?} \
             used={} compiler_used={} type={}",
            name(symbol.id()),
            symbol.linkage(),
            symbol.visibility(),
            symbol.dso_local(),
            symbol.unnamed_addr(),
            symbol.section().map(String::from_utf8_lossy),
            symbol.comdat().map(|id| {
                let comdat = module.comdat(id);
                format!("{}/{:?}", String::from_utf8_lossy(comdat.name), comdat.kind)
            }),
            symbol.align(),
            symbol.addr_space(),
            symbol.tls(),
            symbol.is_used(),
            symbol.is_compiler_used(),
            ty(module, symbol.ty()),
        );
        match symbol.kind() {
            SymbolKind::Unresolved => line.push_str(" unresolved"),
            SymbolKind::Variable { constant: is_constant, init, .. } => {
                let _ = write!(line, " variable constant={is_constant}");
                if let Some(init) = init {
                    let _ = write!(line, " init={}", constant(module, init, &name));
                }
            }
            SymbolKind::Function {
                cc,
                attrs,
                ret_attrs,
                param_attrs,
                personality,
                has_body,
                ..
            } => {
                let _ = write!(
                    line,
                    " function cc={} attrs={:?} ret={} params=[{}] personality={:?} body={has_body}",
                    cc.llvm_id(),
                    module.fn_attrs(attrs),
                    params(module, ret_attrs),
                    param_attrs.iter().map(|&a| params(module, a)).collect::<Vec<_>>().join(", "),
                    personality.map(&name),
                );
            }
            SymbolKind::Alias { target, .. } => {
                let _ = write!(line, " alias of {}", constant(module, target, &name));
            }
            SymbolKind::IFunc { resolver } => {
                let _ = write!(line, " ifunc resolved by {}", name(resolver));
            }
        }
        lines.push(line);
    }
    for unsupported in module.unsupported() {
        lines.push(format!(
            "unsupported {} {:?}",
            unsupported.construct,
            unsupported.symbol.map(&name)
        ));
    }
    lines.sort();
    lines.push(format!("props {:?}", module.props));
    for flag in module.other_flags() {
        lines.push(format!("flag {flag:?}"));
    }
    for (what, list) in [("ctor", module.ctors()), ("dtor", module.dtors())] {
        for entry in list {
            let associated = entry.associated.map(&name);
            lines.push(format!("{what} {} {} {associated:?}", entry.priority, name(entry.func)));
        }
    }
    for (what, lists) in
        [("linker option", module.linker_options()), ("library", module.dependent_libraries())]
    {
        for list in lists.iter() {
            let strings: Vec<_> = list.map(String::from_utf8_lossy).collect();
            lines.push(format!("{what} {strings:?}"));
        }
    }
    lines.push(format!("module asm {:?}", String::from_utf8_lossy(module.module_asm())));
    lines.push(format!("target {:?}", module.target()));
    lines.join("\n")
}

/// `hod_ll`'s reading of `text`: the module and its numbered globals.
pub fn read_text(text: &[u8]) -> Result<(Module, Vec<SymId>), String> {
    let parsed =
        hod_ll::parse_module(text, &hod_ll::ParseOptions::default()).map_err(|e| e.to_string())?;
    let numbered: Vec<SymId> = (0..).map_while(|n| parsed.text.numbered_global(n)).collect();
    Ok((parsed.module, numbered))
}

/// `hod_bc`'s reading of `bitcode`.
pub fn read_bitcode(bitcode: &[u8]) -> Result<(Module, Vec<SymId>), String> {
    let read =
        hod_bc::read_module(bitcode, &hod_bc::ReadOptions::default()).map_err(|e| e.to_string())?;
    Ok((read.module, read.bits.numbered_globals().to_vec()))
}
