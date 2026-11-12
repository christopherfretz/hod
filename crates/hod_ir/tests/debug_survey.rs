//! What compilers emit as debug information, against the field table
//! (`spec/ir.md` §6.9): for a C and a C++ translation unit written to
//! use every construct that has debug information, clang 19 and 22 (and
//! kiln, when it is built), and the flag sets below, every `!DIKind`,
//! every field, every `DIExpression` operation and every debug record
//! is *mapped*, *dropped* or *unsupported*; at `-g -O0` nothing is
//! unsupported; and the metadata, loaded through the builder API the
//! way a producer would, verifies (§6.8).
//!
//! There is no IR parser yet, so the test reads the metadata from the
//! compiler's text with a loader of its own. It is driven by the field
//! table and `DiNode::set`, as §6.9 says a producer is. Function bodies
//! are not loaded: each becomes a body with one instruction for every
//! instruction that has a location, and the function's debug records in
//! their places, which is what the rules of §6.8 are about.

mod modlevel;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use hod_ir::{
    ChecksumKind, ConstId, DiConstant, DiFieldValue, DiFlags, DiId, DiKind, DiLocation, DiMapping,
    DiNode, DiOther, DiString, DiTuple, EmissionKind, ExprId, FunctionBuilder, FunctionDesc,
    IrResult, LocId, Module, NameTableKind, Opcode, SpFlags, TargetSpec, TypeId, Value,
    VariableDesc, di_field, di_kind, di_op_named, di_record, dw_ate, dw_cc, dw_lang, dw_op, dw_tag,
};
use hod_testutil::{Scratch, tools_required};
use modlevel::{MAJORS, llvm_tool};

type Res<T> = Result<T, String>;

fn s<T>(result: IrResult<T>) -> Res<T> {
    result.map_err(|error| error.to_string())
}

/// Splits at the commas that are outside strings and brackets.
fn split_top(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut start) = (0_u32, false, 0);
    for (at, byte) in text.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            _ if quoted => {}
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

/// What is between the bracket at `open` and the one that closes it.
fn balanced(text: &str, open: usize) -> &str {
    let (mut depth, mut quoted) = (0_u32, false);
    for (at, byte) in text.bytes().enumerate().skip(open) {
        match byte {
            b'"' => quoted = !quoted,
            _ if quoted => {}
            b'(' | b'{' => depth += 1,
            b')' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..at];
                }
            }
            _ => {}
        }
    }
    &text[open + 1..]
}

/// Every `!Kind(...)` written in `line`, nested ones included: the kind
/// and what is between its parentheses.
fn nodes_in(line: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = line[from..].find('!') {
        let start = from + at + 1;
        let len = line[start..].bytes().take_while(u8::is_ascii_alphanumeric).count();
        let name = &line[start..start + len];
        from = start + len;
        let node = name.starts_with("DI") || name == "GenericDINode";
        if node && line[from..].starts_with('(') {
            found.push((name, balanced(line, from)));
        }
    }
    found
}

/// `name: value` for each field between a node's parentheses.
fn fields(body: &str) -> Vec<(&str, &str)> {
    split_top(body).into_iter().filter_map(|field| field.split_once(": ")).collect()
}

/// The number of `!N`.
fn number(text: &str) -> Option<u32> {
    text.strip_prefix('!')?.parse().ok()
}

/// The bytes of a string as LLVM writes it between quotes.
fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes.get(at + 1..at + 3).and_then(|hex| std::str::from_utf8(hex).ok());
        match (bytes[at], hex.and_then(|hex| u8::from_str_radix(hex, 16).ok())) {
            (b'\\', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (b'\\', None) => {
                out.push(b'\\');
                at += 2;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// The name after the `@` that `text` starts behind.
fn symbol_name(text: &str) -> Vec<u8> {
    if let Some(quoted) = text.strip_prefix('"') {
        return unescape(quoted.split('"').next().unwrap_or(""));
    }
    let end = text.find([' ', '(', ',', ')']).unwrap_or(text.len());
    text.as_bytes()[..end].to_vec()
}

/// The numbers of the `!dbg` attachments of a line.
fn dbg_of(line: &str) -> impl Iterator<Item = u32> + '_ {
    line.split("!dbg !").skip(1).filter_map(|rest| {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        rest[..digits].parse().ok()
    })
}

/// The debug record or debug intrinsic call a line of a function is:
/// LLVM's name for it, and its arguments with the location last.
fn record_of(line: &str) -> Option<(&str, Vec<&str>)> {
    if line.starts_with("#dbg_") {
        let open = line.find('(')?;
        return Some((&line[..open], split_top(balanced(line, open))));
    }
    let at = line.find("@llvm.dbg.").filter(|_| line.contains("call void"))?;
    let open = at + line[at..].find('(')?;
    let mut args: Vec<&str> = split_top(balanced(line, open))
        .into_iter()
        .map(|arg| arg.strip_prefix("metadata ").unwrap_or(arg))
        .collect();
    let loc = line.rfind("!dbg ").map_or("", |at| line[at + 5..].trim());
    args.push(loc);
    Some((&line[at + 1..open], args))
}

/// What a run wrote: kinds, `(kind, field)`s, expression operations and
/// records, by LLVM's names.
#[derive(Default, Clone, PartialEq, Eq, Debug)]
struct Found {
    kinds: BTreeSet<String>,
    fields: BTreeSet<(String, String)>,
    ops: BTreeSet<String>,
    records: BTreeSet<String>,
}

impl Found {
    fn scan(ir: &str) -> Found {
        let mut found = Found::default();
        for line in ir.lines() {
            for (kind, body) in nodes_in(line) {
                found.kinds.insert(kind.to_string());
                if kind == "DIExpression" {
                    let ops = split_top(body).into_iter().filter(|word| word.starts_with("DW_OP_"));
                    found.ops.extend(ops.map(str::to_string));
                } else if kind != "DIArgList" {
                    let named = fields(body).into_iter().map(|(name, _)| (kind.into(), name.into()));
                    found.fields.extend(named);
                }
            }
            let line = line.trim_start();
            if line.starts_with("declare ") {
                continue;
            }
            if let Some((name, args)) = record_of(line) {
                found.records.insert(name.to_string());
                if name.ends_with("assign") && args[0].starts_with("!DIArgList") {
                    found.records.insert("#dbg_assign.DIArgList".to_string());
                }
            }
        }
        found
    }

    fn add(&mut self, other: &Found) {
        self.kinds.extend(other.kinds.iter().cloned());
        self.fields.extend(other.fields.iter().cloned());
        self.ops.extend(other.ops.iter().cloned());
        self.records.extend(other.records.iter().cloned());
    }

    /// The construct names of what is unsupported, and what the tables
    /// do not know at all.
    fn classify(&self) -> (BTreeSet<String>, Vec<String>) {
        let (mut unsupported, mut unknown) = (BTreeSet::new(), Vec::new());
        let mut note = |mapping: Option<DiMapping>, what: String| match mapping {
            Some(DiMapping::Mapped | DiMapping::Dropped(_)) => {}
            Some(DiMapping::Unsupported(name)) => {
                unsupported.insert(name.to_string());
            }
            None => unknown.push(what),
        };
        for kind in &self.kinds {
            note(di_kind(kind.as_bytes()), format!("kind {kind}"));
        }
        for (kind, field) in &self.fields {
            if di_kind(kind.as_bytes()) == Some(DiMapping::Mapped) {
                note(di_field(kind.as_bytes(), field.as_bytes()), format!("field {kind}.{field}"));
            }
        }
        for record in &self.records {
            note(di_record(record.as_bytes()), format!("record {record}"));
        }
        for op in &self.ops {
            if di_op_named(op.as_bytes()).is_none() {
                if dw_op(op.as_bytes()).is_some() {
                    unsupported.insert(format!("debug:{op}"));
                } else {
                    unknown.push(format!("operation {op}"));
                }
            }
        }
        (unsupported, unknown)
    }
}

/// A numbered metadata definition.
#[derive(Clone, Copy)]
enum Def<'a> {
    Node { distinct: bool, kind: &'a str, body: &'a str },
    Tuple(&'a str),
    Str(&'a str),
}

/// A field's value, owned, so that a node can borrow it.
enum Owned {
    UInt(u64),
    Int(i64),
    Bool(bool),
    Str(Vec<u8>),
    Node(Option<DiId>),
    List(Vec<DiId>),
    Expr(ExprId),
}

impl Owned {
    fn borrowed(&self) -> DiFieldValue<'_> {
        match self {
            Owned::UInt(value) => DiFieldValue::UInt(*value),
            Owned::Int(value) => DiFieldValue::Int(*value),
            Owned::Bool(value) => DiFieldValue::Bool(*value),
            Owned::Str(text) => DiFieldValue::Str(text),
            Owned::Node(id) => DiFieldValue::Node(*id),
            Owned::List(list) => DiFieldValue::List(list),
            Owned::Expr(id) => DiFieldValue::Expr(*id),
        }
    }
}

/// Loads the debug information of one module's text.
struct Loader<'a> {
    ir: &'a str,
    defs: BTreeMap<u32, Def<'a>>,
    m: Module,
    nodes: BTreeMap<u32, DiId>,
    locs: BTreeMap<u32, LocId>,
    /// The debug entries of each body, printed: what the two spellings
    /// of debug records must agree on.
    bodies: Vec<String>,
}

impl<'a> Loader<'a> {
    fn new(ir: &'a str) -> Loader<'a> {
        let mut defs = BTreeMap::new();
        for line in ir.lines() {
            let Some((id, rest)) = line.split_once(" = ") else { continue };
            let Some(id) = number(id) else { continue };
            let (distinct, rest) = match rest.strip_prefix("distinct ") {
                Some(rest) => (true, rest),
                None => (false, rest),
            };
            let def = if rest.starts_with("!{") {
                Def::Tuple(balanced(rest, 1))
            } else if let Some(text) = rest.strip_prefix("!\"") {
                Def::Str(text.strip_suffix('"').unwrap_or(text))
            } else if let Some(&(kind, body)) = nodes_in(rest).first() {
                Def::Node { distinct, kind, body }
            } else {
                continue;
            };
            defs.insert(id, def);
        }
        let m = Module::new(TargetSpec::aarch64_linux_gnu());
        Loader { ir, defs, m, nodes: BTreeMap::new(), locs: BTreeMap::new(), bodies: Vec::new() }
    }

    fn node(&self, text: &str) -> Res<DiId> {
        let id = number(text).and_then(|n| self.nodes.get(&n));
        id.copied().ok_or_else(|| format!("{text} is not a node"))
    }

    fn loc(&self, text: &str) -> Res<LocId> {
        let id = number(text).and_then(|n| self.locs.get(&n));
        id.copied().ok_or_else(|| format!("{text} is not a location"))
    }

    /// `type value`, as metadata wraps a constant.
    fn constant(&mut self, text: &str) -> Res<ConstId> {
        let (ty, value) = text.split_once(' ').ok_or_else(|| format!("not metadata: {text}"))?;
        let m = &mut self.m;
        let float = |value: &str| match value.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).ok().map(f64::from_bits),
            None => value.parse::<f64>().ok(),
        };
        if let Some(bits) = ty.strip_prefix('i').and_then(|bits| bits.parse::<u32>().ok()) {
            let ty = s(m.types.int(bits))?;
            return s(match (value, value.parse::<i64>(), value.parse::<u64>()) {
                ("true", ..) => m.consts.int(&m.types, ty, 1),
                ("false", ..) => m.consts.int(&m.types, ty, 0),
                (_, Ok(value), _) => m.consts.int_signed(&m.types, ty, value),
                (_, _, Ok(value)) => m.consts.int(&m.types, ty, value),
                _ => m.consts.undef(&m.types, ty),
            });
        }
        match (ty, value.strip_prefix('@'), float(value)) {
            ("ptr", Some(name), _) => {
                let sym = s(m.symbol_named(&symbol_name(name)))?;
                s(m.consts.addr(sym, 0))
            }
            ("ptr", None, _) if value == "null" => s(m.consts.null(&m.types, TypeId::PTR)),
            ("double", _, Some(value)) => s(m.consts.f64(value)),
            #[allow(clippy::cast_possible_truncation)]
            ("float", _, Some(value)) => s(m.consts.f32(value as f32)),
            _ => Err(format!("a constant this loader cannot read: {text}")),
        }
    }

    /// The node an element of a tuple or a node-valued field names;
    /// [`DiId::NONE`] for `null`. Tuples, strings and constants become
    /// nodes here (§6.2).
    fn element(&mut self, text: &str) -> Res<DiId> {
        if text == "null" {
            return Ok(DiId::NONE);
        }
        let def = number(text).and_then(|n| self.defs.get(&n).copied());
        let tuple = match def {
            Some(Def::Tuple(inner)) => Some(inner),
            _ => text.strip_prefix("!{").and_then(|inner| inner.strip_suffix('}')),
        };
        let string = match def {
            Some(Def::Str(inner)) => Some(inner),
            _ => text.strip_prefix("!\"").and_then(|inner| inner.strip_suffix('"')),
        };
        if let Some(inner) = tuple {
            let elements = self.elements(inner)?;
            return s(self.m.debug.add(&DiTuple { elements: &elements }.into()));
        }
        if let Some(inner) = string {
            return s(self.m.debug.add(&DiString { text: &unescape(inner) }.into()));
        }
        if text.starts_with('!') {
            return self.node(text);
        }
        let value = self.constant(text)?;
        s(self.m.debug.add(&DiConstant { value }.into()))
    }

    fn elements(&mut self, inner: &str) -> Res<Vec<DiId>> {
        split_top(inner).into_iter().map(|element| self.element(element)).collect()
    }

    /// The expression with these elements between its parentheses.
    fn expression(&mut self, text: &str) -> Res<ExprId> {
        let inner = text.strip_prefix("!DIExpression(").and_then(|inner| inner.strip_suffix(')'));
        let inner = inner.ok_or_else(|| format!("{text} is not an expression"))?;
        let mut elements = Vec::new();
        for word in split_top(inner) {
            let name = word.as_bytes();
            let op = di_op_named(name).map(|(code, _)| code).or_else(|| dw_op(name));
            let value = word.parse::<u64>().ok().or(op).or(dw_ate(name).map(u64::from));
            elements.push(value.ok_or_else(|| format!("expression element {word}"))?);
        }
        s(self.m.debug.expr_from_elements(&elements))
    }

    /// The value of a mapped field, from its text.
    fn field(&mut self, kind: DiKind, name: &str, text: &str) -> Res<Owned> {
        let bits = |named: &dyn Fn(&[u8]) -> Option<u32>| {
            let mut flags = text.split(" | ").map(|flag| named(flag.as_bytes()));
            let bits = flags.try_fold(0, |all, flag| flag.map(|flag| all | flag));
            bits.map(|bits| Owned::UInt(u64::from(bits))).ok_or_else(|| format!("flags {text}"))
        };
        let code = |code: Option<u32>| {
            code.map(|code| Owned::UInt(u64::from(code))).ok_or_else(|| format!("name {text}"))
        };
        let word = text.as_bytes();
        if let Some(quoted) = text.strip_prefix('"') {
            return Ok(Owned::Str(unescape(quoted.strip_suffix('"').unwrap_or(quoted))));
        }
        match (name, text) {
            (_, "true") => return Ok(Owned::Bool(true)),
            (_, "false") => return Ok(Owned::Bool(false)),
            (_, "null") => return Ok(Owned::Node(None)),
            ("emissionKind", _) => return code(EmissionKind::named(word).map(EmissionKind::code)),
            ("nameTableKind", _) => return code(NameTableKind::named(word).map(NameTableKind::code)),
            ("checksumkind", _) => return code(ChecksumKind::named(word).map(ChecksumKind::code)),
            _ => {}
        }
        if let Ok(value) = text.parse::<u64>() {
            return Ok(Owned::UInt(value));
        }
        if let Ok(value) = text.parse::<i64>() {
            return Ok(Owned::Int(value));
        }
        if text.starts_with("!DIExpression(") {
            return self.expression(text).map(Owned::Expr);
        }
        if text.starts_with("DISPFlag") {
            return bits(&|flag| SpFlags::named(flag).map(|flags| flags.0));
        }
        if text.starts_with("DIFlag") {
            return bits(&|flag| DiFlags::named(flag).map(|flags| flags.0));
        }
        if let Some(which) = text.strip_prefix("DW_VIRTUALITY_") {
            let which = ["none", "virtual", "pure_virtual"].iter().position(|name| *name == which);
            return code(which.map(|which| which as u32));
        }
        if text.starts_with("DW_") {
            let number = dw_tag(word).or(dw_ate(word)).or(dw_lang(word)).or(dw_cc(word));
            return code(number.map(u32::from));
        }
        // Metadata: the elements of a tuple where the field is a list,
        // a node otherwise.
        let is_list = DiNode::llvm_default(kind).set(name.as_bytes(), DiFieldValue::List(&[])).is_ok();
        let tuple = match number(text).and_then(|n| self.defs.get(&n).copied()) {
            Some(Def::Tuple(inner)) => Some(inner),
            _ => text.strip_prefix("!{").and_then(|inner| inner.strip_suffix('}')),
        };
        match tuple {
            Some(inner) if is_list => self.elements(inner).map(Owned::List),
            _ => self.element(text).map(|id| Owned::Node(Some(id))),
        }
    }

    fn unsupported(&mut self, construct: &str) {
        self.m.record_unsupported(construct, None);
    }

    fn define_location(&mut self, id: LocId, body: &str) -> Res<()> {
        let mut loc = DiLocation { line: 0, column: 0, scope: DiId::NONE, inlined_at: LocId::NONE };
        for (name, text) in fields(body) {
            let bad = |_| format!("DILocation.{name}: {text}");
            match name {
                "line" => loc.line = text.parse().map_err(bad)?,
                "column" => loc.column = text.parse().map_err(bad)?,
                "scope" => loc.scope = self.node(text)?,
                "inlinedAt" => loc.inlined_at = self.loc(text)?,
                _ => match di_field(b"DILocation", name.as_bytes()) {
                    Some(DiMapping::Dropped(_)) => {}
                    Some(DiMapping::Unsupported(construct)) => self.unsupported(construct),
                    _ => self.unsupported(&format!("debug:DILocation.{name}")),
                },
            }
        }
        s(self.m.debug.define_loc(id, loc))
    }

    fn define_node(&mut self, id: DiId, distinct: bool, kind_name: &str, body: &str) -> Res<()> {
        let mapped = di_kind(kind_name.as_bytes()) == Some(DiMapping::Mapped);
        let Some(kind) = DiKind::named(kind_name.as_bytes()).filter(|_| mapped) else {
            // Unsupported or unknown: `finish_debug` reports the kind.
            let node = DiOther { kind: kind_name.as_bytes() }.into();
            return s(self.m.debug.define_distinct(id, &node));
        };
        let mut values = Vec::new();
        for (name, text) in fields(body) {
            match di_field(kind_name.as_bytes(), name.as_bytes()) {
                Some(DiMapping::Mapped) => {
                    let value = self.field(kind, name, text);
                    values.push((name, value.map_err(|why| format!("{kind_name}.{name}: {why}"))?));
                }
                Some(DiMapping::Dropped(_)) => {}
                Some(DiMapping::Unsupported(construct)) => self.unsupported(construct),
                None => self.unsupported(&format!("debug:{kind_name}.{name}")),
            }
        }
        let mut node = DiNode::llvm_default(kind);
        for (name, value) in &values {
            s(node.set(name.as_bytes(), value.borrowed()))?;
        }
        if distinct != node.must_be_distinct() && node.must_be_distinct() {
            return Err(format!("{kind_name} is not distinct"));
        }
        let debug = &mut self.m.debug;
        s(if distinct { debug.define_distinct(id, &node) } else { debug.define(id, &node) })
    }

    /// Reserves every node and location, then defines them (§6.3), and
    /// lists the compile units (§6.7).
    fn metadata(&mut self) -> Res<()> {
        let defs: Vec<(u32, Def<'a>)> = self.defs.iter().map(|(&n, &def)| (n, def)).collect();
        for &(n, def) in &defs {
            let Def::Node { kind, .. } = def else { continue };
            let dropped = matches!(di_kind(kind.as_bytes()), Some(DiMapping::Dropped(_)));
            if kind == "DILocation" {
                self.locs.insert(n, s(self.m.debug.reserve_loc())?);
            } else if !dropped && kind != "DIExpression" && kind != "DIArgList" {
                self.nodes.insert(n, s(self.m.debug.reserve())?);
            }
        }
        for &(n, def) in &defs {
            let Def::Node { distinct, kind, body } = def else { continue };
            let done = if let Some(&id) = self.locs.get(&n) {
                self.define_location(id, body)
            } else if let Some(&id) = self.nodes.get(&n) {
                self.define_node(id, distinct, kind, body)
            } else {
                Ok(())
            };
            done.map_err(|why| format!("!{n}: {why}"))?;
        }
        let units = self.ir.lines().find_map(|line| line.strip_prefix("!llvm.dbg.cu = !{"));
        for unit in split_top(units.unwrap_or("}").trim_end_matches('}')) {
            let unit = self.node(unit)?;
            s(self.m.debug.add_compile_unit(unit))?;
        }
        Ok(())
    }

    /// The variables that have debug information (§6.7).
    fn globals(&mut self) -> Res<()> {
        for line in self.ir.lines().filter(|line| line.starts_with('@')) {
            let attached: Vec<u32> = dbg_of(line).collect();
            if attached.is_empty() {
                continue;
            }
            let sym = s(self.m.symbol_named(&symbol_name(&line[1..])))?;
            s(self.m.set_variable(sym, &VariableDesc::new(TypeId::I32)))?;
            for n in attached {
                let node = self.node(&format!("!{n}"))?;
                s(self.m.attach_global_expr(sym, node))?;
            }
        }
        Ok(())
    }

    /// One debug record, added to `b` (§6.6).
    fn record(&mut self, b: &mut FunctionBuilder, name: &str, args: &[&str]) -> Res<()> {
        let zero = s(self.m.consts.int(&self.m.types, TypeId::I32, 0))?;
        let zero = s(Value::constant(zero))?;
        let kind = name.rsplit(['_', '.']).next().unwrap_or(name);
        match (kind, args) {
            ("declare", &[_, var, expr, loc]) => {
                let (var, expr, loc) = (self.node(var)?, self.expression(expr)?, self.loc(loc)?);
                let slot = s(b.add_slot(8, 8))?;
                let address = s(b.slot(slot))?;
                s(b.dbg_declare(address, var, expr, loc))
            }
            ("value", &[value, var, expr, loc]) => {
                let (var, expr, loc) = (self.node(var)?, self.expression(expr)?, self.loc(loc)?);
                let list = value.strip_prefix("!DIArgList(").and_then(|list| list.strip_suffix(')'));
                match list {
                    Some(list) => {
                        let values = vec![zero; split_top(list).len()];
                        s(b.dbg_value_list(&values, var, expr, loc))
                    }
                    None if value == "!{}" => s(b.dbg_value(Value::NONE, var, expr, loc)),
                    None => s(b.dbg_value(zero, var, expr, loc)),
                }
            }
            ("assign", &[value, var, expr, _id, _address, address_expr, loc]) => {
                if value.starts_with("!DIArgList") {
                    self.unsupported("debug:#dbg_assign.DIArgList");
                }
                let (var, expr, loc) = (self.node(var)?, self.expression(expr)?, self.loc(loc)?);
                let address_expr = self.expression(address_expr)?;
                s(b.dbg_assign(zero, var, expr, zero, address_expr, loc))
            }
            ("label", &[label, loc]) => {
                let (label, loc) = (self.node(label)?, self.loc(loc)?);
                s(b.dbg_label(label, loc))
            }
            _ => {
                match di_record(name.as_bytes()) {
                    Some(DiMapping::Unsupported(construct)) => self.unsupported(construct),
                    _ => self.unsupported(&format!("debug:{name}")),
                }
                Ok(())
            }
        }
    }

    /// The functions: each one's subprogram (§6.7), its records and the
    /// locations of its instructions, verified (§6.8).
    fn functions(&mut self) -> Res<()> {
        let fn_ty = s(self.m.types.function(TypeId::VOID, &[], false))?;
        let zero = s(self.m.consts.int(&self.m.types, TypeId::I32, 0))?;
        let zero = s(Value::constant(zero))?;
        let mut lines = self.ir.lines();
        while let Some(line) = lines.next() {
            let Some(at) = line.find('@').filter(|_| line.starts_with("define ")) else { continue };
            let name = symbol_name(&line[at + 1..]);
            let subprogram = match dbg_of(line).next() {
                Some(n) => Some(self.node(&format!("!{n}"))?),
                None => None,
            };
            let sym = s(self.m.symbol_named(&name))?;
            let desc = FunctionDesc { subprogram, has_body: true, ..FunctionDesc::new(fn_ty) };
            s(self.m.set_function(sym, &desc))?;
            let mut b = s(FunctionBuilder::new(&self.m, sym, fn_ty))?;
            let entry = s(b.create_block())?;
            s(b.begin_block(entry))?;
            for line in lines.by_ref().map(str::trim).take_while(|line| *line != "}") {
                if let Some((record, args)) = record_of(line) {
                    self.record(&mut b, record, &args).map_err(|why| format!("{line}: {why}"))?;
                } else if let Some(n) = dbg_of(line).next() {
                    b.set_loc(self.loc(&format!("!{n}"))?);
                    s(b.binary(Opcode::Add, TypeId::I32, zero, zero))?;
                }
            }
            b.set_loc(LocId::NONE);
            s(b.ret(None))?;
            let body = s(b.finish())?;
            let function = String::from_utf8_lossy(&name).into_owned();
            body.verify(&self.m).map_err(|why| format!("@{function}: {why}"))?;
            let records: Vec<_> = body.dbg_records().collect();
            self.bodies.push(format!("{:?} {:?} {records:?}", body.slot_vars(), body.declares()));
        }
        Ok(())
    }

    /// Loads everything, finishes and verifies.
    fn load(ir: &'a str) -> Res<Loader<'a>> {
        let mut loader = Loader::new(ir);
        loader.metadata()?;
        loader.globals()?;
        loader.functions()?;
        s(loader.m.finish_debug())?;
        s(loader.m.verify_debug())?;
        Ok(loader)
    }

    fn unsupported_constructs(&self) -> BTreeSet<String> {
        let all = self.m.unsupported().map(|entry| entry.construct.to_string());
        all.filter(|construct| construct.starts_with("debug:")).collect()
    }
}

/// A flag set is the baseline every producer of hod's must be fully
/// supported at, or an option that may bring unsupported constructs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Baseline,
    Option,
}

/// The flag sets, after `-g -O0`, and the first major that has the flag.
const FLAG_SETS: [(&[&str], Level, u32); 20] = [
    (&[], Level::Baseline, 0),
    (&["-gdwarf-4"], Level::Baseline, 0),
    (&["--target=x86_64-unknown-linux-gnu"], Level::Baseline, 0),
    (&["-fPIC", "-fno-omit-frame-pointer"], Level::Baseline, 0),
    (&["-O1"], Level::Option, 0),
    (&["-O2"], Level::Option, 0),
    (&["-O2", "-gkey-instructions"], Level::Option, 22),
    (&["-fdebug-macro"], Level::Option, 0),
    (&["-gsplit-dwarf"], Level::Option, 0),
    (&["-gembed-source"], Level::Option, 0),
    (&["-g1"], Level::Option, 0),
    (&["-gpubnames"], Level::Option, 0),
    (&["-ggnu-pubnames"], Level::Option, 0),
    (&["-fdebug-info-for-profiling"], Level::Option, 0),
    (&["-gno-column-info"], Level::Option, 0),
    (&["-fstandalone-debug"], Level::Option, 0),
    (&["-flto"], Level::Option, 0),
    (&["-gtemplate-alias"], Level::Option, 0),
    (&["-gsimple-template-names"], Level::Option, 0),
    (&["-fdebug-types-section"], Level::Option, 0),
];

/// Everything unsupported that the option flag sets bring. A run may
/// have less; more is a table that has fallen behind its compilers.
const UNSUPPORTED_UNDER_OPTIONS: &[&str] = &[
    // -fdebug-macro
    "debug:DICompileUnit.macros",
    "debug:DIMacro",
    "debug:DIMacroFile",
    // -gsplit-dwarf
    "debug:DICompileUnit.splitDebugFilename",
];

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(name)
}

/// Compiles `source` to LLVM IR text with debug information.
fn compile(compiler: &Path, scratch: &Scratch, source: &Path, flags: &[&str]) -> Res<String> {
    let output = scratch.join("unit.ll");
    let cxx = source.extension().is_some_and(|extension| extension == "cpp");
    let std = if cxx { "-std=c++23" } else { "-std=gnu17" };
    let mut args: Vec<&OsStr> = ["-O0", "-g", "-w", "-S", "-emit-llvm", std].map(OsStr::new).into();
    args.push("--target=aarch64-unknown-linux-gnu".as_ref());
    args.extend(flags.iter().map(OsStr::new));
    args.extend(["-o".as_ref(), output.as_os_str(), source.as_os_str()]);
    let outcome = hod_testutil::run(compiler, args);
    if !outcome.success() {
        return Err(outcome.stderr_text());
    }
    std::fs::read_to_string(&output).map_err(|error| error.to_string())
}

/// Scans and loads one output. Returns what it wrote and what of it is
/// unsupported; `wrong` gets a line for everything that must not be.
fn unit(ir: &str, run: &str, wrong: &mut Vec<String>) -> (Found, BTreeSet<String>, Vec<String>) {
    let found = Found::scan(ir);
    let (unsupported, unknown) = found.classify();
    wrong.extend(unknown.iter().map(|what| format!("{what} is in no table [{run}]")));
    let mut bodies = Vec::new();
    match Loader::load(ir) {
        Err(why) => wrong.push(format!("{why} [{run}]")),
        Ok(loader) => {
            let loaded = loader.unsupported_constructs();
            if unknown.is_empty() && loaded != unsupported {
                wrong.push(format!("loading reports {loaded:?}, the text has {unsupported:?} [{run}]"));
            }
            bodies = loader.bodies;
        }
    }
    (found, unsupported, bodies)
}

/// `kiln` or `kiln++`: `$HOD_KILN`'s directory, or the kiln build beside
/// this checkout's target directory.
fn kiln(name: &str) -> Option<PathBuf> {
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join(name));
    let named = std::env::var_os("HOD_KILN")
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(path).with_file_name(name));
    let path = named.or(beside).filter(|path| path.is_file());
    if path.is_none() {
        eprintln!("note: no {name} (set HOD_KILN); skipping kiln in the debug survey");
    }
    path
}

fn names(set: &BTreeSet<String>) -> String {
    set.iter().map(String::as_str).collect::<Vec<_>>().join(" ")
}

fn field_names(set: &BTreeSet<(String, String)>) -> String {
    set.iter().map(|(kind, field)| format!("{kind}.{field}")).collect::<Vec<_>>().join(" ")
}

/// §6.9: everything clang writes is in the field table; at `-g -O0`
/// nothing of it is unsupported; and it loads and verifies (§6.3, §6.8).
#[test]
fn clang_debug_information_is_covered() {
    let mut wrong = Vec::new();
    let (mut baseline, mut options) = (BTreeMap::new(), Found::default());
    let mut under_options = BTreeSet::new();
    let mut runs = 0;
    for major in MAJORS {
        let Some(clang) = llvm_tool(major, "clang") else { continue };
        let scratch = Scratch::new(&format!("debug-survey-{major}"));
        let found: &mut Found = baseline.entry(major).or_default();
        for source in ["debug_survey.c", "debug_survey.cpp"] {
            for (flags, level, since) in FLAG_SETS {
                if major < since {
                    continue;
                }
                let run = format!("clang {major} {flags:?} {source}");
                let ir = compile(&clang, &scratch, &data(source), flags);
                let ir = ir.unwrap_or_else(|why| panic!("{run}: {why}"));
                let (wrote, unsupported, _) = unit(&ir, &run, &mut wrong);
                runs += 1;
                if level == Level::Baseline {
                    found.add(&wrote);
                    if !unsupported.is_empty() {
                        wrong.push(format!("unsupported at the baseline: {unsupported:?} [{run}]"));
                    }
                } else {
                    options.add(&wrote);
                    under_options.extend(unsupported);
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    if runs == 0 {
        return;
    }
    let extra: Vec<_> =
        under_options.iter().filter(|name| !UNSUPPORTED_UNDER_OPTIONS.contains(&name.as_str())).collect();
    assert!(extra.is_empty(), "unsupported constructs the list does not have: {extra:?}");

    // What the survey saw, for `spec/landed/ir/WP-005.md`.
    eprintln!("debug survey: {runs} runs");
    let mut all = Found::default();
    for (major, found) in &baseline {
        all.add(found);
        eprintln!("  clang {major} at -g -O0:");
        eprintln!("    kinds: {}", names(&found.kinds));
        eprintln!("    operations: {}", names(&found.ops));
        eprintln!("    records: {}", names(&found.records));
        eprintln!("    fields: {}", found.fields.len());
    }
    let majors: Vec<_> = baseline.keys().collect();
    if let [old, new] = majors[..] {
        let only = |a: u32, b: u32| {
            let fields = baseline[&a].fields.difference(&baseline[&b].fields).cloned().collect();
            eprintln!("  fields at -g -O0 of clang {a} only: {}", field_names(&fields));
        };
        only(*new, *old);
        only(*old, *new);
    }
    let more = |all: &BTreeSet<String>, base: &BTreeSet<String>| {
        names(&all.difference(base).cloned().collect())
    };
    eprintln!("  under options only:");
    eprintln!("    kinds: {}", more(&options.kinds, &all.kinds));
    eprintln!("    operations: {}", more(&options.ops, &all.ops));
    eprintln!("    records: {}", more(&options.records, &all.records));
    let fields = options.fields.difference(&all.fields).cloned().collect();
    eprintln!("    fields: {}", field_names(&fields));
    eprintln!("    unsupported: {}", names(&under_options));
    let mut seen = all.clone();
    seen.add(&options);
    let unseen: Vec<String> = hod_ir::di_field_table()
        .filter(|(_, _, mapping)| *mapping == DiMapping::Mapped)
        .filter(|(kind, field, _)| !seen.fields.contains(&((*kind).to_string(), (*field).to_string())))
        .map(|(kind, field, _)| format!("{kind}.{field}"))
        .collect();
    eprintln!("  mapped fields no run wrote: {}", unseen.join(" "));

    // The survey saw what the model was built from.
    for kind in [
        "DIBasicType",
        "DICompileUnit",
        "DICompositeType",
        "DIDerivedType",
        "DIEnumerator",
        "DIExpression",
        "DIFile",
        "DIGlobalVariable",
        "DIGlobalVariableExpression",
        "DIImportedEntity",
        "DILabel",
        "DILexicalBlock",
        "DILocalVariable",
        "DILocation",
        "DINamespace",
        "DISubprogram",
        "DISubrange",
        "DISubroutineType",
        "DITemplateTypeParameter",
        "DITemplateValueParameter",
    ] {
        assert!(all.kinds.contains(kind), "no baseline run wrote a {kind}");
    }
    for record in ["#dbg_declare", "#dbg_label"] {
        assert!(all.records.contains(record), "no baseline run wrote a {record}");
    }
    for record in ["#dbg_value", "#dbg_assign"] {
        assert!(options.records.contains(record), "no run wrote a {record}");
    }
    for kind in ["DIArgList", "DIAssignID", "DILexicalBlockFile", "DIMacro", "DIMacroFile"] {
        assert!(options.kinds.contains(kind), "no run wrote a {kind}");
    }
    for op in ["DW_OP_LLVM_arg", "DW_OP_LLVM_fragment", "DW_OP_stack_value", "DW_OP_deref"] {
        assert!(seen.ops.contains(op), "no run wrote {op}");
    }
}

/// §6.6: the intrinsic calls LLVM 19 can still write are the same
/// declarations and records as the debug records that replaced them.
#[test]
fn both_spellings_of_debug_records_are_one_model() {
    let (Some(clang), Some(opt)) = (llvm_tool(19, "clang"), llvm_tool(19, "opt")) else { return };
    let scratch = Scratch::new("debug-survey-spellings");
    let mut wrong = Vec::new();
    for (source, flags) in [
        ("debug_survey.c", &[][..]),
        ("debug_survey.cpp", &[]),
        ("debug_survey.c", &["-O2"]),
        ("debug_survey.cpp", &["-O2"]),
    ] {
        let records = compile(&clang, &scratch, &data(source), flags).unwrap();
        let input = scratch.write("records.ll", &records);
        let output = scratch.join("calls.ll");
        let args: [&OsStr; 5] = [
            "-S".as_ref(),
            "--write-experimental-debuginfo=false".as_ref(),
            "-o".as_ref(),
            output.as_os_str(),
            input.as_os_str(),
        ];
        let outcome = hod_testutil::run(&opt, args);
        assert!(outcome.success(), "{}", outcome.stderr_text());
        let calls = std::fs::read_to_string(&output).unwrap();
        let (new, _, new_bodies) = unit(&records, &format!("records {source}"), &mut wrong);
        let (old, _, old_bodies) = unit(&calls, &format!("calls {source}"), &mut wrong);
        let kind = if flags.is_empty() { "declare" } else { "value" };
        let (record, call) = (format!("#dbg_{kind}"), format!("llvm.dbg.{kind}"));
        assert!(new.records.contains(&record) && !new.records.contains(&call), "{:?}", new.records);
        assert!(old.records.contains(&call) && !old.records.contains(&record), "{:?}", old.records);
        assert!(old.records.iter().all(|name| name.starts_with("llvm.dbg.")), "{:?}", old.records);
        assert_eq!((&new.kinds, &new.fields, &new.ops), (&old.kinds, &old.fields, &old.ops));
        assert!(!new_bodies.is_empty());
        assert_eq!(new_bodies, old_bodies, "{source} {flags:?}");
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// §6.9: kiln, the producer that matters most, writes nothing that is
/// unsupported or unknown, and its debug information loads and verifies.
#[test]
fn kiln_debug_information_is_covered() {
    let mut wrong = Vec::new();
    let scratch = Scratch::new("debug-survey-kiln");
    let clang = llvm_tool(MAJORS[0], "clang");
    for (driver, source) in [("kiln", "debug_survey.c"), ("kiln++", "debug_survey.cpp")] {
        let Some(compiler) = kiln(driver) else { return };
        let run = format!("{driver} {source}");
        let ir = compile(&compiler, &scratch, &data(source), &[]);
        let ir = ir.unwrap_or_else(|why| panic!("{run}: {why}"));
        let (found, unsupported, bodies) = unit(&ir, &run, &mut wrong);
        if !unsupported.is_empty() {
            wrong.push(format!("unsupported: {unsupported:?} [{run}]"));
        }
        assert!(!bodies.is_empty() && found.kinds.contains("DISubprogram"), "{run}");
        // Against clang, for `spec/landed/ir/WP-005.md`.
        let Some(clang) = &clang else { continue };
        let reference = Found::scan(&compile(clang, &scratch, &data(source), &[]).unwrap());
        let only = |a: &Found, b: &Found| {
            let fields = a.fields.difference(&b.fields).cloned().collect();
            let kinds = a.kinds.difference(&b.kinds).cloned().collect();
            let ops = a.ops.difference(&b.ops).cloned().collect();
            let records = a.records.difference(&b.records).cloned().collect();
            format!("{} {} {} {}", names(&kinds), field_names(&fields), names(&ops), names(&records))
        };
        eprintln!("{run}: only kiln writes: {}", only(&found, &reference));
        eprintln!("{run}: only clang {} writes: {}", MAJORS[0], only(&reference, &found));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// §6.10: the tables of a translation unit that includes the standard
/// library: bytes for each node and each location.
#[test]
fn a_real_translation_unit() {
    let Some(clang) = MAJORS.into_iter().find_map(|major| llvm_tool(major, "clang++")) else {
        return;
    };
    let scratch = Scratch::new("debug-survey-stl");
    let ir = match compile(&clang, &scratch, &data("debug_survey_stl.cpp"), &[]) {
        Ok(ir) => ir,
        Err(why) => {
            assert!(!tools_required(), "the standard library does not compile: {why}");
            eprintln!("note: skipping: no C++ standard library for clang");
            return;
        }
    };
    let found = Found::scan(&ir);
    let (unsupported, unknown) = found.classify();
    assert!(unsupported.is_empty() && unknown.is_empty(), "{unsupported:?} {unknown:?}");
    let loader = Loader::load(&ir).unwrap();
    assert!(loader.unsupported_constructs().is_empty());
    let stats = loader.m.debug.stats();
    let per_node = stats.node_bytes as f64 / stats.nodes as f64;
    let text = ir.lines().filter(|line| line.starts_with('!')).map(str::len).sum::<usize>();
    eprintln!(
        "debug tables of a standard-library unit: {} nodes in {} bytes ({per_node:.1} each), \
         {} bytes of strings, {} expressions in {} bytes, {} locations in {} bytes; \
         {} functions; the metadata is {text} bytes of text",
        stats.nodes,
        stats.node_bytes,
        stats.string_bytes,
        stats.exprs,
        stats.expr_bytes,
        stats.locs,
        stats.loc_bytes,
        loader.bodies.len(),
    );
    assert!(stats.nodes > 5000, "{stats:?}");
    assert!(per_node < 48.0, "{per_node} bytes for each node");
    assert_eq!(stats.loc_bytes, stats.locs * 16);
    let total = stats.node_bytes + stats.string_bytes + stats.expr_bytes + stats.loc_bytes;
    assert!(total * 2 < text, "the tables are {total} bytes, the text {text}");
}
