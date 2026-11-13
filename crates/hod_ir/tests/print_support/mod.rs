//! Support shared by the printer's tests (`spec/ir.md` §9): a short way to
//! write function bodies through `FunctionBuilder`, and the LLVM tools of
//! one major version.
//!
//! Each test file uses part of this module.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use hod_ir::{
    BlockId, Body, CallDesc, ConstId, Dialect, FloatPredicate, FunctionBuilder, FunctionDesc,
    IntPredicate, Intrinsic, MemFlags, Module, Opcode, SymId, TypeId, TypeKind, Value,
};
use hod_testutil::{DEFAULT_TIMEOUT, Outcome, Scratch, Target, run, run_with, tool};

/// The module as text of `dialect`.
///
/// # Panics
/// Panics if the printer refuses it.
pub fn printed(m: &Module, dialect: Dialect, bodies: &[Body]) -> String {
    let mut text = String::new();
    m.print(&mut text, dialect, bodies)
        .unwrap_or_else(|error| panic!("LLVM {}: {error}", dialect.major()));
    text
}

/// Declares the function `name` of type `ret (params...)`, to be given a
/// body.
pub fn define(m: &mut Module, name: &str, ret: TypeId, params: &[TypeId]) -> SymId {
    let ty = m.types.function(ret, params, false).unwrap();
    m.add_function(name.as_bytes(), &FunctionDesc { has_body: true, ..FunctionDesc::new(ty) })
        .unwrap()
}

/// Declares the external function `name` of type `ret (params...)`, or
/// `ret (params..., ...)`.
pub fn declare(
    m: &mut Module,
    name: &str,
    ret: TypeId,
    params: &[TypeId],
    variadic: bool,
) -> SymId {
    let ty = m.types.function(ret, params, variadic).unwrap();
    m.add_function(name.as_bytes(), &FunctionDesc::new(ty)).unwrap()
}

/// The integer `value` of type `ty` as a constant.
pub fn int(m: &mut Module, ty: TypeId, value: i64) -> ConstId {
    m.consts.int_signed(&m.types, ty, value).unwrap()
}

/// A function body being written: the module and a builder, with every
/// step unwrapped.
pub struct Fx<'m> {
    pub m: &'m mut Module,
    pub b: FunctionBuilder,
}

impl<'m> Fx<'m> {
    /// Starts the body of `sym`, with its entry block open.
    pub fn new(m: &'m mut Module, sym: SymId) -> Fx<'m> {
        let ty = m.symbol(sym).ty();
        let mut b = FunctionBuilder::new(m, sym, ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        Fx { m, b }
    }

    /// The finished body, verified.
    pub fn done(self) -> Body {
        let body = self.b.finish().unwrap();
        body.verify(self.m).unwrap();
        body
    }

    /// The finished body, not verified.
    pub fn finish(self) -> Body {
        self.b.finish().unwrap()
    }

    pub fn arg(&self, index: u32) -> Value {
        self.b.arg(index).unwrap()
    }

    #[allow(clippy::unused_self, reason = "read as `x.konst(..)` among the builder calls")]
    pub fn konst(&self, id: ConstId) -> Value {
        Value::constant(id).unwrap()
    }

    #[allow(clippy::unused_self, reason = "read as `x.sym(..)` among the builder calls")]
    pub fn sym(&self, id: SymId) -> Value {
        Value::sym(id).unwrap()
    }

    /// The integer `value` of type `ty`.
    pub fn int(&mut self, ty: TypeId, value: i64) -> Value {
        Value::constant(int(self.m, ty, value)).unwrap()
    }

    pub fn i32(&mut self, value: i64) -> Value {
        self.int(TypeId::I32, value)
    }

    pub fn i64(&mut self, value: i64) -> Value {
        self.int(TypeId::I64, value)
    }

    pub fn f64(&mut self, value: f64) -> Value {
        Value::constant(self.m.consts.f64(value).unwrap()).unwrap()
    }

    pub fn f32(&mut self, value: f32) -> Value {
        Value::constant(self.m.consts.f32(value).unwrap()).unwrap()
    }

    pub fn null(&mut self) -> Value {
        Value::constant(self.m.consts.null(&self.m.types, TypeId::PTR).unwrap()).unwrap()
    }

    /// `@sym + addend`.
    pub fn addr(&mut self, sym: SymId, addend: i64) -> Value {
        Value::constant(self.m.consts.addr(sym, addend).unwrap()).unwrap()
    }

    /// A new block, not yet open.
    pub fn block(&mut self) -> BlockId {
        self.b.create_block().unwrap()
    }

    /// Opens `block`.
    pub fn at(&mut self, block: BlockId) {
        self.b.begin_block(block).unwrap();
    }

    pub fn here(&self) -> BlockId {
        self.b.current_block().unwrap()
    }

    pub fn bin(&mut self, op: Opcode, ty: TypeId, lhs: Value, rhs: Value) -> Value {
        self.b.binary(op, ty, lhs, rhs).unwrap()
    }

    pub fn icmp(&mut self, pred: IntPredicate, lhs: Value, rhs: Value) -> Value {
        self.b.icmp(pred, TypeId::I1, lhs, rhs).unwrap()
    }

    pub fn fcmp(&mut self, pred: FloatPredicate, lhs: Value, rhs: Value) -> Value {
        self.b.fcmp(pred, TypeId::I1, lhs, rhs).unwrap()
    }

    pub fn cast(&mut self, op: Opcode, ty: TypeId, arg: Value) -> Value {
        self.b.cast(op, ty, arg).unwrap()
    }

    /// The flags of a plain access to a `ty`.
    pub fn mem(&self, ty: TypeId) -> MemFlags {
        MemFlags::aligned(self.m.types.align_of(ty)).unwrap()
    }

    pub fn load(&mut self, ty: TypeId, ptr: Value) -> Value {
        let mem = self.mem(ty);
        self.b.load(ty, ptr, mem).unwrap()
    }

    pub fn store(&mut self, value: Value, ptr: Value) {
        let ty = self.b.value_type(self.m, value);
        let mem = self.mem(ty);
        self.b.store(value, ptr, mem).unwrap();
    }

    /// `base + offset + Σ index * scale`.
    pub fn ptr_add(&mut self, base: Value, offset: i64, indices: &[(Value, u64)]) -> Value {
        self.b.ptr_add(TypeId::PTR, base, offset, indices).unwrap()
    }

    pub fn select(&mut self, ty: TypeId, cond: Value, if_true: Value, if_false: Value) -> Value {
        self.b.select(ty, cond, if_true, if_false).unwrap()
    }

    pub fn phi(&mut self, ty: TypeId, incoming: &[(Value, BlockId)]) -> Value {
        self.b.phi(ty, incoming).unwrap()
    }

    /// The return type of the function type `fn_ty`.
    pub fn ret_type(&self, fn_ty: TypeId) -> TypeId {
        match self.m.types.kind(fn_ty) {
            TypeKind::Function { ret, .. } => ret,
            other => panic!("not a function type: {other:?}"),
        }
    }

    /// A plain call of the function `callee`.
    pub fn call(&mut self, callee: SymId, args: &[Value]) -> Value {
        let fn_ty = self.m.symbol(callee).ty();
        let ret = self.ret_type(fn_ty);
        self.b.call(ret, &CallDesc::new(Value::sym(callee).unwrap(), fn_ty, args)).unwrap()
    }

    /// A call of an intrinsic of type `ret (params...)`.
    pub fn intrinsic(
        &mut self,
        intrinsic: Intrinsic<'_>,
        overloads: &[TypeId],
        ret: TypeId,
        params: &[TypeId],
        args: &[Value],
    ) -> Value {
        let fn_ty = self.m.types.function(ret, params, false).unwrap();
        self.b.call(ret, &CallDesc::intrinsic(intrinsic, overloads, fn_ty, args)).unwrap()
    }

    pub fn br(&mut self, target: BlockId) {
        self.b.br(target).unwrap();
    }

    pub fn cond_br(&mut self, cond: Value, if_true: BlockId, if_false: BlockId) {
        self.b.cond_br(cond, if_true, if_false).unwrap();
    }

    pub fn ret(&mut self, value: Value) {
        self.b.ret(Some(value)).unwrap();
    }

    pub fn ret_void(&mut self) {
        self.b.ret(None).unwrap();
    }
}

/// The LLVM tools of one major version.
#[allow(clippy::struct_field_names, reason = "`llvm_as` is the tool's name")]
pub struct Llvm {
    pub major: u32,
    pub llvm_as: PathBuf,
    pub opt: PathBuf,
    pub llc: PathBuf,
    pub readelf: PathBuf,
    pub objdump: PathBuf,
}

impl Llvm {
    /// Finds the tools of LLVM `major`; `None`, after a note, when one is
    /// missing and tools are optional.
    ///
    /// # Panics
    /// Panics when one is missing and `HOD_REQUIRE_TOOLS=1`.
    pub fn new(major: u32) -> Option<Llvm> {
        Some(Llvm {
            major,
            llvm_as: tool(&format!("llvm-as-{major}"))?,
            opt: tool(&format!("opt-{major}"))?,
            llc: tool(&format!("llc-{major}"))?,
            // Reading an object does not depend on the version.
            readelf: tool("llvm-readelf")?,
            objdump: tool("llvm-objdump")?,
        })
    }

    pub fn dialect(&self) -> Dialect {
        Dialect::new(self.major).unwrap()
    }

    /// Requires `llvm-as` (with its verifier) and `opt -passes=verify` to
    /// accept `text`.
    ///
    /// # Panics
    /// Panics, with the diagnostics and the text, when either refuses.
    pub fn accept(&self, text: &str, what: &str) {
        self.accepted_by(&self.llvm_as, &["-o", "/dev/null", "-"], text, what);
        self.accepted_by(&self.opt, &["-passes=verify", "-disable-output", "-"], text, what);
    }

    /// Requires `llvm-as -disable-verify` to accept `text`: the parser
    /// alone.
    pub fn parse(&self, text: &str, what: &str) {
        self.accepted_by(&self.llvm_as, &["-disable-verify", "-o", "/dev/null", "-"], text, what);
    }

    fn accepted_by(&self, program: &Path, args: &[&str], text: &str, what: &str) -> Outcome {
        let outcome = run_with(Command::new(program).args(args), text.as_bytes(), DEFAULT_TIMEOUT);
        assert!(
            outcome.success(),
            "LLVM {}: {} refused {what}:\n{}\n{}",
            self.major,
            program.display(),
            outcome.stderr_text().chars().take(4000).collect::<String>(),
            numbered(text, 400),
        );
        outcome
    }

    /// What `llvm-as` (with its verifier) says against `text`, or `None`
    /// when it accepts it.
    pub fn objection(&self, text: &str) -> Option<String> {
        let outcome = run_with(
            Command::new(&self.llvm_as).args(["-o", "/dev/null", "-"]),
            text.as_bytes(),
            DEFAULT_TIMEOUT,
        );
        (!outcome.success()).then(|| outcome.stderr_text())
    }

    /// LLVM's own text for `text`: what `opt -S -passes=verify` prints.
    pub fn reprint(&self, text: &str, what: &str) -> String {
        self.accepted_by(&self.opt, &["-passes=verify", "-S", "-o", "-", "-"], text, what)
            .stdout_text()
    }

    /// Compiles `text` to the object `<name>.o` in `scratch` with `args`
    /// (`-O0`, `-relocation-model=pic`, ...).
    ///
    /// # Panics
    /// Panics, with the diagnostics, when `llc` fails.
    pub fn object(&self, scratch: &Scratch, name: &str, text: &str, args: &[&str]) -> PathBuf {
        let input = scratch.write(&format!("{name}.ll"), text);
        let output = scratch.join(format!("{name}.o"));
        let mut all: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
        all.extend(["-filetype=obj".as_ref(), "-o".as_ref(), output.as_os_str()]);
        all.push(input.as_os_str());
        let outcome = run(&self.llc, all);
        assert!(
            outcome.success(),
            "LLVM {}: llc {args:?} failed on {name}:\n{}\n{}",
            self.major,
            outcome.stderr_text().chars().take(4000).collect::<String>(),
            numbered(text, 400),
        );
        output
    }

    /// The output of `llvm-readelf` with `args` on `object`.
    pub fn readelf(&self, object: &Path, args: &[&str]) -> String {
        let mut all: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
        all.push(object.as_os_str());
        let outcome = run(&self.readelf, all);
        assert!(outcome.success(), "llvm-readelf failed: {}", outcome.stderr_text());
        outcome.stdout_text()
    }

    /// The contents of section `name` of `object`; empty when it has no
    /// such section.
    pub fn section(&self, object: &Path, name: &str) -> Vec<u8> {
        let outcome =
            run(&self.objdump, ["-s".as_ref(), "-j".as_ref(), name.as_ref(), object.as_os_str()]);
        assert!(outcome.success(), "llvm-objdump failed: {}", outcome.stderr_text());
        parse_hex_dump(&outcome.stdout_text())
    }
}

/// The first `limit` lines of `text`, numbered as LLVM's diagnostics
/// number them.
pub fn numbered(text: &str, limit: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (at, line) in text.lines().enumerate().take(limit) {
        let _ = writeln!(out, "{:5}  {line}", at + 1);
    }
    out
}

/// The bytes of an `llvm-objdump -s` hex dump. Each line is an address,
/// then up to sixteen bytes in four groups, then the same bytes as text.
pub fn parse_hex_dump(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut lines = text.lines().skip_while(|line| !line.starts_with("Contents of section"));
    lines.next();
    for line in lines {
        let Some(rest) = line.strip_prefix(' ') else { break };
        let Some(space) = rest.find(' ') else { break };
        let hex: String = rest[space..].chars().take(36).filter(|c| !c.is_whitespace()).collect();
        assert!(hex.len().is_multiple_of(2), "odd hex dump line: {line}");
        for at in (0..hex.len()).step_by(2) {
            bytes.push(u8::from_str_radix(&hex[at..at + 2], 16).expect("hex digits"));
        }
    }
    bytes
}

/// A symbol of an object, as `llvm-readelf -s` shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ElfSymbol {
    pub name: String,
    pub value: u64,
    pub size: u64,
    /// `OBJECT`, `FUNC`, `TLS`, `IFUNC`, `NOTYPE`, ...
    pub kind: String,
    /// `LOCAL`, `GLOBAL` or `WEAK`.
    pub bind: String,
    /// `DEFAULT`, `HIDDEN` or `PROTECTED`.
    pub visibility: String,
    /// The section's name, or `UND`, `COM`, `ABS`.
    pub section: String,
}

/// The sections (by index) and the symbols of `llvm-readelf -S -s --wide`
/// output.
pub fn parse_symbols(text: &str) -> (Vec<String>, Vec<ElfSymbol>) {
    let mut sections = Vec::new();
    let mut symbols = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('[') {
            // `[ 3] .data  PROGBITS ...`
            let Some((index, rest)) = rest.split_once(']') else { continue };
            let Ok(index) = index.trim().parse::<usize>() else { continue };
            assert_eq!(index, sections.len(), "sections out of order: {line}");
            let name = if index == 0 { "" } else { rest.split_whitespace().next().unwrap_or("") };
            sections.push(name.to_string());
            continue;
        }
        // `     6: 0000000000000010    16 OBJECT  GLOBAL DEFAULT     3 g0`
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        let numbered = fields.first().and_then(|first| first.strip_suffix(':'));
        if fields.len() < 7 || numbered.is_none_or(|number| number.parse::<u32>().is_err()) {
            continue;
        }
        let Ok(value) = u64::from_str_radix(fields[1], 16) else { continue };
        let size = match fields[2].strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).unwrap(),
            None => fields[2].parse().unwrap(),
        };
        let section = match fields[6].parse::<usize>() {
            Ok(index) => sections[index].clone(),
            Err(_) => fields[6].to_string(),
        };
        symbols.push(ElfSymbol {
            // A name may hold spaces.
            name: fields.get(7..).map(|rest| rest.join(" ")).unwrap_or_default(),
            value,
            size,
            kind: fields[3].to_string(),
            bind: fields[4].to_string(),
            visibility: fields[5].to_string(),
            section,
        });
    }
    (sections, symbols)
}

/// Links AArch64 `objects` with `gcc` (and `libs`, such as `-lstdc++`) and
/// runs the program: natively on an AArch64 host, else with the cross
/// `aarch64-linux-gnu-gcc` and under `qemu-aarch64` (`hod_testutil::target`).
/// `None`, after a note, without the tools.
///
/// # Panics
/// Panics when the link fails.
pub fn link_and_run(
    scratch: &Scratch,
    name: &str,
    objects: &[PathBuf],
    libs: &[&str],
) -> Option<Outcome> {
    let target = Target::Aarch64;
    let gcc = target.tool("gcc")?;
    let program = scratch.join(name);
    let mut args: Vec<&std::ffi::OsStr> = objects.iter().map(|object| object.as_os_str()).collect();
    args.extend(libs.iter().map(std::ffi::OsStr::new));
    args.extend(["-o".as_ref(), program.as_os_str()]);
    let linked = run(&gcc, args);
    assert!(linked.success(), "gcc failed to link {name}:\n{}", linked.stderr_text());
    if target.is_host() {
        return Some(run(&program, [] as [&str; 0]));
    }
    let (emulator, sysroot) = (target.tool("emulator")?, target.tool("sysroot")?);
    let mut emulated = target.emulator_args(&sysroot);
    emulated.push(program.into_os_string());
    Some(run(&emulator, emulated))
}
