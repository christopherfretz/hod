//! Assembly text that needs no assembler (`spec/ir.md` §2.3): the
//! inline-assembly templates that assemble to nothing, and module-level
//! assembly made only of symbol directives, read into the symbol-table
//! effects `llc`'s assembler gives them.
//!
//! Everything here reads text and allocates nothing but the list of
//! effects it returns.

use crate::ids::SymId;
use crate::module::Module;
use crate::symbol::{Linkage, SymbolKind, TlsModel, Visibility};
use crate::target::Arch;

// ---- the lexical layer ----

/// Whether `byte` is white space for the assembler.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\x0b' | b'\x0c')
}

/// One statement of assembly text at a time, comments removed: the
/// statement separators are a newline and `;`; a comment is `/* ... */`
/// anywhere, `//` to the end of the line (both targets: `llvm-mc` 19
/// and 22 read `nop // x` as `nop` on x86-64 too, WP-144), and `#` to
/// the end of the line at the start of a statement (both targets) or
/// anywhere (x86-64), as LLVM's assembly lexer reads them.
struct Statements<'a> {
    text: &'a [u8],
    at: usize,
    arch: Arch,
}

/// A statement the lexer could not read: an unterminated block comment.
#[derive(Clone, Copy, Debug)]
struct Unterminated;

impl<'a> Statements<'a> {
    fn new(arch: Arch, text: &'a [u8]) -> Self {
        Statements { text, at: 0, arch }
    }

    /// The next statement, trimmed, without its comments; `None` at the
    /// end. A block comment after the first word of a statement leaves
    /// it unreadable (`Err`): no input of interest has one there.
    fn next_statement(&mut self) -> Option<Result<&'a [u8], Unterminated>> {
        let t = self.text;
        if self.at >= t.len() {
            return None;
        }
        let mut i = self.at;
        loop {
            while i < t.len() && is_space(t[i]) {
                i += 1;
            }
            if !t[i..].starts_with(b"/*") {
                break;
            }
            let Some(close) = find(&t[i + 2..], b"*/") else {
                self.at = t.len();
                return Some(Err(Unterminated));
            };
            i += 2 + close + 2;
        }
        let start = i;
        let arch = self.arch;
        let line_comment = |at: usize, first: bool| {
            let rest = &t[at..];
            match arch {
                Arch::Aarch64 => rest.starts_with(b"//") || (first && rest.starts_with(b"#")),
                Arch::X86_64 => rest.starts_with(b"//") || rest.starts_with(b"#"),
            }
        };
        let mut end = start;
        let mut j = start;
        while j < t.len() && t[j] != b'\n' && t[j] != b';' {
            if line_comment(j, j == start) {
                while j < t.len() && t[j] != b'\n' {
                    j += 1;
                }
                break;
            }
            if t[j..].starts_with(b"/*") {
                self.at = t.len();
                return Some(Err(Unterminated));
            }
            j += 1;
            end = j;
        }
        self.at = j + 1;
        Some(Ok(trim(&t[start..end])))
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = s {
        if !is_space(*first) {
            break;
        }
        s = rest;
    }
    while let [rest @ .., last] = s {
        if !is_space(*last) {
            break;
        }
        s = rest;
    }
    s
}

/// Whether an inline-assembly template assembles to nothing on `arch`:
/// white space, statement separators and comments only (`spec/ir.md`
/// §2.3). A template with a `$` (an operand reference, `$$`) is never
/// empty: LLVM substitutes into the text before the assembler reads it.
pub fn asm_template_is_empty(arch: Arch, template: &[u8]) -> bool {
    if template.contains(&b'$') {
        return false;
    }
    let mut statements = Statements::new(arch, template);
    while let Some(statement) = statements.next_statement() {
        match statement {
            Ok([]) => {}
            _ => return false,
        }
    }
    true
}

// ---- module-level assembly ----

/// The binding a directive gives a symbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsmBinding {
    /// `.globl`, `.global`.
    Global,
    /// `.weak`.
    Weak,
}

/// The type `.type` gives a symbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsmSymbolType {
    /// `@function`, `%function`, `STT_FUNC`.
    Function,
    /// `@object`, `%object`, `STT_OBJECT`.
    Object,
    /// `@notype`, `%notype`, `STT_NOTYPE`.
    NoType,
}

/// What module-level assembly says of one symbol name: the directives
/// that name it, merged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AsmDirectives<'a> {
    /// The name, as the object spells it.
    pub name: &'a [u8],
    /// `.globl` or `.weak`, if one names it.
    pub binding: Option<AsmBinding>,
    /// The last of `.hidden` and `.protected` that names it.
    pub visibility: Option<Visibility>,
    /// The type `.type` gives it.
    pub ty: Option<AsmSymbolType>,
}

/// What module-level assembly does to one symbol of the object
/// (`spec/ir.md` §2.3), once checked against the module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AsmSymbolEffect<'a> {
    /// The name.
    pub name: &'a [u8],
    /// The module symbol of that name, or `None`: the object gets an
    /// undefined symbol of its own.
    pub sym: Option<SymId>,
    /// The binding, when the object's must change (or is new).
    pub binding: Option<AsmBinding>,
    /// The visibility, when the object's must change.
    pub visibility: Option<Visibility>,
    /// The type, when the object's must change.
    pub ty: Option<AsmSymbolType>,
    /// Whether the object must write the symbol even if nothing refers to
    /// it (an undefined symbol a directive names).
    pub retain: bool,
}

/// Why module-level assembly needs an assembler, or does something hod
/// does not reproduce: the kind of the first statement or name that
/// does, for a diagnostic. The construct name is always `module:asm`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AsmRefusal {
    /// What: `label`, `instruction`, `assignment`, `directive`,
    /// `operand`, `comment`, `binding`, `visibility`, `type`, `symbol`.
    pub what: &'static str,
    /// The 1-based statement number in the text (0 for a name's check).
    pub statement: u32,
}

const fn refuse<T>(what: &'static str, statement: u32) -> Result<T, AsmRefusal> {
    Err(AsmRefusal { what, statement })
}

/// A symbol name as these directives accept it: letters, digits, `_`,
/// `.` and `$`, not starting with a digit, and not an assembler-local
/// `.L` name. Quoted names and versioned names (`@`) are refused.
fn is_plain_name(name: &[u8]) -> bool {
    let Some(&first) = name.first() else { return false };
    !first.is_ascii_digit()
        && !name.starts_with(b".L")
        && name.iter().all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'$'))
}

/// Splits `operands` at commas into trimmed parts.
fn operands(operands: &[u8]) -> impl Iterator<Item = &[u8]> {
    operands.split(|&b| b == b',').map(trim)
}

/// One effect of one statement, before merging.
#[derive(Clone, Copy, Debug)]
enum Raw {
    Binding(AsmBinding),
    Visibility(Visibility),
    Type(AsmSymbolType),
}

/// Reads module-level assembly `text` (`spec/ir.md` §2.3) into `out`
/// (cleared first): one entry per name, in the order of the names'
/// first appearance, when every statement is a symbol directive hod
/// reproduces; otherwise the first statement that is not.
///
/// # Errors
/// The [`AsmRefusal`] of the first statement or name hod does not read.
pub fn read_module_asm<'a>(
    arch: Arch,
    text: &'a [u8],
    out: &mut Vec<AsmDirectives<'a>>,
) -> Result<(), AsmRefusal> {
    out.clear();
    let mut raw: Vec<(&'a [u8], u32, Raw)> = Vec::new();
    let mut statements = Statements::new(arch, text);
    let mut number = 0u32;
    while let Some(statement) = statements.next_statement() {
        number += 1;
        let Ok(statement) = statement else { return refuse("comment", number) };
        if statement.is_empty() {
            continue;
        }
        if statement[0] != b'.' {
            if statement.contains(&b':') {
                return refuse("label", number);
            }
            if statement.contains(&b'=') {
                return refuse("assignment", number);
            }
            return refuse("instruction", number);
        }
        let name_end = statement.iter().position(|&b| is_space(b)).unwrap_or(statement.len());
        let (directive, rest) = statement.split_at(name_end);
        let rest = trim(rest);
        let each = match directive {
            b".globl" | b".global" => Some(Raw::Binding(AsmBinding::Global)),
            b".weak" => Some(Raw::Binding(AsmBinding::Weak)),
            b".hidden" => Some(Raw::Visibility(Visibility::Hidden)),
            b".protected" => Some(Raw::Visibility(Visibility::Protected)),
            // `STV_INTERNAL`: no input of interest has it.
            b".internal" => return refuse("visibility", number),
            b".type" => None,
            _ if directive.ends_with(b":") => return refuse("label", number),
            _ => return refuse("directive", number),
        };
        if let Some(effect) = each {
            if rest.is_empty() {
                return refuse("operand", number);
            }
            for name in operands(rest) {
                if !is_plain_name(name) {
                    return refuse("operand", number);
                }
                raw.push((name, number, effect));
            }
            continue;
        }
        // `.type name, <type>`.
        let mut parts = operands(rest);
        let (Some(name), Some(kind), None) = (parts.next(), parts.next(), parts.next()) else {
            return refuse("operand", number);
        };
        if !is_plain_name(name) {
            return refuse("operand", number);
        }
        let ty = match kind {
            b"@function" | b"%function" | b"STT_FUNC" => AsmSymbolType::Function,
            b"@object" | b"%object" | b"STT_OBJECT" => AsmSymbolType::Object,
            b"@notype" | b"%notype" | b"STT_NOTYPE" => AsmSymbolType::NoType,
            _ => return refuse("type", number),
        };
        raw.push((name, number, Raw::Type(ty)));
    }
    // Merge by name, in order of first appearance: sort a permutation by
    // (name, statement), fold runs.
    let mut order: Vec<u32> = (0..raw.len() as u32).collect();
    order.sort_by(|&a, &b| raw[a as usize].0.cmp(raw[b as usize].0).then(a.cmp(&b)));
    let mut merged: Vec<(u32, AsmDirectives<'a>)> = Vec::new();
    let mut run = 0;
    while run < order.len() {
        let first = order[run] as usize;
        let name = raw[first].0;
        let mut entry = AsmDirectives { name, binding: None, visibility: None, ty: None };
        let mut next = run;
        while next < order.len() && raw[order[next] as usize].0 == name {
            let (_, statement, effect) = raw[order[next] as usize];
            match effect {
                Raw::Binding(binding) => {
                    // `.globl` with `.weak` makes `llc` warn and keep the
                    // last; hod does not reproduce the order rule.
                    if entry.binding.is_some_and(|b| b != binding) {
                        return refuse("binding", statement);
                    }
                    entry.binding = Some(binding);
                }
                Raw::Visibility(visibility) => {
                    // The last one wins, as in `MCELFStreamer`.
                    entry.visibility = Some(visibility);
                }
                Raw::Type(ty) => {
                    // `llc` merges types by a precedence of its own.
                    if entry.ty.is_some_and(|t| t != ty) {
                        return refuse("type", statement);
                    }
                    entry.ty = Some(ty);
                }
            }
            next += 1;
        }
        merged.push((first as u32, entry));
        run = next;
    }
    merged.sort_by_key(|&(first, _)| first);
    out.extend(merged.into_iter().map(|(_, entry)| entry));
    Ok(())
}

/// Reads `module`'s module-level assembly and checks each name against
/// the module's symbols (`spec/ir.md` §2.3): the effects on the object,
/// in `out` (cleared first). An empty text has none.
///
/// # Errors
/// [`AsmRefusal`] for a statement [`read_module_asm`] refuses, and for a
/// directive whose effect on a symbol of the module `llc` does not
/// reproduce plainly (it warns, errs, or applies a precedence rule).
pub fn module_asm_effects<'m>(
    module: &'m Module,
    out: &mut Vec<AsmSymbolEffect<'m>>,
) -> Result<(), AsmRefusal> {
    out.clear();
    let mut entries = Vec::new();
    read_module_asm(module.target().arch(), module.module_asm(), &mut entries)?;
    for entry in &entries {
        out.push(effect(module, entry)?);
    }
    Ok(())
}

fn effect<'m>(
    module: &'m Module,
    entry: &AsmDirectives<'m>,
) -> Result<AsmSymbolEffect<'m>, AsmRefusal> {
    let visibility = entry.visibility;
    let Some(sym) = module.find_symbol(entry.name) else {
        // A name the module does not have: an undefined symbol of its
        // own, global unless `.weak`.
        return Ok(AsmSymbolEffect {
            name: entry.name,
            sym: None,
            binding: Some(entry.binding.unwrap_or(AsmBinding::Global)),
            visibility,
            ty: entry.ty,
            retain: true,
        });
    };
    let symbol = module.symbol(sym);
    let linkage = symbol.linkage();
    let weakish = matches!(
        linkage,
        Linkage::LinkOnceAny | Linkage::LinkOnceOdr | Linkage::WeakAny | Linkage::WeakOdr
    );
    match linkage {
        Linkage::Internal
        | Linkage::Private
        | Linkage::Appending
        | Linkage::AvailableExternally => return refuse("symbol", 0),
        _ => {}
    }
    if symbol.is_unresolved() {
        return refuse("symbol", 0);
    }
    let definition = symbol.is_definition();
    // Binding: `.globl` of a weak symbol and `.weak` of a strong
    // definition make `llc` warn or fail; the rest change nothing, but
    // `.weak` of a declaration.
    let binding = match (entry.binding, definition) {
        (Some(AsmBinding::Global), _) if weakish || linkage == Linkage::ExternWeak => {
            return refuse("binding", 0);
        }
        (None | Some(AsmBinding::Global), _) => None,
        (Some(AsmBinding::Weak), true) if weakish => None,
        (Some(AsmBinding::Weak), true) => return refuse("binding", 0),
        (Some(AsmBinding::Weak), false) if linkage == Linkage::ExternWeak => None,
        (Some(AsmBinding::Weak), false) => Some(AsmBinding::Weak),
    };
    // Visibility: the module's own, emitted after the module's assembly,
    // wins when it is not the default.
    let visibility = if symbol.visibility() == Visibility::Default { visibility } else { None };
    // Type: a declaration takes the directive's; a definition must
    // already have it.
    let natural = match symbol.kind() {
        SymbolKind::Function { .. } => Some(AsmSymbolType::Function),
        SymbolKind::Variable { tls: TlsModel::None, .. } => Some(AsmSymbolType::Object),
        _ => None,
    };
    let ty = match entry.ty {
        None => None,
        Some(_) if symbol.tls() != TlsModel::None => return refuse("type", 0),
        Some(ty) if definition => {
            if natural != Some(ty) {
                return refuse("type", 0);
            }
            None
        }
        Some(ty) => Some(ty),
    };
    if linkage == Linkage::Common && binding.is_some() {
        return refuse("binding", 0);
    }
    Ok(AsmSymbolEffect {
        name: entry.name,
        sym: Some(sym),
        binding,
        visibility,
        ty,
        retain: !definition,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(text: &str) -> bool {
        asm_template_is_empty(Arch::Aarch64, text.as_bytes())
    }

    /// §2.3: what assembles to nothing.
    #[test]
    fn empty_templates() {
        for text in
            ["", " ", "\n\t", ";", " ; ;\n", "// x", "  # comment\n\t", "/* a */", "/*a*/ // b\n#c"]
        {
            assert!(empty(text), "{text:?}");
        }
        for text in ["nop", " nop ", "$0", "// $0", "/* open", "a: ", ".byte 0", "x /* c */"] {
            assert!(!empty(text), "{text:?}");
        }
        // `//` is a comment on x86-64 too (`llvm-mc` 19 and 22, WP-144);
        // `#` is one anywhere there.
        for text in ["// x", " # x", "/*a*/ // b\n#c", "; // x\n"] {
            assert!(asm_template_is_empty(Arch::X86_64, text.as_bytes()), "{text:?}");
        }
        for text in ["nop // x", "// x\nnop", "// $0\n x"] {
            assert!(!asm_template_is_empty(Arch::X86_64, text.as_bytes()), "{text:?}");
        }
    }

    fn read(text: &str) -> Result<Vec<AsmDirectives<'_>>, AsmRefusal> {
        let mut out = Vec::new();
        read_module_asm(Arch::Aarch64, text.as_bytes(), &mut out).map(|()| out)
    }

    /// §2.3: the directives and their merge.
    #[test]
    fn module_asm_directives() {
        let got = read(
            ".globl b\n  .weak a // c\n.hidden b; .protected b\n/* x */ .type a, @function\n\n",
        )
        .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, b"b");
        assert_eq!(got[0].binding, Some(AsmBinding::Global));
        assert_eq!(got[0].visibility, Some(Visibility::Protected));
        assert_eq!(got[1].name, b"a");
        assert_eq!(got[1].binding, Some(AsmBinding::Weak));
        assert_eq!(got[1].ty, Some(AsmSymbolType::Function));
        let got = read(".global x, y ,z\n.hidden z").unwrap();
        assert_eq!(got.iter().map(|e| e.name).collect::<Vec<_>>(), [b"x", b"y", b"z"]);
        assert_eq!(got[2].visibility, Some(Visibility::Hidden));
        assert_eq!(read("").unwrap(), []);
        assert_eq!(read("\n\n").unwrap(), []);
        // x86-64 reads `//` as a comment too (WP-144).
        let mut out = Vec::new();
        read_module_asm(Arch::X86_64, b".globl b // c\n// .weak b\n.hidden b # d", &mut out)
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binding, Some(AsmBinding::Global));
        assert_eq!(out[0].visibility, Some(Visibility::Hidden));
    }

    /// §2.3: everything else is refused, and says what it met.
    #[test]
    fn module_asm_refusals() {
        for (text, what, statement) in [
            ("  ret", "instruction", 1),
            (".globl f\nf:", "label", 2),
            ("f: ret", "label", 1),
            (".set x, 1", "directive", 1),
            ("x = 1", "assignment", 1),
            (".text", "directive", 1),
            (".symver a, a@V1", "directive", 1),
            (".size f, 4", "directive", 1),
            (".globl", "operand", 1),
            (".globl \"a b\"", "operand", 1),
            (".globl .Lx", "operand", 1),
            (".globl 1x", "operand", 1),
            (".type f, @gnu_indirect_function", "type", 1),
            (".type f", "operand", 1),
            (".internal f", "visibility", 1),
            (".globl a\n.weak a", "binding", 2),
            (".type a, @object\n.type a, @function", "type", 2),
            ("/* open", "comment", 1),
        ] {
            assert_eq!(read(text), Err(AsmRefusal { what, statement }), "{text:?}");
        }
    }
}
