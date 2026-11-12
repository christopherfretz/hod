//! Prints generated programs as LLVM assembly, for the LLVM oracle
//! (`spec/interp.md` §10).
//!
//! It prints the generator's subset of `hod_ir` and nothing more: anything
//! else is a panic naming the construct. Replace with the `hod_ir` printer
//! when WP-006 lands.
//!
//! Several programs go into one file: the symbols of each get a prefix,
//! and a `main` runs each entry function on its arguments. The file
//! defines the standard host functions itself: `sink` prints its argument
//! and `source` computes [`hod_interp::host::source_value`]. The other
//! declarations (the exception model's functions, its personality and
//! type-information objects, `spec/interp.md` §5.9) are external and keep
//! their names: the oracle links what defines them.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use hod_interp::generate::Generated;
use hod_interp::host;
use hod_ir::{
    BlockId, Body, Callee, Clause, Const, ConstId, Inst, InstKind, Module, Opcode, SymId,
    SymbolKind, TypeId, TypeKind, Value, ValueKind,
};

/// A file of programs under construction.
pub struct File {
    globals: String,
    functions: String,
    main: String,
    declares: BTreeSet<String>,
    programs: usize,
}

impl File {
    pub fn new() -> File {
        File {
            globals: String::new(),
            functions: String::new(),
            main: String::new(),
            declares: BTreeSet::new(),
            programs: 0,
        }
    }

    /// Adds a program. `main` will print `p <label>`, run it, and print
    /// `r <result>`; each call of `sink` prints `s <argument>` between.
    pub fn add(&mut self, label: u64, generated: &Generated) {
        let n = self.programs;
        self.programs += 1;
        let prefix = format!("p{n}_");
        let printer = Printer { m: &generated.module, prefix: &prefix };
        for symbol in generated.module.symbols() {
            let name = String::from_utf8_lossy(symbol.name());
            match symbol.kind() {
                SymbolKind::Variable { constant, init: None, .. } => {
                    let kind = if constant { "constant" } else { "global" };
                    let ty = printer.ty(symbol.ty());
                    self.declares.insert(format!("@{name} = external {kind} {ty}"));
                }
                SymbolKind::Function { has_body: false, .. }
                    if name != host::SINK && name != host::SOURCE =>
                {
                    let TypeKind::Function { ret, params, variadic } =
                        generated.module.types.kind(symbol.ty())
                    else {
                        panic!("function type");
                    };
                    let mut params: Vec<String> = params.iter().map(|&p| printer.ty(p)).collect();
                    if variadic {
                        params.push("...".into());
                    }
                    let ret = printer.ty(ret);
                    self.declares.insert(format!("declare {ret} @{name}({})", params.join(", ")));
                }
                _ => {}
            }
            if let SymbolKind::Variable { constant, init: Some(init), .. } = symbol.kind() {
                let kind = if constant { "constant" } else { "global" };
                let align = symbol.align().map_or(String::new(), |a| format!(", align {a}"));
                writeln!(
                    self.globals,
                    "{} = internal {kind} {}{align}",
                    printer.sym(symbol.id()),
                    printer.typed_const(init),
                )
                .unwrap();
            }
        }
        for body in &generated.bodies {
            let entry = body.symbol() == generated.entry;
            printer.function(body, entry, &mut self.functions, &mut self.declares);
        }
        let args: Vec<String> = generated
            .args
            .iter()
            .map(|arg| match arg {
                hod_interp::Val::Int { bits, value } => format!("i{bits} {value}"),
                other => panic!("argument {other:?}"),
            })
            .collect();
        writeln!(
            self.main,
            "  store i64 0, ptr @source_calls\n  \
             call i32 (ptr, ...) @printf(ptr @fmt_p, i64 {label})\n  \
             %r{n} = call i64 {}({})\n  \
             call i32 (ptr, ...) @printf(ptr @fmt_r, i64 %r{n})\n  \
             call i32 @fflush(ptr null)",
            printer.sym(generated.entry),
            args.join(", "),
        )
        .unwrap();
    }

    /// The text of the file, for `triple`.
    pub fn finish(self, triple: &str) -> String {
        let mut text = format!("target triple = \"{triple}\"\n\n");
        text.push_str(PRELUDE);
        text.push_str(&self.globals);
        text.push('\n');
        for declare in &self.declares {
            writeln!(text, "{declare}").unwrap();
        }
        text.push('\n');
        text.push_str(&self.functions);
        text.push_str("define i32 @main() {\n");
        text.push_str(&self.main);
        text.push_str("  ret i32 0\n}\n");
        text
    }
}

/// What `main` must print for a program whose run in the interpreter is
/// `outcome`.
pub fn expected(label: u64, outcome: &hod_interp::Outcome) -> String {
    let mut text = format!("p {label}\n");
    for event in &outcome.trace {
        if &*event.name == host::SINK {
            match event.args[..] {
                [hod_interp::Val::Int { value, .. }] => writeln!(text, "s {value}").unwrap(),
                _ => panic!("sink of {:?}", event.args),
            }
        }
    }
    match &outcome.result {
        Ok(Some(hod_interp::Val::Int { value, .. })) => writeln!(text, "r {value}").unwrap(),
        other => writeln!(text, "stopped: {other:?}").unwrap(),
    }
    text
}

/// The host functions (`spec/interp.md` §6.1) and what `main` prints with.
const PRELUDE: &str = r#"@fmt_p = private constant [8 x i8] c"p %llu\0A\00"
@fmt_s = private constant [8 x i8] c"s %llu\0A\00"
@fmt_r = private constant [8 x i8] c"r %llu\0A\00"
@source_calls = internal global i64 0

declare i32 @printf(ptr, ...)
declare i32 @fflush(ptr)

define void @sink(i64 %x) noinline {
  call i32 (ptr, ...) @printf(ptr @fmt_s, i64 %x)
  ret void
}

define i64 @source() noinline {
  %calls = load i64, ptr @source_calls
  %next = add i64 %calls, 1
  store i64 %next, ptr @source_calls
  %z0 = mul i64 %next, 11400714819323198485
  %s0 = lshr i64 %z0, 30
  %x0 = xor i64 %z0, %s0
  %z1 = mul i64 %x0, 13787848793156543929
  %s1 = lshr i64 %z1, 27
  %x1 = xor i64 %z1, %s1
  %z2 = mul i64 %x1, 10723151780598845931
  %s2 = lshr i64 %z2, 31
  %x2 = xor i64 %z2, %s2
  ret i64 %x2
}

"#;

struct Printer<'a> {
    m: &'a Module,
    prefix: &'a str,
}

impl Printer<'_> {
    /// A symbol: the program's own get its prefix, the host functions
    /// keep their names.
    fn sym(&self, sym: SymId) -> String {
        let symbol = self.m.symbol(sym);
        let name = std::str::from_utf8(symbol.name()).expect("a generated name is ASCII");
        match symbol.kind() {
            SymbolKind::Function { has_body: false, .. }
            | SymbolKind::Variable { init: None, .. } => format!("@{name}"),
            _ => format!("@{}{name}", self.prefix),
        }
    }

    fn ty(&self, ty: TypeId) -> String {
        match self.m.types.kind(ty) {
            TypeKind::Void => "void".into(),
            TypeKind::Int(bits) => format!("i{bits}"),
            TypeKind::Float => "float".into(),
            TypeKind::Double => "double".into(),
            TypeKind::Ptr { addr_space: 0 } => "ptr".into(),
            TypeKind::Array { elem, len } => format!("[{len} x {}]", self.ty(elem)),
            TypeKind::Struct { fields, packed: false } => {
                let fields: Vec<String> = fields.iter().map(|&field| self.ty(field)).collect();
                format!("{{ {} }}", fields.join(", "))
            }
            other => panic!("type {other:?}"),
        }
    }

    fn constant(&self, id: ConstId) -> String {
        match self.m.consts.get(id) {
            Const::Int { value, .. } => value.to_string(),
            Const::WideInt { words, .. } => {
                let low = u128::from(words.first().copied().unwrap_or(0));
                let high = u128::from(words.get(1).copied().unwrap_or(0));
                (low | (high << 64)).to_string()
            }
            // LLVM writes a `float` as the `double` of the same value.
            Const::Float { ty: TypeId::FLOAT, bits } => {
                format!("0x{:016X}", f64::from(f32::from_bits(bits as u32)).to_bits())
            }
            Const::Float { bits, .. } => format!("0x{bits:016X}"),
            Const::Null(_) => "null".into(),
            // An aggregate of zeros is interned as this.
            Const::Zero(_) => "zeroinitializer".into(),
            Const::Addr { sym, addend: 0 } => self.sym(sym),
            Const::BlockAddr { func, block } => {
                format!("blockaddress({}, %b{})", self.sym(func), block.index())
            }
            Const::Aggregate { ty, elems } => {
                let elems: Vec<String> = elems.iter().map(|&e| self.typed_const(e)).collect();
                match self.m.types.kind(ty) {
                    TypeKind::Array { .. } => format!("[{}]", elems.join(", ")),
                    _ => format!("{{ {} }}", elems.join(", ")),
                }
            }
            Const::Bytes { bytes, .. } => {
                let mut text = String::from("c\"");
                for byte in bytes {
                    write!(text, "\\{byte:02X}").unwrap();
                }
                text.push('"');
                text
            }
            other => panic!("constant {other:?}"),
        }
    }

    fn typed_const(&self, id: ConstId) -> String {
        format!("{} {}", self.ty(self.m.consts.ty(id)), self.constant(id))
    }

    fn value(&self, value: Value) -> String {
        match value.kind() {
            ValueKind::Inst(id) => format!("%v{}", id.index()),
            ValueKind::Arg(index) => format!("%a{index}"),
            ValueKind::Const(id) => self.constant(id),
            ValueKind::Sym(sym) => self.sym(sym),
            ValueKind::Slot(slot) => format!("%s{}", slot.index()),
            other => panic!("value {other:?}"),
        }
    }

    fn typed(&self, body: &Body, value: Value) -> String {
        format!("{} {}", self.ty(body.value_type(self.m, value)), self.value(value))
    }

    fn function(
        &self,
        body: &Body,
        entry: bool,
        out: &mut String,
        declares: &mut BTreeSet<String>,
    ) {
        let TypeKind::Function { ret, params, variadic: false } = self.m.types.kind(body.fn_type())
        else {
            panic!("function type");
        };
        let params: Vec<String> = params
            .iter()
            .enumerate()
            .map(|(index, &param)| format!("{} %a{index}", self.ty(param)))
            .collect();
        // The entry function is compiled as it stands; the others may be
        // inlined and specialized.
        let (linkage, mut attrs) =
            if entry { ("", " noinline".to_owned()) } else { ("internal ", String::new()) };
        if let SymbolKind::Function { personality: Some(personality), .. } =
            self.m.symbol(body.symbol()).kind()
        {
            write!(attrs, " personality ptr {}", self.sym(personality)).unwrap();
        }
        writeln!(
            out,
            "define {linkage}{} {}({}){attrs} {{",
            self.ty(ret),
            self.sym(body.symbol()),
            params.join(", ")
        )
        .unwrap();
        // LLVM wants the entry block first.
        let first = body.entry();
        let rest = body.blocks().filter(|block| block.id() != first);
        for block in std::iter::once(body.block(first)).chain(rest) {
            writeln!(out, "b{}:", block.id().index()).unwrap();
            if block.id() == first {
                for (index, slot) in body.slots().iter().enumerate() {
                    let (size, align) = (slot.size(), slot.align());
                    writeln!(out, "  %s{index} = alloca [{size} x i8], align {align}").unwrap();
                }
            }
            for inst in block.insts() {
                self.inst(body, inst, out, declares);
            }
        }
        out.push_str("}\n\n");
    }

    fn label(block: BlockId) -> String {
        format!("label %b{}", block.index())
    }

    #[allow(clippy::too_many_lines)]
    fn inst(&self, body: &Body, inst: Inst<'_>, out: &mut String, declares: &mut BTreeSet<String>) {
        let name = format!("%v{}", inst.id().index());
        let op = inst.opcode().name();
        let ty = self.ty(inst.ty());
        let v = |value: Value| self.value(value);
        let tv = |value: Value| self.typed(body, value);
        let line = match inst.kind() {
            InstKind::Binary { lhs, rhs } => format!("{name} = {op} {}, {}", tv(lhs), v(rhs)),
            InstKind::Unary { arg } => format!("{name} = {op} {}", tv(arg)),
            InstKind::Cast { arg } => format!("{name} = {op} {} to {ty}", tv(arg)),
            InstKind::ICmp { pred, lhs, rhs } => {
                format!("{name} = icmp {} {}, {}", pred.name(), tv(lhs), v(rhs))
            }
            InstKind::FCmp { pred, lhs, rhs } => {
                format!("{name} = fcmp {} {}, {}", pred.name(), tv(lhs), v(rhs))
            }
            InstKind::Load { ptr, mem } => {
                format!("{name} = load {ty}, ptr {}, align {}", v(ptr), mem.align())
            }
            InstKind::Store { value, ptr, mem } => {
                format!("store {}, ptr {}, align {}", tv(value), v(ptr), mem.align())
            }
            InstKind::PtrAdd(add) => {
                // Byte offsets: the constant one, then each index, sign
                // extended to the width of a pointer and scaled.
                let mut parts = Vec::new();
                if add.offset != 0 || add.index_count() == 0 {
                    parts.push(add.offset.to_string());
                }
                for (n, (index, scale)) in add.indices().enumerate() {
                    let TypeKind::Int(bits) = self.m.types.kind(body.value_type(self.m, index))
                    else {
                        panic!("index type");
                    };
                    let wide = match bits {
                        64 => v(index),
                        0..64 => {
                            writeln!(out, "  {name}.x{n} = sext {} to i64", tv(index)).unwrap();
                            format!("{name}.x{n}")
                        }
                        _ => panic!("index of {bits} bits"),
                    };
                    writeln!(out, "  {name}.m{n} = mul i64 {wide}, {scale}").unwrap();
                    parts.push(format!("{name}.m{n}"));
                }
                let mut at = v(add.base);
                let last = parts.len() - 1;
                for (n, part) in parts.iter().enumerate() {
                    let to = if n == last { name.clone() } else { format!("{name}.p{n}") };
                    writeln!(out, "  {to} = getelementptr i8, ptr {at}, i64 {part}").unwrap();
                    at = to;
                }
                return;
            }
            InstKind::ExtractValue { agg, offset } => {
                // Only the pairs of the checked-arithmetic intrinsics and
                // of landing pads.
                let index = u32::from(offset != 0);
                format!("{name} = extractvalue {}, {index}", tv(agg))
            }
            InstKind::Phi(phi) => {
                let incoming: Vec<String> = phi
                    .incoming()
                    .map(|(value, block)| format!("[ {}, %b{} ]", v(value), block.index()))
                    .collect();
                format!("{name} = phi {ty} {}", incoming.join(", "))
            }
            InstKind::Select { cond, if_true, if_false } => {
                format!("{name} = select i1 {}, {}, {}", v(cond), tv(if_true), tv(if_false))
            }
            InstKind::Call(call) => {
                let args: Vec<String> = call.args().iter().map(|&arg| tv(arg)).collect();
                let callee = match call.callee() {
                    Callee::Value(callee) => v(callee),
                    Callee::Intrinsic(intrinsic) => {
                        let mangled = intrinsic
                            .mangled_name(&self.m.types, call.overloads())
                            .expect("an intrinsic the generator uses has a name");
                        let TypeKind::Function { params, .. } = self.m.types.kind(call.fn_type())
                        else {
                            panic!("function type");
                        };
                        let params: Vec<String> = params.iter().map(|&p| self.ty(p)).collect();
                        declares.insert(format!("declare {ty} @{mangled}({})", params.join(", ")));
                        format!("@{mangled}")
                    }
                };
                let result =
                    if inst.ty() == TypeId::VOID { String::new() } else { format!("{name} = ") };
                let attrs = if call.is_nounwind() { " nounwind" } else { "" };
                match (call.normal_dest(), call.unwind_dest()) {
                    (Some(normal), Some(unwind)) => format!(
                        "{result}invoke {ty} {callee}({}){attrs} to {} unwind {}",
                        args.join(", "),
                        Self::label(normal),
                        Self::label(unwind)
                    ),
                    _ => format!("{result}call {ty} {callee}({}){attrs}", args.join(", ")),
                }
            }
            InstKind::LandingPad(pad) => {
                let mut line = format!("{name} = landingpad {ty}");
                if pad.is_cleanup() {
                    line.push_str(" cleanup");
                }
                for clause in pad.clauses() {
                    match *clause {
                        Clause::Catch(c) => write!(line, " catch {}", self.typed_const(c)),
                        Clause::Filter(c) => write!(line, " filter {}", self.typed_const(c)),
                    }
                    .unwrap();
                }
                line
            }
            InstKind::Resume { value } => format!("resume {}", tv(value)),
            InstKind::IndirectBr { addr, targets } => {
                let targets: Vec<String> = targets.into_iter().map(Self::label).collect();
                format!("indirectbr {}, [{}]", tv(addr), targets.join(", "))
            }
            InstKind::Br { target } => format!("br {}", Self::label(target)),
            InstKind::CondBr { cond, if_true, if_false } => {
                format!("br i1 {}, {}, {}", v(cond), Self::label(if_true), Self::label(if_false))
            }
            InstKind::Switch(switch) => {
                let cases: Vec<String> = switch
                    .cases()
                    .map(|(case, to)| format!("{}, {}", self.typed_const(case), Self::label(to)))
                    .collect();
                format!(
                    "switch {}, {} [ {} ]",
                    tv(switch.cond),
                    Self::label(switch.default),
                    cases.join(" ")
                )
            }
            InstKind::Ret { value: Some(value) } => format!("ret {}", tv(value)),
            InstKind::Ret { value: None } => "ret void".into(),
            InstKind::Unreachable => "unreachable".into(),
            _ => panic!("instruction {:?}", inst.opcode()),
        };
        debug_assert!(inst.opcode() != Opcode::PtrAdd);
        writeln!(out, "  {line}").unwrap();
    }
}
