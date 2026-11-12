//! The census's scanner: tokens, statements, and the reading of each
//! statement kind. Contract: `spec/harness.md` "corpus", "census".

use std::collections::{HashMap, HashSet};

use super::{D, GroupCensus, T};

/// The kinds of token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    /// A keyword, a type name, a label name: `define`, `i32`, `entry`.
    /// Also `...` and `#dbg_value`.
    Word,
    /// A number: `42`, `-1`, `1.0e+00`, `0x7FF8000000000000`.
    Num,
    /// `@name`, `@"quoted name"`, `@7`.
    Global,
    /// `%name`, `%"quoted name"`, `%7`.
    Local,
    /// `!name` or `!7`.
    Meta,
    /// `!"string"`.
    MetaStr,
    /// A `!` before `{`.
    Bang,
    /// `$name`.
    Comdat,
    /// `#7`.
    AttrGroup,
    /// `"string"`.
    Str,
    /// `c"string"`.
    CStr,
    /// Any other character.
    Punct,
    /// The end of a line.
    Nl,
}

/// A token: its kind and where its text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tok {
    pub kind: K,
    start: u32,
    end: u32,
}

impl Tok {
    /// The token's text, sigil and quotes included.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start as usize..self.end as usize]
    }
}

fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$' | b'-')
}

/// Splits a module's text into tokens. A comment is no token; a string, a
/// `c"..."` constant and a quoted name are one token each, so nothing
/// inside them is ever read as code.
pub fn lex(source: &str) -> Vec<Tok> {
    let bytes = source.as_bytes();
    let mut toks = Vec::with_capacity(bytes.len() / 4);
    let mut at = 0;
    // The end of the string that opens at `quote`.
    let string_end = |quote: usize| {
        bytes[quote + 1..].iter().position(|b| *b == b'"').map_or(bytes.len(), |n| quote + n + 2)
    };
    let name_end = |from: usize| {
        let mut end = from;
        while end < bytes.len() && (name_byte(bytes[end]) || bytes[end] == b'\\') {
            end += 1;
        }
        end
    };
    while at < bytes.len() {
        let byte = bytes[at];
        let (kind, end) = match byte {
            b' ' | b'\t' | b'\r' => {
                at += 1;
                continue;
            }
            b'\n' => (K::Nl, at + 1),
            b';' => {
                at = bytes[at..].iter().position(|b| *b == b'\n').map_or(bytes.len(), |n| at + n);
                continue;
            }
            b'"' => (K::Str, string_end(at)),
            b'@' | b'%' | b'$' | b'!' => {
                let kind = match byte {
                    b'@' => K::Global,
                    b'%' => K::Local,
                    b'$' => K::Comdat,
                    _ => K::Meta,
                };
                if bytes.get(at + 1) == Some(&b'"') {
                    (if byte == b'!' { K::MetaStr } else { kind }, string_end(at + 1))
                } else {
                    let end = name_end(at + 1);
                    if end == at + 1 {
                        (if byte == b'!' { K::Bang } else { K::Punct }, end)
                    } else {
                        (kind, end)
                    }
                }
            }
            b'#' => {
                let end = name_end(at + 1);
                let digits = end > at + 1 && bytes[at + 1..end].iter().all(u8::is_ascii_digit);
                (
                    if digits {
                        K::AttrGroup
                    } else if end > at + 1 {
                        K::Word
                    } else {
                        K::Punct
                    },
                    end,
                )
            }
            b'0'..=b'9' => (K::Num, number_end(bytes, at)),
            b'-' if bytes.get(at + 1).is_some_and(u8::is_ascii_digit) => {
                (K::Num, number_end(bytes, at + 1))
            }
            b'c' if bytes.get(at + 1) == Some(&b'"') => (K::CStr, string_end(at + 1)),
            _ if name_byte(byte) => (K::Word, name_end(at)),
            // One character; a multi-byte one is taken whole.
            _ => {
                let mut end = at + 1;
                while !source.is_char_boundary(end) {
                    end += 1;
                }
                (K::Punct, end)
            }
        };
        toks.push(Tok { kind, start: at as u32, end: end as u32 });
        at = end;
    }
    toks
}

/// The end of the number that starts at `from` (after a sign): digits,
/// letters and dots, and a sign right after an exponent's `e`.
fn number_end(bytes: &[u8], from: usize) -> usize {
    let hex = bytes[from..].starts_with(b"0x");
    let mut end = from;
    while end < bytes.len() {
        let byte = bytes[end];
        let exponent_sign = matches!(byte, b'+' | b'-')
            && !hex
            && end > from
            && matches!(bytes[end - 1], b'e' | b'E');
        if !(byte.is_ascii_alphanumeric() || byte == b'.' || exponent_sign) {
            break;
        }
        end += 1;
    }
    end
}

/// What a type is, as far as the census tells types apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Void,
    Int,
    Fp,
    Ptr,
    Vector,
    Array,
    Struct,
    /// A named type the module does not define, or an opaque one.
    Opaque,
    Label,
    Metadata,
    Token,
    Other,
}

const FP_KINDS: [&str; 7] = ["half", "bfloat", "float", "double", "fp128", "x86_fp80", "ppc_fp128"];
const LINKAGES: [&str; 11] = [
    "private",
    "internal",
    "available_externally",
    "linkonce",
    "linkonce_odr",
    "weak",
    "weak_odr",
    "common",
    "appending",
    "extern_weak",
    "external",
];
const VISIBILITY_WORDS: [&str; 9] = [
    "dso_local",
    "dso_preemptable",
    "default",
    "hidden",
    "protected",
    "dllimport",
    "dllexport",
    "unnamed_addr",
    "local_unnamed_addr",
];
const OTHER_CCONVS: [&str; 10] = [
    "aarch64_vector_pcs",
    "aarch64_sve_vector_pcs",
    "aarch64_sme_preservemost_from_x0",
    "aarch64_sme_preservemost_from_x1",
    "aarch64_sme_preservemost_from_x2",
    "spir_func",
    "spir_kernel",
    "ptx_kernel",
    "ptx_device",
    "riscv_vector_cc",
];
const FAST_MATH: [&str; 8] = ["nnan", "ninf", "nsz", "arcp", "contract", "afn", "reassoc", "fast"];
const ORDERINGS: [&str; 6] = ["unordered", "monotonic", "acquire", "release", "acq_rel", "seq_cst"];
const BINARY: [&str; 19] = [
    "add", "sub", "mul", "udiv", "sdiv", "urem", "srem", "shl", "lshr", "ashr", "and", "or", "xor",
    "fadd", "fsub", "fmul", "fdiv", "frem", "fneg",
];
const CASTS: [&str; 14] = [
    "trunc",
    "zext",
    "sext",
    "fptrunc",
    "fpext",
    "fptoui",
    "fptosi",
    "uitofp",
    "sitofp",
    "ptrtoint",
    "ptrtoaddr",
    "inttoptr",
    "bitcast",
    "addrspacecast",
];
/// Words that are an operation when `(` follows: a constant expression.
const CONST_EXPRS: [&str; 30] = [
    "getelementptr",
    "bitcast",
    "inttoptr",
    "ptrtoint",
    "ptrtoaddr",
    "addrspacecast",
    "trunc",
    "zext",
    "sext",
    "add",
    "sub",
    "mul",
    "xor",
    "and",
    "or",
    "shl",
    "lshr",
    "ashr",
    "icmp",
    "fcmp",
    "select",
    "extractelement",
    "insertelement",
    "shufflevector",
    "fptrunc",
    "fpext",
    "uitofp",
    "sitofp",
    "fptoui",
    "fptosi",
];
/// Words a constant operand can start with: where the attributes of a call
/// argument end.
const VALUE_WORDS: [&str; 12] = [
    "null",
    "undef",
    "poison",
    "zeroinitializer",
    "true",
    "false",
    "none",
    "blockaddress",
    "dso_local_equivalent",
    "no_cfi",
    "ptrauth",
    "splat",
];
/// Attributes whose argument is part of what they mean and is kept.
const KEEP_ARGUMENT: [&str; 7] =
    ["memory", "captures", "nofpclass", "allockind", "uwtable", "denormal_fpenv", "alignstack"];
/// Attributes whose argument is a type, written `(T)`.
const TYPE_ARGUMENT: [&str; 6] =
    ["byval", "sret", "byref", "inalloca", "preallocated", "elementtype"];

fn is_int_type(word: &str) -> bool {
    word.len() > 1 && word.starts_with('i') && word[1..].bytes().all(|b| b.is_ascii_digit())
}

fn is_cconv(word: &str) -> bool {
    (word.ends_with("cc") && word != "cc") || OTHER_CCONVS.contains(&word)
}

/// The name of an intrinsic without its overload suffix: the trailing
/// components that spell types (`.i64`, `.p0`, `.v4i32`, `.nxv2f64`,
/// `.f64`, `.a2i8`, `.s_name`) are dropped.
pub fn fold_intrinsic(name: &str) -> &str {
    fn spells_type(part: &str) -> bool {
        let digits_after = |prefix: &str| {
            part.strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        };
        let shape_after = |prefix: &str| {
            part.strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        };
        digits_after("i")
            || digits_after("f")
            || digits_after("p")
            || shape_after("v")
            || shape_after("nxv")
            || shape_after("a")
            || part.starts_with("s_")
            || part.starts_with("sl_")
            || matches!(part, "ppcf128" | "bf16" | "x86amx" | "isVoid" | "Metadata" | "anyptr")
    }
    let mut end = name.len();
    while let Some(dot) = name[..end].rfind('.') {
        // `llvm.<something>` is the least that is kept.
        if name[..dot].matches('.').count() < 1 || !spells_type(&name[dot + 1..end]) {
            break;
        }
        end = dot;
    }
    &name[..end]
}

/// One module being scanned.
struct Scan<'a> {
    src: &'a str,
    toks: Vec<Tok>,
    /// Named types: the tokens of their bodies.
    types: HashMap<&'a str, (usize, usize)>,
    /// Attribute groups: their attributes, rendered.
    groups: HashMap<&'a str, Vec<String>>,
    /// The numbered nodes `!llvm.module.flags` and `!llvm.ident` list.
    flag_nodes: HashSet<&'a str>,
    ident_nodes: HashSet<&'a str>,
}

impl<'a> Scan<'a> {
    fn text(&self, index: usize) -> &'a str {
        self.toks.get(index).map_or("", |tok| tok.text(self.src))
    }

    fn kind(&self, index: usize) -> Option<K> {
        self.toks.get(index).map(|tok| tok.kind)
    }

    fn is(&self, index: usize, punct: &str) -> bool {
        self.kind(index) == Some(K::Punct) && self.text(index) == punct
    }

    fn is_word(&self, index: usize, word: &str) -> bool {
        self.kind(index) == Some(K::Word) && self.text(index) == word
    }

    /// The source text of the tokens `from..to`.
    fn span(&self, from: usize, to: usize) -> &'a str {
        if from >= to || to > self.toks.len() {
            return "";
        }
        &self.src[self.toks[from].start as usize..self.toks[to - 1].end as usize]
    }

    /// The index after the bracket that closes the one at `open`, within
    /// `..end`.
    fn close(&self, open: usize, end: usize) -> usize {
        let mut depth = 0usize;
        for index in open..end {
            if self.kind(index) == Some(K::Punct) {
                match self.text(index) {
                    "(" | "[" | "{" | "<" => depth += 1,
                    ")" | "]" | "}" | ">" => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return index + 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        end
    }

    /// The first index in `from..end`, outside brackets, where `found` holds.
    fn find_flat(&self, from: usize, end: usize, found: impl Fn(usize) -> bool) -> Option<usize> {
        let mut depth = 0usize;
        for index in from..end {
            if depth == 0 && found(index) {
                return Some(index);
            }
            if self.kind(index) == Some(K::Punct) {
                match self.text(index) {
                    "(" | "[" | "{" | "<" => depth += 1,
                    ")" | "]" | "}" | ">" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        None
    }

    /// The parts of `from..end` between commas that are outside brackets.
    fn split_commas(&self, from: usize, end: usize) -> Vec<(usize, usize)> {
        let mut parts = Vec::new();
        let mut start = from;
        while start < end {
            let stop = self.find_flat(start, end, |index| self.is(index, ",")).unwrap_or(end);
            parts.push((start, stop));
            start = stop + 1;
        }
        parts
    }

    fn starts_type(&self, index: usize) -> bool {
        match self.kind(index) {
            Some(K::Word) => {
                let word = self.text(index);
                is_int_type(word)
                    || FP_KINDS.contains(&word)
                    || matches!(
                        word,
                        "void"
                            | "ptr"
                            | "label"
                            | "metadata"
                            | "token"
                            | "opaque"
                            | "x86_amx"
                            | "target"
                    )
            }
            Some(K::Local) => true,
            Some(K::Punct) => matches!(self.text(index), "<" | "[" | "{"),
            _ => false,
        }
    }

    /// The type that starts at `index`: its class and the index after it.
    /// A function type's parameter list is not taken.
    fn parse_type(&self, index: usize, end: usize) -> Option<(Class, usize)> {
        if index >= end || !self.starts_type(index) {
            return None;
        }
        let word = self.text(index);
        Some(match self.kind(index)? {
            K::Word => match word {
                "void" => (Class::Void, index + 1),
                "label" => (Class::Label, index + 1),
                "metadata" => (Class::Metadata, index + 1),
                "token" => (Class::Token, index + 1),
                "opaque" => (Class::Opaque, index + 1),
                "ptr" if self.is_word(index + 1, "addrspace") && self.is(index + 2, "(") => {
                    (Class::Ptr, self.close(index + 2, end))
                }
                "ptr" => (Class::Ptr, index + 1),
                "target" if self.is(index + 1, "(") => (Class::Other, self.close(index + 1, end)),
                word if is_int_type(word) => (Class::Int, index + 1),
                word if FP_KINDS.contains(&word) => (Class::Fp, index + 1),
                _ => (Class::Other, index + 1),
            },
            K::Local => {
                let class = match self.types.get(word) {
                    Some(&(from, to)) if from < to && self.text(from) != "opaque" => {
                        self.parse_type(from, to).map_or(Class::Opaque, |(class, _)| class)
                    }
                    _ => Class::Opaque,
                };
                (class, index + 1)
            }
            _ => {
                let after = self.close(index, end);
                let class = match word {
                    "[" => Class::Array,
                    "{" => Class::Struct,
                    // `<{ ... }>` is a packed struct, `<N x T>` a vector.
                    _ if self.is(index + 1, "{") => Class::Struct,
                    _ => Class::Vector,
                };
                (class, after)
            }
        })
    }

    /// How a type is named in a table that is by type: integers,
    /// floating-point kinds and vectors by their text, the rest by class.
    fn type_name(&self, index: usize, end: usize) -> String {
        match self.parse_type(index, end) {
            Some((Class::Int | Class::Fp | Class::Vector | Class::Ptr | Class::Other, after)) => {
                self.span(index, after).to_owned()
            }
            Some((Class::Void, _)) => "void".to_owned(),
            Some((Class::Array, _)) => "array".to_owned(),
            Some((Class::Struct, _)) => "struct".to_owned(),
            Some((Class::Opaque, _)) => "opaque".to_owned(),
            Some((Class::Label, _)) => "label".to_owned(),
            Some((Class::Metadata, _)) => "metadata".to_owned(),
            Some((Class::Token, _)) => "token".to_owned(),
            None => "?".to_owned(),
        }
    }

    /// The text of the type in `from..to` with every named type replaced
    /// by its body, a few levels deep and cut at 96 characters.
    fn shape(&self, from: usize, to: usize, depth: u32) -> String {
        let mut out = String::new();
        let mut copied = self.toks.get(from).map_or(0, |tok| tok.start as usize);
        for index in from..to {
            let tok = self.toks[index];
            if tok.kind != K::Local {
                continue;
            }
            out.push_str(&self.src[copied..tok.start as usize]);
            match self.types.get(tok.text(self.src)) {
                Some(&(body, body_end)) if depth < 4 => {
                    out.push_str(&self.shape(body, body_end, depth + 1));
                }
                _ => out.push_str(tok.text(self.src)),
            }
            copied = tok.end as usize;
        }
        if from < to {
            out.push_str(&self.src[copied..self.toks[to - 1].end as usize]);
        }
        if out.len() > 96 {
            let mut cut = 93;
            while !out.is_char_boundary(cut) {
                cut -= 1;
            }
            out.truncate(cut);
            out.push_str("...");
        }
        out
    }

    /// One attribute at `index`: how it is counted, and the index after
    /// it. An attribute group is expanded by the caller.
    fn attribute(&self, index: usize, end: usize) -> (String, usize) {
        let word = self.text(index);
        if self.kind(index) == Some(K::Str) {
            if self.is(index + 1, "=") && self.kind(index + 2) == Some(K::Str) {
                let value = self.text(index + 2);
                let mut cut = value.len().min(48);
                while !value.is_char_boundary(cut) {
                    cut -= 1;
                }
                let dots = if cut < value.len() { "...\"" } else { "" };
                return (format!("{word}={}{dots}", &value[..cut]), index + 3);
            }
            return (word.to_owned(), index + 1);
        }
        if self.is(index + 1, "(") {
            let after = self.close(index + 1, end);
            let rendered = if KEEP_ARGUMENT.contains(&word) {
                self.span(index, after).to_owned()
            } else if TYPE_ARGUMENT.contains(&word) {
                format!("{word}(T)")
            } else {
                format!("{word}(...)")
            };
            return (rendered, after);
        }
        if self.kind(index + 1) == Some(K::Num) && matches!(word, "align" | "alignstack") {
            return (format!("{word} {}", self.text(index + 1)), index + 2);
        }
        if self.is(index + 1, "=") && self.kind(index + 2) == Some(K::Num) {
            return (format!("{word}={}", self.text(index + 2)), index + 3);
        }
        (word.to_owned(), index + 1)
    }
}

/// The counting of one module.
struct Counter<'a, 'c> {
    scan: Scan<'a>,
    census: &'c mut GroupCensus,
    definitions: u64,
    declarations: u64,
    variables: u64,
    blocks: u64,
    instructions: u64,
    nodes: u64,
    fn_blocks: u64,
    fn_instructions: u64,
    /// A definition's body is being counted.
    open: bool,
}

/// Counts one module into `census`.
pub fn scan_module(census: &mut GroupCensus, module: &str) {
    census.modules += 1;
    let toks = lex(module);
    let scan = Scan {
        src: module,
        toks,
        types: HashMap::new(),
        groups: HashMap::new(),
        flag_nodes: HashSet::new(),
        ident_nodes: HashSet::new(),
    };
    let mut counter = Counter {
        scan,
        census,
        definitions: 0,
        declarations: 0,
        variables: 0,
        blocks: 0,
        instructions: 0,
        nodes: 0,
        fn_blocks: 0,
        fn_instructions: 0,
        open: false,
    };
    let statements = counter.statements();
    counter.first_pass(&statements);
    for &(from, to, in_body) in &statements {
        if in_body {
            counter.body_statement(from, to);
        } else {
            counter.end_function();
            counter.top_level(from, to);
        }
    }
    counter.end_function();
    let lines = module.bytes().filter(|b| *b == b'\n').count() as u64;
    for (of, value) in [
        (D::ModuleBytes, module.len() as u64),
        (D::ModuleLines, lines),
        (D::Definitions, counter.definitions),
        (D::Declarations, counter.declarations),
        (D::GlobalVariables, counter.variables),
        (D::BlocksPerModule, counter.blocks),
        (D::InstructionsPerModule, counter.instructions),
        (D::MetadataNodes, counter.nodes),
    ] {
        counter.census.sample(of, value);
    }
}

impl<'a> Counter<'a, '_> {
    fn bump(&mut self, table: T, key: &str) {
        self.census.bump(table, key);
    }

    /// The statements of the module: token ranges, and whether each is in
    /// a function body. A statement is a line, joined with the lines that
    /// follow while a bracket is open and, in a body, with a following line
    /// that starts with `to`, `unwind`, `catch`, `cleanup` or `filter`. The
    /// `{` that ends a `define` and the `}` that ends a body are not part
    /// of a statement.
    fn statements(&self) -> Vec<(usize, usize, bool)> {
        let scan = &self.scan;
        let mut out = Vec::new();
        let mut in_body = false;
        let mut pending: Option<(usize, usize)> = None;
        let mut depth = 0i64;
        let mut line = 0;
        let count = scan.toks.len();
        while line < count {
            let mut end = line;
            while end < count && scan.toks[end].kind != K::Nl {
                end += 1;
            }
            let next_line = end + 1;
            if end == line {
                line = next_line;
                continue;
            }
            let continues = pending.is_some()
                && (depth > 0
                    || (in_body
                        && scan.kind(line) == Some(K::Word)
                        && matches!(
                            scan.text(line),
                            "to" | "unwind" | "catch" | "cleanup" | "filter"
                        )));
            if !continues {
                if let Some((from, to)) = pending.take() {
                    out.push((from, to, in_body));
                }
                depth = 0;
                if in_body && end == line + 1 && scan.is(line, "}") {
                    in_body = false;
                    line = next_line;
                    continue;
                }
            }
            let mut stop = end;
            let opens_body = !in_body
                && pending.is_none()
                && scan.is_word(line, "define")
                && scan.is(end - 1, "{");
            if opens_body {
                stop = end - 1;
            }
            for index in line..stop {
                if scan.toks[index].kind == K::Punct {
                    match scan.text(index) {
                        "(" | "[" | "{" | "<" => depth += 1,
                        ")" | "]" | "}" | ">" => depth -= 1,
                        _ => {}
                    }
                }
            }
            pending = Some((pending.map_or(line, |(from, _)| from), stop));
            if opens_body {
                if let Some((from, to)) = pending.take() {
                    out.push((from, to, false));
                }
                depth = 0;
                in_body = true;
            }
            line = next_line;
        }
        if let Some((from, to)) = pending {
            out.push((from, to, in_body));
        }
        out
    }

    /// Collects what later statements refer to: named types, attribute
    /// groups, and the nodes of the module flags and of `!llvm.ident`.
    fn first_pass(&mut self, statements: &[(usize, usize, bool)]) {
        for &(from, to, in_body) in statements {
            if in_body {
                continue;
            }
            let scan = &mut self.scan;
            let first = scan.text(from);
            match scan.kind(from) {
                Some(K::Local) if scan.is(from + 1, "=") && scan.is_word(from + 2, "type") => {
                    scan.types.insert(first, (from + 3, to));
                }
                Some(K::Word)
                    if first == "attributes" && scan.kind(from + 1) == Some(K::AttrGroup) =>
                {
                    let name = scan.text(from + 1);
                    let mut attrs = Vec::new();
                    let mut index = from + 4;
                    while index + 1 < to {
                        let (attr, after) = scan.attribute(index, to - 1);
                        attrs.push(attr);
                        index = after;
                    }
                    scan.groups.insert(name, attrs);
                }
                Some(K::Meta) if matches!(first, "!llvm.module.flags" | "!llvm.ident") => {
                    for index in from + 1..to {
                        if scan.kind(index) == Some(K::Meta) {
                            let node = scan.text(index);
                            if first == "!llvm.ident" {
                                scan.ident_nodes.insert(node);
                            } else {
                                scan.flag_nodes.insert(node);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Counts every type mention and constant in `from..to`; `place` is
    /// `initializer` or `operand`.
    #[expect(
        clippy::too_many_lines,
        reason = "one case after another; splitting it would not make it clearer"
    )]
    fn mentions(&mut self, from: usize, to: usize, place: &str) {
        // A signature and a type have no constants: `none` and the like
        // are attribute arguments there.
        let constants = !matches!(place, "signature" | "type");
        let mut index = from;
        while index < to {
            let kind = self.scan.toks[index].kind;
            let word = self.scan.text(index);
            match kind {
                K::Word if is_int_type(word) => self.bump(T::IntWidth, word),
                K::Word if FP_KINDS.contains(&word) => self.bump(T::TypeMention, word),
                K::Word if word == "ptr" => {
                    if self.scan.is_word(index + 1, "addrspace") {
                        let after = self.scan.close(index + 2, to);
                        let text = self.scan.span(index, after).to_owned();
                        self.bump(T::TypeMention, &text);
                    } else {
                        self.bump(T::TypeMention, "ptr");
                    }
                }
                K::Word if matches!(word, "token" | "x86_amx") => self.bump(T::TypeMention, word),
                K::Word if word == "target" && self.scan.is(index + 1, "(") => {
                    let after = self.scan.close(index + 1, to);
                    let text = self.scan.span(index, after).to_owned();
                    self.bump(T::TypeMention, &text);
                }
                K::Word
                    if constants
                        && VALUE_WORDS.contains(&word)
                        && !matches!(word, "true" | "false")
                        // `none` is a constant only as a `token`.
                        && (word != "none" || (index > from && self.scan.is_word(index - 1, "token"))) =>
                {
                    self.bump(T::ConstKind, &format!("{place}: {word}"));
                }
                K::Word if constants && CONST_EXPRS.contains(&word) => {
                    // Flags, a predicate, `inrange(...)`, then `(`.
                    let mut after = index + 1;
                    while self.scan.kind(after) == Some(K::Word)
                        && !self.scan.starts_type(after)
                        && after < to
                    {
                        after += 1;
                        if self.scan.text(after - 1) == "inrange" && self.scan.is(after, "(") {
                            after = self.scan.close(after, to);
                        }
                    }
                    if self.scan.is(after, "(") && after > index {
                        let is_opcode = index == from && place == "operand";
                        if !is_opcode {
                            self.bump(T::ConstExpr, &format!("{place}: {word}"));
                        }
                    }
                }
                K::CStr if constants => {
                    self.bump(T::ConstKind, &format!("{place}: c\"...\" string"));
                }
                K::Num if constants => {
                    let form = if let Some(hex) = word.strip_prefix("0x") {
                        match hex.as_bytes().first() {
                            Some(b'H') => Some("0xH (half)"),
                            Some(b'K') => Some("0xK (x86_fp80)"),
                            Some(b'L') => Some("0xL (fp128)"),
                            Some(b'M') => Some("0xM (ppc_fp128)"),
                            Some(b'R') => Some("0xR (bfloat)"),
                            _ => Some("0x... (hexadecimal double)"),
                        }
                    } else if word.contains('.') {
                        Some("decimal")
                    } else {
                        None
                    };
                    if let Some(form) = form {
                        self.bump(
                            T::ConstKind,
                            &format!("{place}: floating-point literal, {form}"),
                        );
                    }
                }
                K::Global if place == "initializer" => {
                    self.bump(T::ConstKind, "initializer: address of a global or function");
                }
                K::Punct if word == "<" => {
                    let scalable = self.scan.is_word(index + 1, "vscale");
                    if scalable || self.scan.kind(index + 1) == Some(K::Num) {
                        let after = self.scan.close(index, to);
                        if self.scan.is_word(index + if scalable { 4 } else { 2 }, "x")
                            || self.scan.is_word(index + 2, "x")
                        {
                            let text = self.scan.span(index, after).to_owned();
                            self.bump(T::VectorTy, &text);
                        }
                    }
                }
                K::Punct
                    if word == "["
                        && self.scan.kind(index + 1) == Some(K::Num)
                        && self.scan.is_word(index + 2, "x") =>
                {
                    let element = match self.scan.kind(index + 3) {
                        Some(K::Word) => self.scan.text(index + 3),
                        Some(K::Local) => "a named type",
                        _ => match self.scan.text(index + 3) {
                            "[" => "an array",
                            "<" if self.scan.is(index + 4, "{") => "a struct",
                            "<" => "a vector",
                            _ => "a struct",
                        },
                    };
                    self.bump(T::TypeMention, &format!("array of {element}"));
                }
                _ => {}
            }
            index += 1;
        }
    }

    /// Counts `!kind` attachments outside brackets in `from..to`.
    fn attachments(&mut self, from: usize, to: usize, table: T) {
        let mut depth = 0usize;
        for index in from..to {
            let tok = self.scan.toks[index];
            match tok.kind {
                K::Punct => match self.scan.text(index) {
                    "(" | "[" | "{" | "<" => depth += 1,
                    ")" | "]" | "}" | ">" => depth = depth.saturating_sub(1),
                    _ => {}
                },
                K::Meta if depth == 0 => {
                    let name = self.scan.text(index);
                    if !name[1..].bytes().all(|b| b.is_ascii_digit()) && !name.starts_with("!DI") {
                        self.bump(table, name);
                    }
                }
                _ => {}
            }
        }
    }

    /// Counts the attributes in `from..to` into `table`, expanding
    /// attribute groups; stops at the first token that is not one.
    /// Returns the index it stopped at.
    fn attributes(&mut self, from: usize, to: usize, table: T, stop_at_values: bool) -> usize {
        let mut index = from;
        while index < to {
            match self.scan.kind(index) {
                Some(K::AttrGroup) => {
                    let group = self.scan.groups.get(self.scan.text(index)).cloned();
                    for attr in group.unwrap_or_default() {
                        self.bump(table, &attr);
                    }
                    index += 1;
                }
                Some(K::Word) => {
                    let word = self.scan.text(index);
                    // An `invoke`'s destinations are not attributes.
                    if self.scan.starts_type(index)
                        || matches!(word, "to" | "unwind")
                        || (stop_at_values
                            && (VALUE_WORDS.contains(&word)
                                || CONST_EXPRS.contains(&word)
                                || word == "asm"))
                    {
                        break;
                    }
                    let (attr, after) = self.scan.attribute(index, to);
                    self.bump(table, &attr);
                    index = after;
                }
                Some(K::Str) => {
                    let (attr, after) = self.scan.attribute(index, to);
                    self.bump(table, &attr);
                    index = after;
                }
                _ => break,
            }
        }
        index
    }

    /// A type that is passed, returned or given by value: counted when it
    /// is an aggregate.
    fn by_value(&mut self, role: &str, index: usize, end: usize) {
        if let Some((Class::Struct | Class::Array, after)) = self.scan.parse_type(index, end) {
            let shape = self.scan.shape(index, after, 0);
            self.bump(T::AggByValue, &format!("{role}: {shape}"));
        }
    }

    fn top_level(&mut self, from: usize, to: usize) {
        let first = self.scan.text(from);
        match self.scan.kind(from) {
            Some(K::Word) => match first {
                "define" | "declare" => self.function_header(from, to),
                "target" => {
                    let what = self.scan.text(from + 1);
                    let value = self.scan.text(from + 3);
                    self.bump(T::TopLevel, &format!("target {what}"));
                    self.bump(T::Header, &format!("{what} = {value}"));
                }
                "source_filename" => self.bump(T::TopLevel, "source_filename"),
                "module" if self.scan.is_word(from + 1, "asm") => {
                    self.bump(T::TopLevel, "module asm");
                    let line = self.scan.text(from + 2).trim_matches('"');
                    let word = line.split([' ', '\\']).find(|w| !w.is_empty());
                    self.bump(T::ModuleAsm, word.unwrap_or("(empty)"));
                }
                "attributes" => self.bump(T::TopLevel, "attributes group"),
                "uselistorder" | "uselistorder_bb" => self.bump(T::TopLevel, first),
                _ => {
                    self.bump(T::TopLevel, "(not recognised)");
                    self.bump(T::Unrecognised, &format!("top level: {first}"));
                }
            },
            Some(K::Local) if self.scan.is_word(from + 2, "type") => {
                let opaque = self.scan.is_word(from + 3, "opaque");
                self.bump(T::TopLevel, if opaque { "named type, opaque" } else { "named type" });
                self.mentions(from + 3, to, "type");
            }
            Some(K::Comdat) => {
                self.bump(T::TopLevel, "comdat");
                let kind = self.scan.text(from + 3).to_owned();
                self.bump(T::Comdat, &format!("comdat {kind}"));
            }
            Some(K::Global) => self.global(from, to),
            Some(K::Meta) => self.metadata(from, to),
            _ => {
                self.bump(T::TopLevel, "(not recognised)");
                self.bump(T::Unrecognised, &format!("top level: {first}"));
            }
        }
    }

    fn metadata(&mut self, from: usize, to: usize) {
        let name = self.scan.text(from);
        if !name[1..].bytes().all(|b| b.is_ascii_digit()) {
            self.bump(T::TopLevel, "named metadata");
            self.bump(T::NamedMetadata, name);
            return;
        }
        self.bump(T::TopLevel, "metadata node");
        self.nodes += 1;
        let mut body = from + 2;
        if self.scan.is_word(body, "distinct") {
            body += 1;
        }
        let head = self.scan.text(body);
        if self.scan.kind(body) == Some(K::Meta) && head.starts_with("!DI") {
            self.bump(T::DiNode, head);
        } else if self.scan.kind(body) == Some(K::Meta) {
            self.bump(T::DiNode, &format!("{head} (other specialized node)"));
        } else {
            self.bump(T::DiNode, "tuple !{...}");
        }
        for index in body..to {
            if self.scan.kind(index) == Some(K::Word) {
                let word = self.scan.text(index);
                if word.starts_with("DW_") {
                    self.bump(T::Dwarf, word);
                } else if word == "emissionKind" {
                    let kind = self.scan.text(index + 2).to_owned();
                    self.bump(T::Dwarf, &format!("emissionKind: {kind}"));
                }
            }
        }
        if self.scan.flag_nodes.contains(name) {
            // `!{i32 <behavior>, !"<name>", <value>}`.
            let flag = self.scan.text(body + 5).trim_start_matches('!').to_owned();
            let behavior = self.scan.text(body + 3);
            let value = self.scan.span(body + 7, to.saturating_sub(1));
            self.bump(T::ModuleFlag, &format!("{flag} = {value} (behavior {behavior})"));
        }
        if self.scan.ident_nodes.contains(name) {
            let ident = self.scan.text(body + 2).trim_start_matches('!').to_owned();
            self.bump(T::Header, &format!("ident = {ident}"));
        }
    }

    /// The words before a function's return type or a variable's kind:
    /// linkage, visibility and the like. Returns the index after them.
    fn prefix_words(&mut self, from: usize, to: usize, what: &str) -> usize {
        let mut index = from;
        let mut linkage = None;
        let mut cconv = None;
        while index < to {
            let word = self.scan.text(index);
            match self.scan.kind(index) {
                Some(K::Word) if LINKAGES.contains(&word) => linkage = Some(word),
                Some(K::Word) if VISIBILITY_WORDS.contains(&word) => {
                    self.bump(T::Visibility, &format!("{what}: {word}"));
                }
                Some(K::Word) if word == "thread_local" => {
                    if self.scan.is(index + 1, "(") {
                        let after = self.scan.close(index + 1, to);
                        let text = self.scan.span(index, after).to_owned();
                        self.bump(T::ThreadLocal, &text);
                        index = after;
                        continue;
                    }
                    self.bump(T::ThreadLocal, "thread_local (general dynamic)");
                }
                Some(K::Word) if word == "cc" && self.scan.kind(index + 1) == Some(K::Num) => {
                    cconv = Some(format!("cc {}", self.scan.text(index + 1)));
                    index += 1;
                }
                Some(K::Word) if is_cconv(word) => cconv = Some(word.to_owned()),
                Some(K::Word) if word == "addrspace" && self.scan.is(index + 1, "(") => {
                    let after = self.scan.close(index + 1, to);
                    let text = self.scan.span(index, after).to_owned();
                    self.bump(T::GlobalKind, &format!("{what} in {text}"));
                    index = after;
                    continue;
                }
                Some(K::Word) if word == "externally_initialized" => {
                    self.bump(T::GlobalKind, "externally_initialized");
                }
                Some(K::Meta) => {
                    // `declare !dbg !7 ...`.
                    self.bump(T::MdAttachGlobal, word);
                    index += 1;
                }
                _ => break,
            }
            index += 1;
        }
        self.bump(T::Linkage, &format!("{what}: {}", linkage.unwrap_or("external (default)")));
        if what == "define" || what == "declare" {
            let cconv = cconv.unwrap_or_else(|| "ccc (default)".to_owned());
            self.bump(T::CallConv, &format!("{what}: {cconv}"));
        }
        index
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one case after another; splitting it would not make it clearer"
    )]
    fn function_header(&mut self, from: usize, to: usize) {
        let what = self.scan.text(from);
        let defined = what == "define";
        self.bump(T::TopLevel, what);
        if defined {
            self.open = true;
            self.definitions += 1;
            self.fn_blocks = 0;
            self.fn_instructions = 0;
        } else {
            self.declarations += 1;
        }
        let fn_attrs = if defined { T::FnAttr } else { T::FnAttrDecl };
        let index = self.prefix_words(from + 1, to, what);
        let index = self.attributes(index, to, T::RetAttr, false);
        let Some((_, after_type)) = self.scan.parse_type(index, to) else {
            self.bump(T::Unrecognised, &format!("{what}: no return type"));
            return;
        };
        let returns = self.scan.type_name(index, to);
        self.bump(T::Signature, &format!("{what} returns {returns}"));
        self.by_value(&format!("{what} result"), index, to);
        let name_at = after_type;
        let name = self.scan.text(name_at);
        if name.starts_with("@llvm.") && !defined {
            self.bump(T::IntrinsicDecl, fold_intrinsic(&name[1..]));
        }
        if !self.scan.is(name_at + 1, "(") {
            self.bump(T::Unrecognised, &format!("{what}: no parameter list"));
            return;
        }
        let close = self.scan.close(name_at + 1, to);
        let params = self.scan.split_commas(name_at + 2, close - 1);
        let mut count = 0;
        for (start, end) in params {
            if self.scan.is_word(start, "...") {
                self.bump(T::Signature, &format!("{what} variadic"));
                continue;
            }
            count += 1;
            let Some((_, after)) = self.scan.parse_type(start, end) else { continue };
            let class = self.scan.type_name(start, end);
            self.bump(T::Signature, &format!("{what} parameter {class}"));
            self.by_value(&format!("{what} parameter"), start, end);
            self.attributes(after, end, T::ParamAttr, false);
        }
        let bucket = match count {
            0..=8 => count.to_string(),
            9..=16 => "9-16".to_owned(),
            _ => "17 or more".to_owned(),
        };
        self.bump(T::Signature, &format!("{what} with {bucket} parameters"));
        self.mentions(from + 1, close, "signature");

        // After the parameters.
        let mut index = close;
        while index < to {
            let word = self.scan.text(index);
            match self.scan.kind(index) {
                Some(K::Word) if matches!(word, "unnamed_addr" | "local_unnamed_addr") => {
                    self.bump(T::Visibility, &format!("{what}: {word}"));
                    index += 1;
                }
                Some(K::Word) if matches!(word, "section" | "partition") => {
                    let name = self.scan.text(index + 1).to_owned();
                    self.bump(T::Section, &format!("{what}: {word} {name}"));
                    index += 2;
                }
                Some(K::Word) if word == "comdat" => {
                    self.bump(T::Comdat, &format!("on {what}"));
                    index += 1;
                    if self.scan.is(index, "(") {
                        index = self.scan.close(index, to);
                    }
                }
                Some(K::Word) if word == "gc" => {
                    let name = self.scan.text(index + 1).to_owned();
                    self.bump(T::Signature, &format!("{what} gc {name}"));
                    index += 2;
                }
                Some(K::Word) if matches!(word, "prefix" | "prologue" | "personality") => {
                    // A typed constant: up to the next word of this list,
                    // attachment or the end.
                    let stop = self
                        .scan
                        .find_flat(index + 1, to, |at| {
                            at > index + 2
                                && (self.scan.kind(at) == Some(K::Meta)
                                    || matches!(
                                        self.scan.text(at),
                                        "prefix" | "prologue" | "personality"
                                    ))
                        })
                        .unwrap_or(to);
                    if word == "personality" {
                        let function = (index + 1..stop)
                            .find(|at| self.scan.kind(*at) == Some(K::Global))
                            .map_or("?", |at| self.scan.text(at));
                        self.bump(T::Eh, &format!("personality {function}"));
                    } else {
                        self.bump(T::Signature, &format!("{what} with {word} data"));
                    }
                    index = stop;
                }
                Some(K::Meta) => {
                    self.bump(T::MdAttachGlobal, word);
                    index += 2;
                }
                Some(K::AttrGroup) => {
                    let group = self.scan.groups.get(word).cloned();
                    for attr in group.unwrap_or_default() {
                        self.bump(fn_attrs, &attr);
                    }
                    index += 1;
                }
                Some(K::Word | K::Str) => {
                    let (attr, after) = self.scan.attribute(index, to);
                    self.bump(fn_attrs, &attr);
                    index = after;
                }
                _ => index += 1,
            }
        }
    }

    fn global(&mut self, from: usize, to: usize) {
        self.bump(T::TopLevel, "global variable, alias or ifunc");
        let name = self.scan.text(from);
        // Find the kind word first: what the prefix is counted as.
        let kind_at = self.scan.find_flat(from + 2, to, |at| {
            self.scan.kind(at) == Some(K::Word)
                && matches!(self.scan.text(at), "global" | "constant" | "alias" | "ifunc")
        });
        let Some(kind_at) = kind_at else {
            self.bump(T::Unrecognised, "global without a kind word");
            return;
        };
        let kind = self.scan.text(kind_at);
        let what = if matches!(kind, "alias" | "ifunc") { kind } else { "variable" };
        if what == "variable" {
            self.variables += 1;
        }
        self.prefix_words(from + 2, kind_at, what);
        let type_at = kind_at + 1;
        let Some((_, after_type)) = self.scan.parse_type(type_at, to) else {
            self.bump(T::Unrecognised, "global without a type");
            return;
        };
        let class = self.scan.type_name(type_at, to);
        // The initializer ends at the first comma outside brackets.
        let init_end =
            self.scan.find_flat(after_type, to, |at| self.scan.is(at, ",")).unwrap_or(to);
        let has_init = init_end > after_type;
        let kind_name = if what != "variable" {
            kind.to_owned()
        } else if has_init {
            format!("{kind} {class}")
        } else {
            format!("{kind} {class}, declaration")
        };
        self.bump(T::GlobalKind, &kind_name);
        if name.starts_with("@llvm.") {
            self.bump(T::GlobalKind, name);
        }
        self.mentions(type_at, after_type, "initializer");
        if what == "variable" {
            self.mentions(after_type, init_end, "initializer");
        } else {
            // An alias's or IFUNC's target follows the comma.
            self.mentions(init_end, to, "aliasee");
        }
        let mut index = init_end;
        while index < to {
            let word = self.scan.text(index);
            match self.scan.kind(index) {
                Some(K::Word) if matches!(word, "section" | "partition") => {
                    let section = self.scan.text(index + 1).to_owned();
                    self.bump(T::Section, &format!("{what}: {word} {section}"));
                }
                Some(K::Word) if word == "comdat" => self.bump(T::Comdat, &format!("on {what}")),
                Some(K::Word) if word == "align" && what == "variable" => {
                    let align = self.scan.text(index + 1).to_owned();
                    self.bump(T::Align, &format!("global variable align {align}"));
                }
                Some(K::Word)
                    if what == "variable"
                        && (word.starts_with("sanitize")
                            || word.starts_with("no_sanitize")
                            || word == "code_model") =>
                {
                    self.bump(T::GlobalKind, &format!("with {word}"));
                }
                Some(K::Meta)
                    if what == "variable" && !word[1..].bytes().all(|b| b.is_ascii_digit()) =>
                {
                    self.bump(T::MdAttachGlobal, word);
                }
                _ => {}
            }
            index += 1;
        }
    }

    fn body_statement(&mut self, from: usize, to: usize) {
        let first_kind = self.scan.kind(from);
        // A label.
        if to == from + 2
            && self.scan.is(from + 1, ":")
            && matches!(first_kind, Some(K::Word | K::Num | K::Str))
        {
            self.blocks += 1;
            self.fn_blocks += 1;
            return;
        }
        let first = self.scan.text(from);
        if first_kind == Some(K::Word) && first.starts_with("#dbg_") {
            self.bump(T::DebugRecord, &format!("{first} (record)"));
            return;
        }
        let op_at = if first_kind == Some(K::Local) && self.scan.is(from + 1, "=") {
            from + 2
        } else {
            from
        };
        if self.scan.kind(op_at) != Some(K::Word) {
            self.bump(T::Unrecognised, &format!("in a body: {first}"));
            return;
        }
        // The entry block has no label line.
        if self.fn_blocks == 0 {
            self.blocks += 1;
            self.fn_blocks = 1;
        }
        self.instructions += 1;
        self.fn_instructions += 1;
        let mut op_at = op_at;
        let mut opcode = self.scan.text(op_at);
        if matches!(opcode, "tail" | "musttail" | "notail") && self.scan.is_word(op_at + 1, "call")
        {
            self.bump(T::CallKind, &format!("{opcode} call"));
            op_at += 1;
            opcode = "call";
        }
        self.bump(T::Opcode, opcode);
        self.attachments(op_at + 1, to, T::MdAttachInstr);
        self.mentions(op_at, to, "operand");
        self.instruction(opcode, op_at + 1, to);
    }

    /// The words at `from..` that are not a type, counted as flags of
    /// `opcode`; returns them and the index of the type.
    fn flags(&mut self, opcode: &str, from: usize, to: usize) -> (Vec<&'a str>, usize) {
        let mut index = from;
        let mut words = Vec::new();
        while index < to && self.scan.kind(index) == Some(K::Word) && !self.scan.starts_type(index)
        {
            let word = self.scan.text(index);
            if word == "syncscope" && self.scan.is(index + 1, "(") {
                index = self.scan.close(index + 1, to);
                self.bump(T::InstrFlag, &format!("{opcode} syncscope(...)"));
                continue;
            }
            words.push(word);
            index += 1;
        }
        (words, index)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one case after another; splitting it would not make it clearer"
    )]
    fn instruction(&mut self, opcode: &'a str, from: usize, to: usize) {
        // Where the operands end and the attachments begin.
        let end = self
            .scan
            .find_flat(from, to, |at| {
                self.scan.is(at, ",") && self.scan.kind(at + 1) == Some(K::Meta) && {
                    let name = self.scan.text(at + 1);
                    !name[1..].bytes().all(|b| b.is_ascii_digit()) && !name.starts_with("!DI")
                }
            })
            .unwrap_or(to);
        match opcode {
            "call" | "invoke" | "callbr" => self.call(opcode, from, end),
            "ret" | "resume" | "freeze" | "extractvalue" | "insertvalue" | "extractelement"
            | "insertelement" | "shufflevector" | "indirectbr" => {
                let name = self.scan.type_name(from, end);
                self.bump(T::OpType, &format!("{opcode} {name}"));
                if opcode == "ret" {
                    self.by_value("returned by ret", from, end);
                }
                if opcode == "resume" {
                    self.bump(T::Eh, "resume");
                }
            }
            "br" => {
                let conditional = !self.scan.is_word(from, "label");
                let name = if conditional { "br i1 (conditional)" } else { "br label" };
                self.bump(T::OpType, name);
            }
            "switch" => {
                let name = self.scan.type_name(from, end);
                let labels =
                    (from..end).filter(|at| self.scan.is_word(*at, "label")).count().max(1) - 1;
                let bucket = match labels {
                    0 => "0 cases",
                    1..=4 => "1-4 cases",
                    5..=16 => "5-16 cases",
                    17..=64 => "17-64 cases",
                    _ => "65 or more cases",
                };
                self.bump(T::OpType, &format!("switch {name}"));
                self.bump(T::Switch, &format!("switch {name}, {bucket}"));
            }
            "unreachable" => {}
            "alloca" => {
                let (words, at) = self.flags(opcode, from, end);
                for word in words {
                    self.bump(T::InstrFlag, &format!("alloca {word}"));
                }
                let name = self.scan.type_name(at, end);
                self.bump(T::OpType, &format!("alloca {name}"));
                let parts = self.scan.split_commas(at, end);
                for &(start, stop) in parts.iter().skip(1) {
                    if self.scan.is_word(start, "align") {
                        let align = self.scan.text(start + 1).to_owned();
                        self.bump(T::Align, &format!("alloca align {align}"));
                    } else if self.scan.is_word(start, "addrspace") {
                        self.bump(T::InstrFlag, "alloca addrspace(...)");
                    } else if self.scan.starts_type(start) {
                        let constant = self.scan.kind(stop - 1) == Some(K::Num);
                        let count = if constant { "constant" } else { "variable" };
                        self.bump(T::InstrFlag, &format!("alloca with a {count} count"));
                    }
                }
            }
            "load" | "store" => {
                let (words, at) = self.flags(opcode, from, end);
                for word in words {
                    self.bump(T::InstrFlag, &format!("{opcode} {word}"));
                }
                let name = self.scan.type_name(at, end);
                self.bump(T::OpType, &format!("{opcode} {name}"));
                for (start, _) in self.scan.split_commas(at, end).into_iter().skip(1) {
                    let word = self.scan.text(start);
                    if word == "align" {
                        let align = self.scan.text(start + 1).to_owned();
                        self.bump(T::Align, &format!("{opcode} align {align}"));
                    }
                }
                for index in at..end {
                    let word = self.scan.text(index);
                    if self.scan.kind(index) == Some(K::Word) && ORDERINGS.contains(&word) {
                        self.bump(T::InstrFlag, &format!("{opcode} ordering {word}"));
                    }
                }
            }
            "getelementptr" => {
                let (words, mut at) = self.flags(opcode, from, end);
                for word in &words {
                    self.bump(T::InstrFlag, &format!("getelementptr {word}"));
                    if *word == "inrange" && self.scan.is(at, "(") {
                        at = self.scan.close(at, end);
                    }
                }
                if words.is_empty() {
                    self.bump(T::InstrFlag, "getelementptr (no flag)");
                }
                let name = match self.scan.parse_type(at, end) {
                    Some((Class::Struct, _)) => "struct".to_owned(),
                    Some((Class::Array, _)) => "array".to_owned(),
                    _ => self.scan.type_name(at, end),
                };
                self.bump(T::OpType, &format!("getelementptr {name}"));
                let parts = self.scan.split_commas(at, end);
                let indices = parts.len().saturating_sub(2);
                let constant =
                    parts.iter().skip(2).all(|&(_, stop)| self.scan.kind(stop - 1) == Some(K::Num));
                let how = if constant { "all literal" } else { "some variable" };
                self.bump(T::Gep, &format!("{name}, {indices} indices, {how}"));
            }
            "icmp" | "fcmp" => {
                let (mut words, at) = self.flags(opcode, from, end);
                let predicate = words.pop().unwrap_or("?");
                for word in words {
                    self.bump(T::InstrFlag, &format!("{opcode} {word}"));
                }
                self.bump(T::Predicate, &format!("{opcode} {predicate}"));
                let name = self.scan.type_name(at, end);
                self.bump(T::OpType, &format!("{opcode} {name}"));
            }
            "phi" | "select" | "va_arg" => {
                let (words, mut at) = self.flags(opcode, from, end);
                for word in words {
                    self.bump(T::InstrFlag, &format!("{opcode} {word}"));
                }
                // The value type: for `select` and `va_arg` the second
                // operand's.
                if opcode != "phi"
                    && let Some(&(start, _)) = self.scan.split_commas(at, end).get(1)
                {
                    at = start;
                }
                let name = self.scan.type_name(at, end);
                self.bump(T::OpType, &format!("{opcode} {name}"));
            }
            "landingpad" => {
                self.bump(T::Eh, "landingpad");
                let name = self.scan.type_name(from, end);
                self.bump(T::OpType, &format!("landingpad {name}"));
                for at in from..end {
                    if self.scan.kind(at) != Some(K::Word) {
                        continue;
                    }
                    match self.scan.text(at) {
                        "cleanup" => self.bump(T::Eh, "landingpad clause: cleanup"),
                        "catch" => {
                            let all = self.scan.is_word(at + 2, "null");
                            let what =
                                if all { "catch null (catch-all)" } else { "catch <typeinfo>" };
                            self.bump(T::Eh, &format!("landingpad clause: {what}"));
                        }
                        "filter" => {
                            let empty = self.scan.text(at + 2) == "0";
                            let what = if empty { "filter, empty" } else { "filter, with types" };
                            self.bump(T::Eh, &format!("landingpad clause: {what}"));
                        }
                        _ => {}
                    }
                }
            }
            "atomicrmw" | "cmpxchg" | "fence" => {
                let (words, at) = self.flags(opcode, from, end);
                for word in words {
                    let ordering = if ORDERINGS.contains(&word) { "ordering " } else { "" };
                    self.bump(T::InstrFlag, &format!("{opcode} {ordering}{word}"));
                }
                let parts = self.scan.split_commas(at, end);
                if let Some(&(start, _)) = parts.get(1) {
                    let name = self.scan.type_name(start, end);
                    self.bump(T::OpType, &format!("{opcode} {name}"));
                }
                for at in at..end {
                    let word = self.scan.text(at);
                    if self.scan.kind(at) == Some(K::Word) && ORDERINGS.contains(&word) {
                        self.bump(T::InstrFlag, &format!("{opcode} ordering {word}"));
                    }
                }
            }
            "catchswitch" | "catchpad" | "cleanuppad" | "catchret" | "cleanupret" => {
                self.bump(T::Eh, opcode);
            }
            _ if BINARY.contains(&opcode) => {
                let (words, at) = self.flags(opcode, from, end);
                for word in words {
                    let flag = if FAST_MATH.contains(&word) { "(fast-math)" } else { "" };
                    self.bump(T::InstrFlag, format!("{opcode} {word} {flag}").trim_end());
                }
                let name = self.scan.type_name(at, end);
                self.bump(T::OpType, &format!("{opcode} {name}"));
            }
            _ if CASTS.contains(&opcode) => {
                let (words, at) = self.flags(opcode, from, end);
                for word in words {
                    self.bump(T::InstrFlag, &format!("{opcode} {word}"));
                }
                let source = self.scan.type_name(at, end);
                let target = self
                    .scan
                    .find_flat(at, end, |index| self.scan.is_word(index, "to"))
                    .map_or_else(|| "?".to_owned(), |index| self.scan.type_name(index + 1, end));
                self.bump(T::OpType, &format!("{opcode} {source}"));
                self.bump(T::Cast, &format!("{opcode} {source} to {target}"));
            }
            _ => self.bump(T::Unrecognised, &format!("instruction: {opcode}")),
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one case after another; splitting it would not make it clearer"
    )]
    fn call(&mut self, opcode: &str, from: usize, end: usize) {
        if opcode == "invoke" {
            self.bump(T::Eh, "invoke");
        }
        // Fast-math flags, the calling convention, return attributes.
        let mut index = from;
        let mut cconv = None;
        while index < end && self.scan.kind(index) == Some(K::Word) {
            let word = self.scan.text(index);
            if FAST_MATH.contains(&word) {
                self.bump(T::InstrFlag, &format!("{opcode} {word} (fast-math)"));
            } else if word == "cc" && self.scan.kind(index + 1) == Some(K::Num) {
                cconv = Some(format!("cc {}", self.scan.text(index + 1)));
                index += 1;
            } else if is_cconv(word) {
                cconv = Some(word.to_owned());
            } else if word == "addrspace" && self.scan.is(index + 1, "(") {
                index = self.scan.close(index + 1, end);
                continue;
            } else {
                break;
            }
            index += 1;
        }
        let cconv = cconv.unwrap_or_else(|| "ccc (default)".to_owned());
        self.bump(T::CallConv, &format!("call site: {cconv}"));
        let index = self.attributes(index, end, T::RetAttr, false);
        let Some((_, mut callee)) = self.scan.parse_type(index, end) else {
            self.bump(T::Unrecognised, &format!("{opcode} without a type"));
            return;
        };
        let name = self.scan.type_name(index, end);
        self.bump(T::OpType, &format!("{opcode} {name}"));
        self.by_value(&format!("{opcode} result"), index, end);
        if self.scan.is(callee, "(") {
            // An explicit function type: `call i32 (ptr, ...) @printf(...)`.
            let close = self.scan.close(callee, end);
            let variadic = (callee..close).any(|at| self.scan.is_word(at, "..."));
            let how = if variadic { "variadic" } else { "not variadic" };
            self.bump(T::CallKind, &format!("{opcode} with an explicit function type, {how}"));
            callee = close;
        }
        let callee_text = self.scan.text(callee);
        let mut args_at = callee + 1;
        match self.scan.kind(callee) {
            Some(K::Global) if callee_text.starts_with("@llvm.") => {
                let folded = fold_intrinsic(&callee_text[1..]).to_owned();
                self.bump(T::CallKind, &format!("{opcode} of an intrinsic"));
                self.bump(T::Intrinsic, &folded);
                if folded.starts_with("llvm.dbg.") {
                    self.bump(T::DebugRecord, &format!("{folded} (intrinsic call)"));
                }
            }
            Some(K::Global) => self.bump(T::CallKind, &format!("{opcode} direct")),
            Some(K::Local) => self.bump(T::CallKind, &format!("{opcode} indirect")),
            Some(K::Word) if callee_text == "asm" => {
                self.bump(T::CallKind, &format!("{opcode} of inline assembly"));
                let mut at = callee + 1;
                let mut marks = Vec::new();
                while self.scan.kind(at) == Some(K::Word) {
                    marks.push(self.scan.text(at));
                    at += 1;
                }
                let cut = |text: &str| {
                    let mut end = text.len().min(60);
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    text[..end].to_owned()
                };
                let body = cut(self.scan.text(at));
                let constraints = cut(self.scan.text(at + 2));
                marks.insert(0, "asm");
                self.bump(
                    T::InlineAsm,
                    &format!("{} {body}, constraints {constraints}", marks.join(" ")),
                );
                args_at = at + 3;
            }
            _ => {
                self.bump(T::CallKind, &format!("{opcode} through a constant expression"));
                args_at = self
                    .scan
                    .find_flat(callee, end, |at| at > callee && self.scan.is(at, "("))
                    .unwrap_or(end);
                if self.scan.is(callee + 1, "(") {
                    args_at = self.scan.close(callee + 1, end);
                }
            }
        }
        if !self.scan.is(args_at, "(") {
            return;
        }
        let close = self.scan.close(args_at, end);
        for (start, stop) in self.scan.split_commas(args_at + 1, close - 1) {
            let Some((class, after)) = self.scan.parse_type(start, stop) else { continue };
            if class == Class::Metadata {
                continue;
            }
            self.by_value(&format!("{opcode} argument"), start, stop);
            self.attributes(after, stop, T::CallArgAttr, true);
        }
        let mut index = self.attributes(close, end, T::CallAttr, false);
        if self.scan.is(index, "[") {
            let bundles = self.scan.close(index, end);
            for at in index..bundles {
                if self.scan.kind(at) == Some(K::Str) && self.scan.is(at + 1, "(") {
                    let tag = self.scan.text(at).to_owned();
                    self.bump(T::Bundle, &tag);
                }
            }
            index = bundles;
        }
        let _ = index;
    }
}

impl Counter<'_, '_> {
    /// Records the sizes of the function whose body just ended, if one
    /// is open.
    fn end_function(&mut self) {
        if self.open {
            self.open = false;
            self.census.sample(D::BlocksPerFunction, self.fn_blocks);
            self.census.sample(D::InstructionsPerFunction, self.fn_instructions);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = r#"source_filename = "a.c"
target datalayout = "e-m:e-i64:64"
target triple = "aarch64-unknown-linux-gnu"

module asm ".globl helper"

%struct.P = type { i32, i32 }
%struct.Q = type { %struct.P, [2 x double] }
%opaque = type opaque

$inl = comdat any

@g = dso_local global i32 7, align 4
@t = internal thread_local(initialexec) global i64 0, align 8
@t2 = thread_local global i8 0
@str = private unnamed_addr constant [3 x i8] c"hi\00", align 1
@p = hidden constant ptr getelementptr inbounds ([3 x i8], ptr @str, i64 0, i64 1), section "mysec"
@ext = external global [0 x i32]
@w = weak_odr global %struct.P zeroinitializer, comdat($inl), !dbg !9
@al = alias i32, ptr @g
@llvm.global_ctors = appending global [1 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @init, ptr null }]
@f128 = global fp128 0xL00000000000000004000000000000000
@v = global <4 x i32> <i32 1, i32 2, i32 3, i32 4>

declare i32 @printf(ptr noundef, ...) #1
declare void @llvm.memcpy.p0.p0.i64(ptr noalias writeonly captures(none), ptr noalias readonly captures(none), i64, i1 immarg) #2
declare fastcc void @fast(i128)
declare void @llvm.dbg.declare(metadata, metadata, metadata)
declare void @may_throw()
declare ptr @id(ptr)
declare %struct.P @ret_agg([2 x i64])
declare void @init()
declare i32 @__gxx_personality_v0(...)
@_ZTIi = external constant ptr

define linkonce_odr hidden noundef i32 @inl(%struct.P %a, ptr byval(%struct.Q) align 8 %b, i64 %n, ptr %fp, [2 x i64] %arr) unnamed_addr #0 comdat personality ptr @__gxx_personality_v0 !dbg !5 {
entry:
  %x = alloca i32, align 4
  %vla = alloca i8, i64 %n, align 16
    #dbg_declare(ptr %x, !6, !DIExpression(), !7)
  call void @llvm.dbg.declare(metadata ptr %x, metadata !6, metadata !DIExpression()), !dbg !7
  store volatile i32 1, ptr %x, align 4, !nosanitize !11
  %l = load atomic i32, ptr %x seq_cst, align 4
  %a1 = add nsw i32 %l, 1
  %f = fadd fast double 1.000000e+00, 0x7FF8000000000000
  %c = icmp samesign ult i32 %a1, 10
  %fc = fcmp nnan oeq double %f, 0.000000e+00
  %z = zext nneg i32 %a1 to i64
  %gep = getelementptr inbounds nuw %struct.Q, ptr %b, i32 0, i32 1, i64 %z
  %sel = select i1 %c, i32 1, i32 2
  %call = call i32 (ptr, ...) @printf(ptr noundef @str, i32 noundef %sel) #3
  call void @llvm.memcpy.p0.p0.i64(ptr align 4 %x, ptr align 4 @g, i64 4, i1 false)
  %r = tail call fastcc i32 %fp(i32 1)
  %as = call i32 asm sideeffect "mov x0, x1", "=r,~{x0}"()
  %agg = call %struct.P @ret_agg([2 x i64] %arr)
  call void inttoptr (i64 16 to ptr)()
  invoke void @may_throw()
          to label %cont unwind label %lpad

cont:                                             ; preds = %entry
  switch i32 %sel, label %default [
    i32 1, label %one
    i32 2, label %one
  ]

one:
  %ph = phi i32 [ 1, %cont ], [ 1, %cont ]
  br i1 %c, label %default, label %default, !prof !8

lpad:
  %lp = landingpad { ptr, i32 }
          cleanup
          catch ptr null
          catch ptr @_ZTIi
          filter [0 x ptr] zeroinitializer
  resume { ptr, i32 } %lp

default:
  %ar = atomicrmw add ptr %x, i32 1 seq_cst, align 4
  %cx = cmpxchg weak ptr %x, i32 0, i32 1 acq_rel monotonic
  fence syncscope("singlethread") seq_cst
  %ev = extractvalue %struct.P %agg, 0
  ret i32 %ev
}

define ptr @mt(ptr %b) {
  %m = musttail call ptr @id(ptr %b)
  ret ptr %m
}

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="non-leaf" }
attributes #1 = { "target-cpu"="generic" }
attributes #2 = { nocallback nofree nounwind willreturn memory(argmem: readwrite) }
attributes #3 = { nounwind }

!llvm.module.flags = !{!0, !1}
!llvm.ident = !{!2}
!llvm.dbg.cu = !{!3}

!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 2, !"Debug Info Version", i32 3}
!2 = !{!"clang version 22"}
!3 = distinct !DICompileUnit(language: DW_LANG_C11, file: !4, emissionKind: FullDebug)
!4 = !DIFile(filename: "a.c", directory: "/x")
!5 = distinct !DISubprogram(name: "inl", unit: !3)
!6 = !DILocalVariable(name: "x", scope: !5)
!7 = !DILocation(line: 1, scope: !5)
!8 = !{!"branch_weights", i32 1, i32 2}
!9 = !DIGlobalVariableExpression(var: !10, expr: !DIExpression())
!10 = !DIGlobalVariable(name: "w")
!11 = !{}
"#;

    fn census(module: &str) -> GroupCensus {
        let mut census = GroupCensus::default();
        scan_module(&mut census, module);
        census
    }

    /// Every row of every table for [`MODULE`], as `table, count, row`.
    const ROWS: &str = r#"TopLevel	4	attributes group
TopLevel	1	comdat
TopLevel	9	declare
TopLevel	2	define
TopLevel	12	global variable, alias or ifunc
TopLevel	12	metadata node
TopLevel	1	module asm
TopLevel	3	named metadata
TopLevel	2	named type
TopLevel	1	named type, opaque
TopLevel	1	source_filename
TopLevel	1	target datalayout
TopLevel	1	target triple
Header	1	datalayout = "e-m:e-i64:64"
Header	1	ident = "clang version 22"
Header	1	triple = "aarch64-unknown-linux-gnu"
Opcode	1	add
Opcode	2	alloca
Opcode	1	atomicrmw
Opcode	1	br
Opcode	8	call
Opcode	1	cmpxchg
Opcode	1	extractvalue
Opcode	1	fadd
Opcode	1	fcmp
Opcode	1	fence
Opcode	1	getelementptr
Opcode	1	icmp
Opcode	1	invoke
Opcode	1	landingpad
Opcode	1	load
Opcode	1	phi
Opcode	1	resume
Opcode	2	ret
Opcode	1	select
Opcode	1	store
Opcode	1	switch
Opcode	1	zext
OpType	1	add i32
OpType	1	alloca i32
OpType	1	alloca i8
OpType	1	atomicrmw i32
OpType	1	br i1 (conditional)
OpType	3	call i32
OpType	1	call ptr
OpType	1	call struct
OpType	3	call void
OpType	1	cmpxchg i32
OpType	1	extractvalue struct
OpType	1	fadd double
OpType	1	fcmp double
OpType	1	getelementptr struct
OpType	1	icmp i32
OpType	1	invoke void
OpType	1	landingpad struct
OpType	1	load i32
OpType	1	phi i32
OpType	1	resume struct
OpType	1	ret i32
OpType	1	ret ptr
OpType	1	select i32
OpType	1	store i32
OpType	1	switch i32
OpType	1	zext i32
Cast	1	zext i32 to i64
Predicate	1	fcmp oeq
Predicate	1	icmp ult
InstrFlag	1	add nsw
InstrFlag	1	alloca with a variable count
InstrFlag	1	atomicrmw add
InstrFlag	1	atomicrmw ordering seq_cst
InstrFlag	1	cmpxchg ordering acq_rel
InstrFlag	1	cmpxchg ordering monotonic
InstrFlag	1	cmpxchg weak
InstrFlag	1	fadd fast (fast-math)
InstrFlag	1	fcmp nnan
InstrFlag	1	fence ordering seq_cst
InstrFlag	1	fence syncscope(...)
InstrFlag	1	getelementptr inbounds
InstrFlag	1	getelementptr nuw
InstrFlag	1	icmp samesign
InstrFlag	1	load atomic
InstrFlag	1	load ordering seq_cst
InstrFlag	1	store volatile
InstrFlag	1	zext nneg
Gep	1	struct, 3 indices, some variable
Align	1	alloca align 16
Align	1	alloca align 4
Align	1	global variable align 1
Align	1	global variable align 4
Align	1	global variable align 8
Align	1	load align 4
Align	1	store align 4
Switch	1	switch i32, 1-4 cases
CallKind	3	call direct
CallKind	1	call indirect
CallKind	2	call of an intrinsic
CallKind	1	call of inline assembly
CallKind	1	call through a constant expression
CallKind	1	call with an explicit function type, variadic
CallKind	1	invoke direct
CallKind	1	musttail call
CallKind	1	tail call
Intrinsic	1	llvm.dbg.declare
Intrinsic	1	llvm.memcpy
IntrinsicDecl	1	llvm.dbg.declare
IntrinsicDecl	1	llvm.memcpy
InlineAsm	1	asm sideeffect "mov x0, x1", constraints "=r,~{x0}"
ModuleAsm	1	.globl
CallConv	8	call site: ccc (default)
CallConv	1	call site: fastcc
CallConv	8	declare: ccc (default)
CallConv	1	declare: fastcc
CallConv	2	define: ccc (default)
Linkage	1	alias: external (default)
Linkage	9	declare: external (default)
Linkage	1	define: external (default)
Linkage	1	define: linkonce_odr
Linkage	1	variable: appending
Linkage	2	variable: external
Linkage	5	variable: external (default)
Linkage	1	variable: internal
Linkage	1	variable: private
Linkage	1	variable: weak_odr
Visibility	1	define: hidden
Visibility	1	define: unnamed_addr
Visibility	1	variable: dso_local
Visibility	1	variable: hidden
Visibility	1	variable: unnamed_addr
ThreadLocal	1	thread_local (general dynamic)
ThreadLocal	1	thread_local(initialexec)
Comdat	1	comdat any
Comdat	1	on define
Comdat	1	on variable
GlobalKind	1	@llvm.global_ctors
GlobalKind	1	alias
GlobalKind	1	constant array
GlobalKind	1	constant ptr
GlobalKind	1	constant ptr, declaration
GlobalKind	1	global <4 x i32>
GlobalKind	1	global array
GlobalKind	1	global array, declaration
GlobalKind	1	global fp128
GlobalKind	1	global i32
GlobalKind	1	global i64
GlobalKind	1	global i8
GlobalKind	1	global struct
Section	1	variable: section "mysec"
Signature	1	declare parameter array
Signature	1	declare parameter i1
Signature	1	declare parameter i128
Signature	1	declare parameter i64
Signature	3	declare parameter metadata
Signature	4	declare parameter ptr
Signature	2	declare returns i32
Signature	1	declare returns ptr
Signature	1	declare returns struct
Signature	5	declare returns void
Signature	2	declare variadic
Signature	3	declare with 0 parameters
Signature	4	declare with 1 parameters
Signature	1	declare with 3 parameters
Signature	1	declare with 4 parameters
Signature	1	define parameter array
Signature	1	define parameter i64
Signature	3	define parameter ptr
Signature	1	define parameter struct
Signature	1	define returns i32
Signature	1	define returns ptr
Signature	1	define with 1 parameters
Signature	1	define with 5 parameters
FnAttr	1	"frame-pointer"="non-leaf"
FnAttr	1	noinline
FnAttr	1	nounwind
FnAttr	1	optnone
FnAttr	1	uwtable
FnAttrDecl	1	"target-cpu"="generic"
FnAttrDecl	1	memory(argmem: readwrite)
FnAttrDecl	1	nocallback
FnAttrDecl	1	nofree
FnAttrDecl	1	nounwind
FnAttrDecl	1	willreturn
ParamAttr	1	align 8
ParamAttr	1	byval(T)
ParamAttr	2	captures(none)
ParamAttr	1	immarg
ParamAttr	2	noalias
ParamAttr	1	noundef
ParamAttr	1	readonly
ParamAttr	1	writeonly
RetAttr	1	noundef
CallAttr	1	nounwind
CallArgAttr	2	align 4
CallArgAttr	2	noundef
IntWidth	4	i1
IntWidth	1	i128
IntWidth	41	i32
IntWidth	13	i64
IntWidth	4	i8
TypeMention	1	array of a struct
TypeMention	1	array of double
TypeMention	1	array of i32
TypeMention	3	array of i64
TypeMention	2	array of i8
TypeMention	1	array of ptr
TypeMention	3	double
TypeMention	1	fp128
TypeMention	38	ptr
VectorTy	1	<4 x i32>
AggByValue	1	call argument: [2 x i64]
AggByValue	1	call result: { i32, i32 }
AggByValue	1	declare parameter: [2 x i64]
AggByValue	1	declare result: { i32, i32 }
AggByValue	1	define parameter: [2 x i64]
AggByValue	1	define parameter: { i32, i32 }
ConstExpr	1	initializer: getelementptr
ConstExpr	1	operand: inttoptr
ConstKind	2	initializer: address of a global or function
ConstKind	1	initializer: c"..." string
ConstKind	1	initializer: floating-point literal, 0xL (fp128)
ConstKind	1	initializer: null
ConstKind	1	initializer: zeroinitializer
ConstKind	1	operand: floating-point literal, 0x... (hexadecimal double)
ConstKind	2	operand: floating-point literal, decimal
ConstKind	1	operand: null
ConstKind	1	operand: zeroinitializer
Eh	1	invoke
Eh	1	landingpad
Eh	1	landingpad clause: catch <typeinfo>
Eh	1	landingpad clause: catch null (catch-all)
Eh	1	landingpad clause: cleanup
Eh	1	landingpad clause: filter, empty
Eh	1	personality @__gxx_personality_v0
Eh	1	resume
ModuleFlag	1	"Debug Info Version" = i32 3 (behavior 2)
ModuleFlag	1	"PIC Level" = i32 2 (behavior 8)
NamedMetadata	1	!llvm.dbg.cu
NamedMetadata	1	!llvm.ident
NamedMetadata	1	!llvm.module.flags
MdAttachInstr	1	!dbg
MdAttachInstr	1	!nosanitize
MdAttachInstr	1	!prof
MdAttachGlobal	2	!dbg
DebugRecord	1	#dbg_declare (record)
DebugRecord	1	llvm.dbg.declare (intrinsic call)
DiNode	1	!DICompileUnit
DiNode	1	!DIFile
DiNode	1	!DIGlobalVariable
DiNode	1	!DIGlobalVariableExpression
DiNode	1	!DILocalVariable
DiNode	1	!DILocation
DiNode	1	!DISubprogram
DiNode	5	tuple !{...}
Dwarf	1	DW_LANG_C11
Dwarf	1	emissionKind: FullDebug"#;

    #[test]
    fn every_table_of_a_module() {
        let census = census(MODULE);
        let mut rows = Vec::new();
        for table in T::ALL {
            for key in census.keys(table) {
                rows.push(format!("{table:?}\t{}\t{key}", census.count(table, key)));
            }
        }
        let wanted: Vec<&str> = ROWS.lines().collect();
        for row in &rows {
            assert!(wanted.contains(&row.as_str()), "not expected: {row}");
        }
        for row in &wanted {
            assert!(rows.iter().any(|have| have == row), "missing: {row}");
        }
        // Every table but the three this module has nothing for.
        for table in T::ALL {
            let none = matches!(table, T::Bundle | T::Unrecognised);
            assert_eq!(census.keys(table).is_empty(), none, "{table:?}");
        }
        let sizes: Vec<(D, u64, u64)> =
            D::ALL.iter().map(|d| (*d, census.summary(*d).sum, census.summary(*d).max)).collect();
        assert_eq!(
            sizes,
            [
                (D::ModuleBytes, MODULE.len() as u64, MODULE.len() as u64),
                (D::ModuleLines, 111, 111),
                (D::Definitions, 2, 2),
                (D::Declarations, 9, 9),
                (D::GlobalVariables, 11, 11),
                (D::BlocksPerModule, 6, 6),
                (D::InstructionsPerModule, 31, 31),
                (D::MetadataNodes, 12, 12),
                (D::BlocksPerFunction, 6, 5),
                (D::InstructionsPerFunction, 31, 29),
            ]
        );
    }

    #[test]
    fn tokens() {
        let source = "@\"a ; b\" = constant [4 x i8] c\"r;t\\00\", align 1 ; ret void\n\
                      %\"x y\" = type { i32 }\n  %1 = add i32 -7, 1.5e+10 ; c\n!0 = !{!\"s\", !1}\n\
                        #dbg_value(i32 0, !1, !DIExpression(), !2)\n$c = comdat any #3 0x7FF0000000000000";
        let toks = lex(source);
        let seen: Vec<(K, &str)> = toks.iter().map(|tok| (tok.kind, tok.text(source))).collect();
        let wanted = [
            (K::Global, "@\"a ; b\""),
            (K::Punct, "="),
            (K::Word, "constant"),
            (K::Punct, "["),
            (K::Num, "4"),
            (K::Word, "x"),
            (K::Word, "i8"),
            (K::Punct, "]"),
            (K::CStr, "c\"r;t\\00\""),
            (K::Punct, ","),
            (K::Word, "align"),
            (K::Num, "1"),
            (K::Nl, "\n"),
            (K::Local, "%\"x y\""),
            (K::Punct, "="),
            (K::Word, "type"),
            (K::Punct, "{"),
            (K::Word, "i32"),
            (K::Punct, "}"),
            (K::Nl, "\n"),
            (K::Local, "%1"),
            (K::Punct, "="),
            (K::Word, "add"),
            (K::Word, "i32"),
            (K::Num, "-7"),
            (K::Punct, ","),
            (K::Num, "1.5e+10"),
            (K::Nl, "\n"),
            (K::Meta, "!0"),
            (K::Punct, "="),
            (K::Bang, "!"),
            (K::Punct, "{"),
            (K::MetaStr, "!\"s\""),
            (K::Punct, ","),
            (K::Meta, "!1"),
            (K::Punct, "}"),
            (K::Nl, "\n"),
            (K::Word, "#dbg_value"),
            (K::Punct, "("),
            (K::Word, "i32"),
            (K::Num, "0"),
            (K::Punct, ","),
            (K::Meta, "!1"),
            (K::Punct, ","),
            (K::Meta, "!DIExpression"),
            (K::Punct, "("),
            (K::Punct, ")"),
            (K::Punct, ","),
            (K::Meta, "!2"),
            (K::Punct, ")"),
            (K::Nl, "\n"),
            (K::Comdat, "$c"),
            (K::Punct, "="),
            (K::Word, "comdat"),
            (K::Word, "any"),
            (K::AttrGroup, "#3"),
            (K::Num, "0x7FF0000000000000"),
        ];
        assert_eq!(seen, wanted);
        // An unterminated string and a lone sigil end the text quietly.
        assert_eq!(lex("\"abc").len(), 1);
        assert_eq!(lex("@ % é").iter().map(|tok| tok.kind).collect::<Vec<_>>(), [K::Punct; 3]);
    }

    /// Code inside comments, strings and quoted names is not counted.
    #[test]
    fn strings_comments_and_quoted_names() {
        let module = "\
; define void @commented() {\n\
;   call void @llvm.trap()\n\
; }\n\
@\"call void @x() ; ret\" = private constant [22 x i8] c\"  store i32 0, ptr @g\\00\", align 1 ; \"open\n\
@\"define i32 @y\" = global ptr @\"call void @x() ; ret\", section \"; landingpad { ptr }\"\n\
define void @\"invoke \\22 f\"(ptr %\"load i64\") \"a;b\"=\"switch (\" {\n\
\"br label ; x\":                                   ; preds = %0\n\
  %\"add i32 ; 1\" = load ptr, ptr %\"load i64\", align 8 ; store ptr null\n\
  call void asm \"ret ; mov x0, x1 }\", \"~{x0}\"() ; call void @z()\n\
  ret void ; }\n\
}\n";
        let census = census(module);
        let opcodes: Vec<(&str, u64)> = census
            .keys(T::Opcode)
            .into_iter()
            .map(|key| (key, census.count(T::Opcode, key)))
            .collect();
        assert_eq!(opcodes, [("call", 1), ("load", 1), ("ret", 1)]);
        assert_eq!(census.keys(T::TopLevel), ["define", "global variable, alias or ifunc"]);
        assert_eq!(census.count(T::TopLevel, "define"), 1);
        assert_eq!(census.count(T::TopLevel, "global variable, alias or ifunc"), 2);
        assert_eq!(census.keys(T::Intrinsic), [""; 0]);
        assert_eq!(census.keys(T::Eh), [""; 0]);
        assert_eq!(census.keys(T::Unrecognised), [""; 0]);
        assert_eq!(census.keys(T::Section), ["variable: section \"; landingpad { ptr }\""]);
        assert_eq!(census.keys(T::FnAttr), ["\"a;b\"=\"switch (\""]);
        assert_eq!(
            census.keys(T::InlineAsm),
            ["asm \"ret ; mov x0, x1 }\", constraints \"~{x0}\""]
        );
        assert_eq!(census.keys(T::IntWidth), ["i8"]);
        assert_eq!(census.count(T::ConstKind, "initializer: c\"...\" string"), 1);
        assert_eq!(census.count(T::ConstKind, "initializer: address of a global or function"), 1);
        assert_eq!(census.summary(D::BlocksPerModule).sum, 1);
        assert_eq!(census.summary(D::InstructionsPerModule).sum, 3);
    }

    #[test]
    fn intrinsic_names_are_folded() {
        for (name, folded) in [
            ("llvm.memcpy.p0.p0.i64", "llvm.memcpy"),
            ("llvm.memcpy.inline.p0.p0.i32", "llvm.memcpy.inline"),
            ("llvm.va_start.p0", "llvm.va_start"),
            ("llvm.va_start", "llvm.va_start"),
            ("llvm.is.fpclass.f128", "llvm.is.fpclass"),
            ("llvm.eh.typeid.for.p0", "llvm.eh.typeid.for"),
            ("llvm.vector.reduce.add.v4i32", "llvm.vector.reduce.add"),
            ("llvm.masked.load.nxv2f64.p0", "llvm.masked.load"),
            ("llvm.fptosi.sat.i32.f64", "llvm.fptosi.sat"),
            ("llvm.fabs.ppcf128", "llvm.fabs"),
            ("llvm.fabs.bf16", "llvm.fabs"),
            ("llvm.ssa.copy.s_struct.Ps", "llvm.ssa.copy.s_struct.Ps"),
            ("llvm.threadlocal.address.p0", "llvm.threadlocal.address"),
            ("llvm.aarch64.neon.ld1x2.v16i8.p0", "llvm.aarch64.neon.ld1x2"),
            ("llvm.trap", "llvm.trap"),
            ("llvm.i32", "llvm.i32"),
        ] {
            assert_eq!(fold_intrinsic(name), folded, "{name}");
        }
    }

    #[test]
    fn rarer_constructs() {
        let module = "\
@a = addrspace(1) global i32 0\n\
@i = ifunc void (), ptr @resolver\n\
declare !dbg !3 cc 10 void @ghc(<vscale x 2 x i64>, ptr addrspace(1)) gc \"statepoint-example\"\n\
declare aarch64_vector_pcs void @pcs() prefix i32 123\n\
define void @f(ptr %p) personality ptr @pers {\n\
  call void @g() [ \"deopt\"(i32 1), \"funclet\"(token none) ]\n\
  invoke cc 10 void @g() to label %ok unwind label %bad\n\
ok:\n\
  %r = notail call zeroext i8 @h(ptr nonnull sret({ i32 }) align 4 %p, { i64, i64 } zeroinitializer) nounwind memory(none)\n\
  %v = load <vscale x 2 x i64>, ptr %p\n\
  %c = callbr i32 asm \"\", \"!i\"() to label %ok [label %bad]\n\
  ret void\n\
bad:\n\
  %pad = cleanuppad within none []\n\
  cleanupret from %pad unwind to caller\n\
}\n\
this is not IR\n\
uselistorder ptr @f, { 1, 0 }\n";
        let census = census(module);
        let has = |table: T, key: &str| assert_eq!(census.count(table, key), 1, "{table:?} {key}");
        has(T::GlobalKind, "variable in addrspace(1)");
        has(T::GlobalKind, "ifunc");
        has(T::Linkage, "ifunc: external (default)");
        has(T::CallConv, "declare: cc 10");
        has(T::CallConv, "declare: aarch64_vector_pcs");
        has(T::CallConv, "call site: cc 10");
        has(T::MdAttachGlobal, "!dbg");
        has(T::Signature, "declare gc \"statepoint-example\"");
        has(T::Signature, "declare with prefix data");
        has(T::Signature, "declare parameter <vscale x 2 x i64>");
        has(T::Signature, "declare parameter ptr addrspace(1)");
        has(T::TypeMention, "ptr addrspace(1)");
        assert_eq!(census.count(T::VectorTy, "<vscale x 2 x i64>"), 2);
        has(T::Bundle, "\"deopt\"");
        has(T::Bundle, "\"funclet\"");
        has(T::ConstKind, "operand: none");
        has(T::Eh, "personality @pers");
        has(T::Eh, "invoke");
        has(T::Eh, "cleanuppad");
        has(T::Eh, "cleanupret");
        has(T::CallKind, "notail call");
        has(T::CallKind, "callbr of inline assembly");
        has(T::RetAttr, "zeroext");
        has(T::CallArgAttr, "sret(T)");
        has(T::CallArgAttr, "nonnull");
        has(T::CallArgAttr, "align 4");
        has(T::AggByValue, "call argument: { i64, i64 }");
        has(T::CallAttr, "nounwind");
        has(T::CallAttr, "memory(none)");
        assert_eq!(census.keys(T::CallAttr).len(), 2, "an invoke's destinations are no attributes");
        has(T::OpType, "load <vscale x 2 x i64>");
        has(T::TopLevel, "uselistorder");
        has(T::Unrecognised, "top level: this");
        assert_eq!(census.keys(T::Unrecognised).len(), 1);
        assert_eq!(census.summary(D::BlocksPerModule).sum, 3);
        assert_eq!(census.summary(D::InstructionsPerModule).sum, 8);
    }

    /// A row's module count is the modules it occurs in, however often.
    #[test]
    fn module_counts() {
        let mut census = GroupCensus::default();
        scan_module(
            &mut census,
            "define void @f() {\n  ret void\n}\ndefine void @g() {\n  ret void\n}\n",
        );
        scan_module(&mut census, "define i32 @f() {\n  ret i32 0\n}\n");
        scan_module(&mut census, "");
        assert_eq!(census.modules, 3);
        let cell = census.cell(T::Opcode, "ret");
        assert_eq!((cell.count, cell.modules), (3, 2));
        let cell = census.cell(T::OpType, "ret void");
        assert_eq!((cell.count, cell.modules), (2, 1));
        assert_eq!(census.summary(D::Definitions).median, 1);
        assert_eq!(census.summary(D::InstructionsPerFunction).n, 3);
        let mut other = GroupCensus::default();
        scan_module(&mut other, "define void @h() {\n  ret void\n}\n");
        census.merge(other);
        let cell = census.cell(T::Opcode, "ret");
        assert_eq!((census.modules, cell.count, cell.modules), (4, 4, 3));
    }
}
