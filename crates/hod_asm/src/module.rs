//! Module-level assembly (`module asm`) for AArch64 ELF (`spec/asm.md`
//! §9): the statements of the module's text read into sections, symbols
//! and relocations, as LLVM's integrated assembler (`llc`, which emits
//! the module's assembly before anything else) would write them.
//!
//! [`assemble_module`] keeps a section per name the text switches to
//! (`.text`, `.data`, `.bss`, `.section`, `.pushsection`/`.popsection`,
//! `.previous`), with its bytes, alignment, relocations and the mapping
//! changes of a code section (`$x`/`$d`); a table of the symbols the
//! text defines (labels, `.set`), references or gives a directive to; and
//! leaves nothing to guess: a directive, a section or a symbol use it
//! does not reproduce is refused by name.
//!
//! The caller (module emission, `hod_cg`) places the sections in the
//! object before anything of the module's own, and joins the symbol
//! table with the module's symbols: this module knows nothing of IR.
//!
//! References are resolved as LLVM's ELF assembler resolves them
//! (`ELFObjectWriter`): a PC-relative field to a label of local binding
//! in the same section is filled in here; one to a label of local binding
//! elsewhere, and every absolute field to such a label, is a relocation
//! against the label's section with the label's offset in the addend;
//! everything else is a relocation against the symbol. A reference to a
//! `.set` constant is that constant.

use std::collections::HashMap;

use crate::a64::{Features, RefKind, encode, operand, split_label, split_mnemonic};
use crate::expr::{Cursor, Name, Value};
use crate::reason::{self, Refusal, operand as op_reason, other};

type R<T> = Result<T, Refusal>;

fn fail<T>(what: &'static str) -> R<T> {
    Err(Refusal(what))
}

/// What a section holds, which sets its ELF type and flags.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SectionKind {
    /// Code: `SHT_PROGBITS`, `SHF_ALLOC | SHF_EXECINSTR`.
    Text,
    /// Read-only data: `SHT_PROGBITS`, `SHF_ALLOC`.
    ReadOnly,
    /// Writable data: `SHT_PROGBITS`, `SHF_ALLOC | SHF_WRITE`.
    Data,
    /// Zero-initialized writable data: `SHT_NOBITS`,
    /// `SHF_ALLOC | SHF_WRITE`.
    Bss,
}

impl SectionKind {
    /// The kind LLVM's assembler gives a section of this name
    /// (`ELFAsmParser`'s defaults), for the names this crate takes:
    /// `.text`, `.rodata`, `.data`, `.bss`, each alone or followed by
    /// `.` and more. The mergeable `.rodata.str*`/`.rodata.cst*` and the
    /// thread-local, array and note sections are not taken.
    pub fn of_name(name: &str) -> Option<SectionKind> {
        let has = |prefix: &str| {
            name.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        };
        if name.starts_with(".rodata.str") || name.starts_with(".rodata.cst") {
            return None;
        }
        if has(".text") {
            Some(SectionKind::Text)
        } else if has(".rodata") {
            Some(SectionKind::ReadOnly)
        } else if has(".data") {
            Some(SectionKind::Data)
        } else if has(".bss") {
            Some(SectionKind::Bss)
        } else {
            None
        }
    }

    /// The flags a `.section` directive may name: `a`, `w`, `x`, as
    /// many as the kind has (LLVM's assembler adds them to the name's
    /// defaults).
    fn allows_flag(self, flag: u8) -> bool {
        match flag {
            b'a' => true,
            b'w' => matches!(self, SectionKind::Data | SectionKind::Bss),
            b'x' => self == SectionKind::Text,
            _ => false,
        }
    }

    /// Without file space.
    pub fn is_nobits(self) -> bool {
        self == SectionKind::Bss
    }
}

/// A mapping state of a code section.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mapping {
    /// Instructions follow: `$x`.
    Code,
    /// Data follows: `$d`.
    Data,
}

/// The field a relocation fills.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RelocKind {
    /// An instruction's PC-relative field.
    Code(RefKind),
    /// `.quad`: `R_AARCH64_ABS64`.
    Abs64,
    /// `.word`: `R_AARCH64_ABS32`.
    Abs32,
    /// `.hword`: `R_AARCH64_ABS16`.
    Abs16,
}

/// What a relocation is against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// The symbol of this index in [`ModuleCode::symbols`].
    Symbol(u32),
    /// The section of this index in [`ModuleCode::sections`] (its
    /// section symbol); the label's offset is in the addend.
    Section(u32),
}

/// One relocation of a section.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reloc {
    /// The byte offset of the field in the section.
    pub offset: u64,
    /// The field.
    pub kind: RelocKind,
    /// The target.
    pub target: Target,
    /// The addend.
    pub addend: i64,
}

/// One section the text writes to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    /// The name.
    pub name: String,
    /// The kind.
    pub kind: SectionKind,
    /// The alignment the text asks for (at least 4 for code, as LLVM
    /// 22's assembler gives every code section; LLVM 19 gives 1).
    pub align: u64,
    /// The contents (empty for [`SectionKind::Bss`]).
    pub bytes: Vec<u8>,
    /// The size (the contents' length, or the space of a `.bss`).
    pub size: u64,
    /// The relocations, in order.
    pub relocs: Vec<Reloc>,
    /// For code: each change of mapping state, at its offset, in order.
    pub mapping: Vec<(u64, Mapping)>,
}

/// A symbol binding a directive gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Binding {
    /// `.globl`, `.global`.
    Global,
    /// `.weak`.
    Weak,
}

/// A symbol visibility a directive gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    /// `.hidden`.
    Hidden,
    /// `.protected`.
    Protected,
}

/// A symbol type `.type` gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolType {
    /// `@function`.
    Function,
    /// `@object`.
    Object,
    /// `@notype`.
    NoType,
}

/// Where a symbol is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Def {
    /// Not defined by the text.
    Undefined,
    /// A label: the section's index and the offset.
    Label(u32, u64),
    /// `.set name, value` / `name = value`: an absolute value.
    Absolute(i64),
}

/// A symbol the text defines, references or names in a directive.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Symbol {
    /// The name.
    pub name: String,
    /// Its definition.
    pub def: Def,
    /// The binding a directive gives; without one a defined symbol is
    /// local and an undefined one global.
    pub binding: Option<Binding>,
    /// The last visibility a directive gives.
    pub visibility: Option<Visibility>,
    /// The type `.type` gives.
    pub ty: Option<SymbolType>,
    /// The size `.size` gives.
    pub size: Option<u64>,
    /// Whether a relocation is against it.
    pub referenced: bool,
}

impl Symbol {
    /// An assembler-local name (`.L...`): never in the symbol table.
    pub fn is_temporary(&self) -> bool {
        self.name.starts_with(".L")
    }

    /// Whether its binding is local: defined, and given no binding.
    pub fn is_local(&self) -> bool {
        self.binding.is_none() && self.def != Def::Undefined
    }
}

/// What module-level assembly puts in the object.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct ModuleCode {
    /// The sections, in the order the text first switches to them
    /// (`.text` first: the text starts there).
    pub sections: Vec<Section>,
    /// The symbols, in the order the text first names them.
    pub symbols: Vec<Symbol>,
}

impl ModuleCode {
    /// Whether the text writes nothing but symbol effects: no byte, no
    /// alignment beyond `.text`'s own, no label.
    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(|s| s.size == 0 && s.relocs.is_empty())
            && self.symbols.iter().all(|s| s.def == Def::Undefined)
    }
}

/// A numeric label's definition.
struct LocalDef {
    n: u32,
    seq: u32,
    section: u32,
    offset: u64,
}

/// A field to fill in once every label is known.
enum Fill {
    /// An instruction's field to a numeric label.
    Local { n: u32, forward: bool, kind: RefKind },
    /// A field to a named symbol.
    Named { sym: u32, kind: RelocKind },
}

struct Pending {
    seq: u32,
    section: u32,
    offset: u64,
    addend: i64,
    fill: Fill,
}

struct State<'t> {
    out: &'t mut ModuleCode,
    features: Features,
    names: HashMap<String, u32>,
    current: u32,
    previous: Option<u32>,
    stack: Vec<(u32, Option<u32>)>,
    locals: Vec<LocalDef>,
    pending: Vec<Pending>,
    /// The number of the statement being read.
    seq: u32,
}

/// Assembles module-level assembly `text` for AArch64 into `out`
/// (cleared first). `features` are the target's (the module's
/// assembly is assembled with the target's features, not a function's).
///
/// # Errors
/// The `asm:*` refusal of the first statement, operand or symbol use
/// that is not reproduced (`spec/asm.md` §9).
pub fn assemble_module(text: &str, features: Features, out: &mut ModuleCode) -> R<()> {
    out.sections.clear();
    out.symbols.clear();
    let mut cleaned = String::new();
    crate::text::clean(text, &mut cleaned)?;
    let mut st = State {
        out,
        features,
        names: HashMap::new(),
        current: 0,
        previous: None,
        stack: Vec::new(),
        locals: Vec::new(),
        pending: Vec::new(),
        seq: 0,
    };
    st.out.sections.push(new_section(".text", SectionKind::Text));
    for (seq, stmt) in crate::text::statements(&cleaned).enumerate() {
        st.seq = u32::try_from(seq).map_err(|_| Refusal(op_reason::REACH))?;
        st.statement(stmt)?;
    }
    st.resolve()
}

fn new_section(name: &str, kind: SectionKind) -> Section {
    Section {
        name: name.to_owned(),
        kind,
        align: if kind == SectionKind::Text { 4 } else { 1 },
        bytes: Vec::new(),
        size: 0,
        relocs: Vec::new(),
        mapping: Vec::new(),
    }
}

/// `.size` of the form `.-name` (white space allowed).
fn dot_minus(text: &str) -> Option<&str> {
    let rest = crate::text::trim(text).strip_prefix('.')?;
    let rest = crate::text::trim(rest).strip_prefix('-')?;
    Some(crate::text::trim(rest))
}

/// Splits operand text at top-level commas (outside strings).
fn split_commas(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => i += if b.get(i + 1) == Some(&b'\\') { 3 } else { 2 },
            b',' => {
                parts.push(crate::text::trim(&text[start..i]));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(crate::text::trim(&text[start.min(text.len())..]));
    parts
}

/// A plain symbol name: `[A-Za-z_.$][A-Za-z0-9_.$]*`, not `.`.
fn plain_name(name: &str) -> R<&str> {
    let mut c = Cursor::new(name);
    match c.name() {
        Some(n) if n != "." && c.at_end() => Ok(n),
        _ => fail(op_reason::SYNTAX),
    }
}

/// The bytes of a string literal `"..."` with LLVM's escapes (`\b`
/// `\f` `\n` `\r` `\t` `\"` `\\`, up to three octal digits, `\x` and
/// any number of hex digits, truncated to a byte), appended to `out`.
fn string_into(text: &str, out: &mut Vec<u8>) -> R<()> {
    let inner = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .filter(|_| text.len() >= 2)
        .ok_or(Refusal(op_reason::SYNTAX))?;
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&e) = b.get(i) else { return fail(op_reason::SYNTAX) };
        i += 1;
        match e {
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'x' | b'X' => {
                let mut v: u32 = 0;
                let mut digits = 0;
                while let Some(d) = b.get(i).and_then(|&c| char::from(c).to_digit(16)) {
                    v = (v.wrapping_mul(16) + d) & 0xffff;
                    i += 1;
                    digits += 1;
                }
                if digits == 0 {
                    return fail(op_reason::SYNTAX);
                }
                out.push(v as u8);
            }
            b'0'..=b'7' => {
                let mut v = u32::from(e - b'0');
                let mut digits = 1;
                while digits < 3 {
                    match b.get(i) {
                        Some(&d @ b'0'..=b'7') => {
                            v = v * 8 + u32::from(d - b'0');
                            i += 1;
                            digits += 1;
                        }
                        _ => break,
                    }
                }
                if v > 255 {
                    return fail(op_reason::RANGE);
                }
                out.push(v as u8);
            }
            _ => return fail(op_reason::SYNTAX),
        }
    }
    Ok(())
}

impl State<'_> {
    fn section(&mut self) -> &mut Section {
        &mut self.out.sections[self.current as usize]
    }

    fn switch_to(&mut self, index: u32) {
        if index != self.current {
            self.previous = Some(self.current);
            self.current = index;
        }
    }

    /// The index of the section `name`, created with `kind` if new.
    fn section_named(&mut self, name: &str, kind: SectionKind) -> u32 {
        if let Some(i) = self.out.sections.iter().position(|s| s.name == name) {
            return i as u32;
        }
        self.out.sections.push(new_section(name, kind));
        (self.out.sections.len() - 1) as u32
    }

    /// The symbol `name`, added if new.
    fn symbol(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.names.get(name) {
            return i;
        }
        let i = self.out.symbols.len() as u32;
        self.out.symbols.push(Symbol {
            name: name.to_owned(),
            def: Def::Undefined,
            binding: None,
            visibility: None,
            ty: None,
            size: None,
            referenced: false,
        });
        self.names.insert(name.to_owned(), i);
        i
    }

    fn mark(&mut self, to: Mapping) {
        let s = self.section();
        if s.kind != SectionKind::Text {
            return;
        }
        if s.mapping.last().map(|&(_, m)| m) != Some(to) {
            let at = s.size;
            s.mapping.push((at, to));
        }
    }

    /// Appends data bytes to the current section.
    fn data(&mut self, bytes: &[u8]) -> R<()> {
        self.mark(Mapping::Data);
        let s = self.section();
        if s.kind.is_nobits() {
            if bytes.iter().any(|&b| b != 0) {
                return fail(other::SECTION);
            }
        } else {
            s.bytes.extend_from_slice(bytes);
        }
        s.size += bytes.len() as u64;
        Ok(())
    }

    fn statement(&mut self, stmt: &str) -> R<()> {
        let mut stmt = stmt;
        while let Some((label, rest)) = split_label(stmt) {
            self.label(label)?;
            stmt = rest;
        }
        if stmt.is_empty() {
            return Ok(());
        }
        let (head, rest) = split_mnemonic(stmt);
        // `name = value`.
        if let Some(value) = rest.strip_prefix('=') {
            return self.set(head, value);
        }
        if head.contains('=') {
            let (name, value) = stmt.split_once('=').unwrap_or((stmt, ""));
            return self.set(crate::text::trim(name), value);
        }
        if head.starts_with('.') {
            return self.directive(head, rest);
        }
        self.instruction(head, rest)
    }

    fn label(&mut self, label: &str) -> R<()> {
        let offset = self.section().size;
        if let Ok(n) = label.parse::<u32>() {
            self.locals.push(LocalDef { n, seq: self.seq, section: self.current, offset });
            return Ok(());
        }
        let i = self.symbol(label);
        let sym = &mut self.out.symbols[i as usize];
        if sym.def != Def::Undefined {
            return fail(other::LABEL);
        }
        sym.def = Def::Label(self.current, offset);
        Ok(())
    }

    fn set(&mut self, name: &str, value: &str) -> R<()> {
        let name = plain_name(name)?;
        let mut c = Cursor::new(value);
        let v = c.constant()?;
        if !c.at_end() {
            return fail(op_reason::SYNTAX);
        }
        let i = self.symbol(name);
        let sym = &mut self.out.symbols[i as usize];
        if sym.def != Def::Undefined
            || self.pending.iter().any(|p| matches!(p.fill, Fill::Named { sym, .. } if sym == i))
        {
            return fail(other::LABEL);
        }
        sym.def = Def::Absolute(v);
        Ok(())
    }

    fn instruction(&mut self, mnemonic: &str, rest: &str) -> R<()> {
        if self.section().kind != SectionKind::Text {
            return fail(other::SECTION);
        }
        if self.section().size % 4 != 0 {
            return fail(other::UNALIGNED);
        }
        let ops = operand::parse(rest)?;
        let e = encode::encode(mnemonic, ops.as_slice(), self.features)?;
        self.mark(Mapping::Code);
        let offset = self.section().size;
        if let Some((kind, to)) = e.fixup {
            match to {
                Value { name: Some(Name::Local(n, forward)), constant: 0 } => {
                    self.pending.push(Pending {
                        seq: self.seq,
                        section: self.current,
                        offset,
                        addend: 0,
                        fill: Fill::Local { n, forward, kind },
                    });
                }
                Value { name: Some(Name::Local(..)), .. } => return fail(op_reason::EXPRESSION),
                Value { name: Some(Name::Sym(s)), constant } => {
                    let sym = self.symbol(plain_name(s)?);
                    self.pending.push(Pending {
                        seq: self.seq,
                        section: self.current,
                        offset,
                        addend: constant,
                        fill: Fill::Named { sym, kind: RelocKind::Code(kind) },
                    });
                }
                Value { name: None, .. } => return fail(op_reason::SYMBOL),
            }
        }
        let s = self.section();
        s.bytes.extend_from_slice(&e.word.to_le_bytes());
        s.size += 4;
        Ok(())
    }

    fn directive(&mut self, name: &str, rest: &str) -> R<()> {
        match name {
            ".text" => self.simple_section(".text", SectionKind::Text, rest),
            ".data" => self.simple_section(".data", SectionKind::Data, rest),
            ".bss" => self.simple_section(".bss", SectionKind::Bss, rest),
            ".section" => {
                let index = self.section_directive(rest)?;
                self.switch_to(index);
                Ok(())
            }
            ".pushsection" => {
                let index = self.section_directive(rest)?;
                self.stack.push((self.current, self.previous));
                self.switch_to(index);
                Ok(())
            }
            ".popsection" => {
                if !rest.is_empty() {
                    return fail(op_reason::FORM);
                }
                let Some((current, previous)) = self.stack.pop() else {
                    return fail(other::SECTION);
                };
                self.current = current;
                self.previous = previous;
                Ok(())
            }
            ".previous" => {
                if !rest.is_empty() {
                    return fail(op_reason::FORM);
                }
                if let Some(previous) = self.previous {
                    self.previous = Some(self.current);
                    self.current = previous;
                }
                Ok(())
            }
            ".globl" | ".global" => self.each_name(rest, |s| s.bind(Binding::Global)),
            ".weak" => self.each_name(rest, |s| s.bind(Binding::Weak)),
            ".hidden" => self.each_name(rest, |s| {
                s.visibility = Some(Visibility::Hidden);
                Ok(())
            }),
            ".protected" => self.each_name(rest, |s| {
                s.visibility = Some(Visibility::Protected);
                Ok(())
            }),
            ".type" => self.type_directive(rest),
            ".size" => self.size_directive(rest),
            ".set" | ".equ" => {
                let Some((name, value)) = rest.split_once(',') else {
                    return fail(op_reason::FORM);
                };
                self.set(crate::text::trim(name), value)
            }
            ".byte" => self.values(rest, 1),
            ".hword" | ".short" | ".2byte" => self.values(rest, 2),
            ".word" | ".long" | ".int" | ".4byte" => self.values(rest, 4),
            ".quad" | ".xword" | ".8byte" => self.values(rest, 8),
            ".ascii" => self.strings(rest, false),
            ".asciz" | ".string" => self.strings(rest, true),
            ".zero" => {
                let mut c = Cursor::new(rest);
                let n = c.constant()?;
                if !c.at_end() {
                    return fail(op_reason::SYNTAX);
                }
                self.fill(n, 0)
            }
            ".space" | ".skip" => {
                let parts = split_commas(rest);
                let (n, fill) = match parts.as_slice() {
                    [n] => (constant(n)?, 0),
                    [n, f] => (constant(n)?, constant(f)?),
                    _ => return fail(op_reason::FORM),
                };
                self.fill(n, fill as u8)
            }
            ".p2align" | ".align" => self.align(rest, true),
            ".balign" => self.align(rest, false),
            _ => Err(reason::directive(name)),
        }
    }

    fn simple_section(&mut self, name: &str, kind: SectionKind, rest: &str) -> R<()> {
        if !rest.is_empty() {
            // A subsection number.
            return fail(other::SECTION);
        }
        let index = self.section_named(name, kind);
        self.switch_to(index);
        Ok(())
    }

    /// `name[, "flags"[, @type]]`: the section's index.
    fn section_directive(&mut self, rest: &str) -> R<u32> {
        let parts = split_commas(rest);
        let name = parts.first().copied().unwrap_or("");
        let name = name.strip_prefix('"').and_then(|n| n.strip_suffix('"')).unwrap_or(name);
        let name = plain_name(name).map_err(|_| Refusal(other::SECTION))?;
        let Some(kind) = SectionKind::of_name(name) else { return fail(other::SECTION) };
        if parts.len() > 3 {
            return fail(other::SECTION);
        }
        if let Some(flags) = parts.get(1) {
            let flags = flags
                .strip_prefix('"')
                .and_then(|f| f.strip_suffix('"'))
                .ok_or(Refusal(other::SECTION))?;
            if !flags.bytes().all(|f| kind.allows_flag(f)) {
                return fail(other::SECTION);
            }
        }
        if let Some(ty) = parts.get(2) {
            let ty = ty.strip_prefix(['@', '%']).ok_or(Refusal(other::SECTION))?;
            let nobits = match ty {
                "progbits" => false,
                "nobits" => true,
                _ => return fail(other::SECTION),
            };
            if nobits != kind.is_nobits() {
                return fail(other::SECTION);
            }
        }
        Ok(self.section_named(name, kind))
    }

    fn each_name(&mut self, rest: &str, mut f: impl FnMut(&mut Symbol) -> R<()>) -> R<()> {
        if rest.is_empty() {
            return fail(op_reason::FORM);
        }
        for part in split_commas(rest) {
            let i = self.symbol(plain_name(part)?);
            f(&mut self.out.symbols[i as usize])?;
        }
        Ok(())
    }

    fn type_directive(&mut self, rest: &str) -> R<()> {
        let parts = split_commas(rest);
        let [name, ty] = parts.as_slice() else { return fail(op_reason::FORM) };
        let ty = match *ty {
            "@function" | "%function" | "STT_FUNC" => SymbolType::Function,
            "@object" | "%object" | "STT_OBJECT" => SymbolType::Object,
            "@notype" | "%notype" | "STT_NOTYPE" => SymbolType::NoType,
            _ => return fail(other::SYMBOL),
        };
        let i = self.symbol(plain_name(name)?);
        let sym = &mut self.out.symbols[i as usize];
        // LLVM merges two types by a precedence of its own.
        if sym.ty.is_some_and(|t| t != ty) {
            return fail(other::SYMBOL);
        }
        sym.ty = Some(ty);
        Ok(())
    }

    /// `.size name, .-name` (at a point in the label's section after it)
    /// or `.size name, <constant>`.
    fn size_directive(&mut self, rest: &str) -> R<()> {
        let parts = split_commas(rest);
        let [name, value] = parts.as_slice() else { return fail(op_reason::FORM) };
        let name = plain_name(name)?;
        let size = if let Some(other) = dot_minus(value) {
            if plain_name(other)? != name {
                return fail(op_reason::EXPRESSION);
            }
            let Some(&i) = self.names.get(name) else { return fail(other::SYMBOL) };
            match self.out.symbols[i as usize].def {
                Def::Label(section, offset) if section == self.current => {
                    self.section().size - offset
                }
                _ => return fail(other::SYMBOL),
            }
        } else {
            u64::try_from(constant(value)?).map_err(|_| Refusal(op_reason::RANGE))?
        };
        let i = self.symbol(name);
        let sym = &mut self.out.symbols[i as usize];
        if sym.size.is_some() {
            return fail(other::SYMBOL);
        }
        sym.size = Some(size);
        Ok(())
    }

    fn values(&mut self, rest: &str, size: u8) -> R<()> {
        if rest.is_empty() {
            return fail(op_reason::FORM);
        }
        for part in split_commas(rest) {
            let mut c = Cursor::new(part);
            let v = c.expr()?;
            if !c.at_end() {
                return fail(op_reason::SYNTAX);
            }
            // A name `.set` already gave a value is that value, as LLVM's
            // assembler folds it.
            let folded = match v.name {
                Some(Name::Sym(s)) => self
                    .names
                    .get(plain_name(s)?)
                    .and_then(|&i| match self.out.symbols[i as usize].def {
                        Def::Absolute(k) => Some(k),
                        _ => None,
                    })
                    .map(|k| k.checked_add(v.constant).ok_or(Refusal(op_reason::RANGE)))
                    .transpose()?,
                _ => None,
            };
            let v = match folded {
                Some(constant) => Value { name: None, constant },
                None => v,
            };
            match v.name {
                None => {
                    let bits = u32::from(size) * 8;
                    let fits = bits == 64
                        || (v.constant >= -(1i64 << (bits - 1)) && v.constant < (1i64 << bits));
                    if !fits {
                        return fail(op_reason::RANGE);
                    }
                    let bytes = v.constant.to_le_bytes();
                    self.data(&bytes[..usize::from(size)])?;
                }
                Some(Name::Local(..)) => return fail(op_reason::LABEL),
                Some(Name::Sym(s)) => {
                    let kind = match size {
                        8 => RelocKind::Abs64,
                        4 => RelocKind::Abs32,
                        2 => RelocKind::Abs16,
                        _ => return fail(op_reason::SYMBOL),
                    };
                    if self.section().kind.is_nobits() {
                        return fail(other::SECTION);
                    }
                    let sym = self.symbol(plain_name(s)?);
                    self.mark(Mapping::Data);
                    let offset = self.section().size;
                    self.pending.push(Pending {
                        seq: self.seq,
                        section: self.current,
                        offset,
                        addend: v.constant,
                        fill: Fill::Named { sym, kind },
                    });
                    self.data(&[0; 8][..usize::from(size)])?;
                }
            }
        }
        Ok(())
    }

    fn strings(&mut self, rest: &str, nul: bool) -> R<()> {
        if rest.is_empty() {
            return fail(op_reason::FORM);
        }
        let mut bytes = Vec::new();
        for part in split_commas(rest) {
            bytes.clear();
            string_into(part, &mut bytes)?;
            if nul {
                bytes.push(0);
            }
            self.data(&bytes)?;
        }
        Ok(())
    }

    fn fill(&mut self, n: i64, value: u8) -> R<()> {
        // LLVM warns and writes nothing for a negative count; hod
        // refuses.
        let n =
            usize::try_from(n).ok().filter(|&n| n <= 1 << 24).ok_or(Refusal(op_reason::RANGE))?;
        self.data(&vec![value; n])
    }

    /// `.p2align`/`.align` (`log2` true: a power of two, as AArch64 ELF
    /// reads `.align`) or `.balign`, with an optional fill and no
    /// maximum. Code is padded as LLVM's AArch64 assembler pads it
    /// (zeros to a multiple of 4, then `nop`s), unless a fill is given;
    /// data with zeros. No mapping symbol marks the padding.
    fn align(&mut self, rest: &str, log2: bool) -> R<()> {
        let parts = split_commas(rest);
        let (n, fill) = match parts.as_slice() {
            [n] => (constant(n)?, None),
            [n, f] => (constant(n)?, Some(constant(f)?)),
            _ => return fail(op_reason::FORM),
        };
        let align = if log2 {
            if !(0..=16).contains(&n) {
                return fail(op_reason::RANGE);
            }
            1u64 << n
        } else {
            match u64::try_from(n) {
                Ok(0) => 1,
                Ok(a) if a.is_power_of_two() && a <= 1 << 16 => a,
                _ => return fail(op_reason::RANGE),
            }
        };
        if let Some(f) = fill
            && !(-128..256).contains(&f)
        {
            return fail(op_reason::RANGE);
        }
        let s = self.section();
        s.align = s.align.max(align);
        let pad = (s.size.wrapping_neg() & (align - 1)) as usize;
        if s.kind.is_nobits() {
            if fill.is_some_and(|f| f != 0) {
                return fail(other::SECTION);
            }
        } else {
            match fill {
                Some(f) => s.bytes.extend(std::iter::repeat_n(f as u8, pad)),
                None if s.kind == SectionKind::Text => {
                    s.bytes.extend(std::iter::repeat_n(0, pad % 4));
                    for _ in 0..pad / 4 {
                        s.bytes.extend_from_slice(&0xd503_201f_u32.to_le_bytes());
                    }
                }
                None => s.bytes.extend(std::iter::repeat_n(0, pad)),
            }
        }
        s.size += pad as u64;
        Ok(())
    }

    /// Fills or records every reference, once every label is known.
    fn resolve(mut self) -> R<()> {
        for p in std::mem::take(&mut self.pending) {
            match p.fill {
                Fill::Local { n, forward, kind } => {
                    let def = if forward {
                        self.locals.iter().find(|d| d.n == n && d.seq > p.seq)
                    } else {
                        self.locals.iter().rev().find(|d| d.n == n && d.seq <= p.seq)
                    };
                    let Some(def) = def else { return fail(op_reason::LABEL) };
                    if def.section != p.section {
                        return fail(op_reason::LABEL);
                    }
                    let disp = def.offset as i64 - p.offset as i64;
                    self.patch(p.section, p.offset, kind, disp)?;
                }
                Fill::Named { sym, kind } => self.named(&p, sym, kind)?,
            }
        }
        // A `.L` name must be defined; `.size` and `.type` need a
        // definition of the text's or of the caller's.
        for s in &self.out.symbols {
            if s.is_temporary() && s.def == Def::Undefined {
                return fail(op_reason::SYMBOL);
            }
            if s.size.is_some() && !matches!(s.def, Def::Label(..)) {
                return fail(other::SYMBOL);
            }
            if matches!(s.def, Def::Absolute(_)) && (s.ty.is_some() || s.visibility.is_some()) {
                return fail(other::SYMBOL);
            }
        }
        Ok(())
    }

    fn patch(&mut self, section: u32, offset: u64, kind: RefKind, disp: i64) -> R<()> {
        let bytes = &mut self.out.sections[section as usize].bytes;
        let at = offset as usize;
        let word = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let word = kind.fixup().patch(word, disp).ok_or(Refusal(op_reason::REACH))?;
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        Ok(())
    }

    fn named(&mut self, p: &Pending, sym: u32, kind: RelocKind) -> R<()> {
        let s = &self.out.symbols[sym as usize];
        let reloc = |target, addend| Reloc { offset: p.offset, kind, target, addend };
        let r = match (s.def, kind) {
            (Def::Absolute(_), RelocKind::Code(_)) => return fail(op_reason::SYMBOL),
            (Def::Absolute(v), _) => {
                let size = match kind {
                    RelocKind::Abs64 => 8,
                    RelocKind::Abs32 => 4,
                    _ => 2,
                };
                let value = v.wrapping_add(p.addend);
                let bits = size * 8;
                if bits < 64 && !(value >= -(1i64 << (bits - 1)) && value < (1i64 << bits)) {
                    return fail(op_reason::RANGE);
                }
                let at = p.offset as usize;
                self.out.sections[p.section as usize].bytes[at..at + size]
                    .copy_from_slice(&value.to_le_bytes()[..size]);
                return Ok(());
            }
            (Def::Label(section, offset), RelocKind::Code(code)) if s.is_local() => {
                if section == p.section {
                    let disp = (offset as i64).wrapping_add(p.addend) - p.offset as i64;
                    return self.patch(p.section, p.offset, code, disp);
                }
                reloc(Target::Section(section), (offset as i64).wrapping_add(p.addend))
            }
            (Def::Label(section, offset), _) if s.is_local() => {
                reloc(Target::Section(section), (offset as i64).wrapping_add(p.addend))
            }
            _ => reloc(Target::Symbol(sym), p.addend),
        };
        if let Target::Symbol(i) = r.target {
            self.out.symbols[i as usize].referenced = true;
        }
        self.out.sections[p.section as usize].relocs.push(r);
        Ok(())
    }
}

impl Symbol {
    fn bind(&mut self, binding: Binding) -> R<()> {
        // `.globl` with `.weak` makes LLVM warn and keep the last.
        if self.binding.is_some_and(|b| b != binding) {
            return fail(other::SYMBOL);
        }
        self.binding = Some(binding);
        Ok(())
    }
}

fn constant(text: &str) -> R<i64> {
    let mut c = Cursor::new(text);
    let v = c.constant()?;
    if !c.at_end() {
        return fail(op_reason::SYNTAX);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asm(text: &str) -> R<ModuleCode> {
        let mut out = ModuleCode::default();
        assemble_module(text, Features::BASE, &mut out).map(|()| out)
    }

    /// `spec/asm.md` §9: sections, labels, resolution.
    #[test]
    fn sections_and_symbols() {
        let m = asm(".globl f\nf: b f\n b l\nl: ret\n.pushsection .rodata\nd: .long 42\n.quad l\n.popsection\n nop").unwrap();
        assert_eq!(m.sections.len(), 2);
        let text = &m.sections[0];
        assert_eq!(text.size, 16);
        // `b f` keeps its relocation (global); `b l` is resolved.
        assert_eq!(&text.bytes[4..8], &0x1400_0001_u32.to_le_bytes());
        assert_eq!(text.relocs[0].target, Target::Symbol(0));
        let ro = &m.sections[1];
        assert_eq!(ro.name, ".rodata");
        assert_eq!(&ro.bytes[..4], &42u32.to_le_bytes());
        assert_eq!(
            ro.relocs,
            [Reloc { offset: 4, kind: RelocKind::Abs64, target: Target::Section(0), addend: 8 }]
        );
        assert_eq!(m.symbols[0].binding, Some(Binding::Global));
        assert!(m.symbols[1].is_local());
    }

    #[test]
    fn refusals() {
        let r = |t: &str| asm(t).err().map(|r| r.0);
        assert_eq!(r(".symver a, a@V1"), Some("asm:directive/symver"));
        assert_eq!(r(".section .init_array"), Some(other::SECTION));
        assert_eq!(r(".section .text,\"axG\""), Some(other::SECTION));
        assert_eq!(r(".data\n nop"), Some(other::SECTION));
        assert_eq!(r(".byte 1\n nop"), Some(other::UNALIGNED));
        assert_eq!(r("f:\nf:"), Some(other::LABEL));
        assert_eq!(r(".globl x\n.weak x"), Some(other::SYMBOL));
        assert_eq!(r(".byte 256"), Some(op_reason::RANGE));
        assert_eq!(r(".byte x"), Some(op_reason::SYMBOL));
        assert_eq!(r("b .Lnowhere"), Some(op_reason::SYMBOL));
        assert_eq!(r(".size u, 4"), Some(other::SYMBOL));
        assert_eq!(r(".popsection"), Some(other::SECTION));
        assert_eq!(r(".quad ."), Some(op_reason::EXPRESSION));
    }

    #[test]
    fn strings() {
        let mut out = Vec::new();
        string_into(r#""a\n\x41\101\\""#, &mut out).unwrap();
        assert_eq!(out, b"a\nAA\\");
        assert!(string_into(r#""\q""#, &mut Vec::new()).is_err());
    }

    #[test]
    fn directives_only_is_empty() {
        assert!(asm(".globl u\n.hidden u").unwrap().is_empty());
        assert!(!asm("nop").unwrap().is_empty());
    }
}
