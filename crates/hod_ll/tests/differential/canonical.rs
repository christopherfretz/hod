//! `hod_ll` against LLVM's own reading of each module, the text
//! `llvm-as | llvm-dis` prints: in it every upgrade has been applied and
//! every function attribute stands in a group, one function header per
//! line. A scanner written for that regular text (not the parser under
//! test) reads the headers, and what `hod_ll` made of the module as
//! written has to agree with it.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use hod_ir::{
    AttrClass, AttrSyntax, CallConv, Const, ConstId, DiKind, FixupTarget, FnAttrs, FnStr, Linkage,
    Module, ParamAttrs, ParamMem, SymId, Symbol, SymbolKind, UnnamedAddr, UwTable, Visibility,
    attr_class,
};
use hod_ll::{FunctionText, Lexer, MdKind, ModuleText, ParsedModule, Tok, unescape};

use crate::common::{Case, corpus, parse, parsed};

/// `text` cut at each `separator` that is outside quotes and brackets;
/// empty pieces are left out.
fn split_top(text: &str, separator: u8) -> Vec<&str> {
    let bytes = text.as_bytes();
    let (mut pieces, mut start, mut depth, mut at) = (Vec::new(), 0, 0u32, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += 1;
                }
            }
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => depth = depth.saturating_sub(1),
            byte if byte == separator && depth == 0 => {
                pieces.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    pieces.push(&text[start.min(text.len())..]);
    pieces.into_iter().map(str::trim).filter(|piece| !piece.is_empty()).collect()
}

/// The bytes a name stands for: `"…"` unescaped, anything else as it is.
fn name_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    match text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        Some(inner) => unescape(inner.as_bytes(), &mut bytes),
        None => bytes.extend_from_slice(text.as_bytes()),
    }
    bytes
}

/// The number in `keyword(N)`, `keyword=N` or after the keyword.
fn number(text: &str) -> u64 {
    let digits: String =
        text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or_else(|_| panic!("a number in {text:?}"))
}

/// A function header of the canonical text, as the scanner reads it.
#[derive(Debug, Default)]
struct Header {
    name: String,
    defined: bool,
    /// The words before the name: linkage, visibility, calling
    /// convention, return attributes, return type.
    before: Vec<String>,
    params: Vec<String>,
    /// The words after the parameter list.
    after: Vec<String>,
}

fn headers(canonical: &str) -> Vec<Header> {
    let mut found = Vec::new();
    for line in canonical.lines() {
        let defined = line.starts_with("define ");
        if !defined && !line.starts_with("declare ") {
            continue;
        }
        let words = split_top(line, b' ');
        let at = words.iter().position(|word| word.starts_with('@')).expect("a function name");
        let word = words[at];
        // The name ends at the parenthesis that opens the parameters.
        let open = match word.strip_prefix("@\"") {
            Some(quoted) => 2 + quoted.find('"').expect("closing quote") + 1,
            None => word.find('(').expect("parameters"),
        };
        let params = &word[open + 1..word.len() - 1];
        found.push(Header {
            name: word[1..open].to_string(),
            defined,
            before: words[1..at].iter().map(ToString::to_string).collect(),
            params: split_top(params, b',').iter().map(ToString::to_string).collect(),
            after: words[at + 1..].iter().map(ToString::to_string).collect(),
        });
    }
    found
}

/// The body of each `attributes #N = { … }` line.
fn groups(canonical: &str) -> Vec<(u32, Vec<&str>)> {
    canonical
        .lines()
        .filter_map(|line| line.strip_prefix("attributes #"))
        .map(|rest| {
            let (id, body) = rest.split_once(" = { ").expect("a group body");
            let body = body.strip_suffix(" }").expect("a group's closing brace");
            (id.parse().expect("a group id"), split_top(body, b' '))
        })
        .collect()
}

/// What the scanner expects of a parameter's or the return value's
/// attributes: `words` are everything written for it.
fn expect_param(module: &Module, words: &[String], attrs: ParamAttrs, context: &str) {
    let flag_names = [
        ("zeroext", ParamAttrs::ZEROEXT),
        ("signext", ParamAttrs::SIGNEXT),
        ("inreg", ParamAttrs::INREG),
        ("nest", ParamAttrs::NEST),
        ("swiftself", ParamAttrs::SWIFT_SELF),
        ("swifterror", ParamAttrs::SWIFT_ERROR),
        ("swiftasync", ParamAttrs::SWIFT_ASYNC),
    ];
    let mem_names = [
        ("sret(", ParamMem::Sret),
        ("byval(", ParamMem::ByVal),
        ("byref(", ParamMem::ByRef),
        ("inalloca(", ParamMem::InAlloca),
        ("preallocated(", ParamMem::Preallocated),
        ("elementtype(", ParamMem::ElementType),
    ];
    for (name, flag) in flag_names {
        let written = words.iter().any(|word| word == name);
        assert_eq!(attrs.has(flag), written, "{context}: {name}");
    }
    let mem = mem_names
        .iter()
        .find(|(prefix, _)| words.iter().any(|word| word.starts_with(prefix)))
        .map_or(ParamMem::None, |&(_, mem)| mem);
    assert_eq!(attrs.mem(), mem, "{context}: memory attribute");
    assert_eq!(attrs.mem_type().is_some(), mem != ParamMem::None, "{context}: memory type");
    if let Some(ty) = attrs.mem_type()
        && words.iter().any(|word| word.ends_with("(i32)") || word.ends_with("(i64)"))
    {
        let size = if words.iter().any(|word| word.ends_with("(i32)")) { 4 } else { 8 };
        assert_eq!(module.types.alloc_size_of(ty), size, "{context}: memory type size");
    }
    let align = words.iter().position(|word| word == "align").map(|at| number(&words[at + 1]));
    assert_eq!(attrs.align(), align, "{context}: align");
    let stack = words.iter().find(|word| word.starts_with("alignstack(")).map(|word| number(word));
    assert_eq!(attrs.stack_align(), stack, "{context}: alignstack");
}

/// What the scanner expects of the function attributes, from the groups
/// the header names.
fn expect_fn_attrs(
    parsed: &ParsedModule<'_>,
    sym: SymId,
    group_words: &[&str],
    attrs: hod_ir::FnAttrsId,
    context: &str,
) {
    let view = parsed.module.fn_attrs(attrs);
    let (mut flags, mut uwtable, mut stack) = (0, UwTable::None, None);
    let mut strings: BTreeSet<(String, Vec<u8>)> = BTreeSet::new();
    let mut rejected: BTreeSet<String> = BTreeSet::new();
    for word in group_words {
        if let Some(rest) = word.strip_prefix('"') {
            let (name, value) = match rest.split_once("\"=\"") {
                Some((name, value)) => (name, value.strip_suffix('"').expect("closing quote")),
                None => (rest.strip_suffix('"').expect("closing quote"), ""),
            };
            let (name, value) = (name_bytes(&format!("\"{name}\"")), name_bytes(value));
            match attr_class(AttrSyntax::String, &name) {
                AttrClass::Kept => {
                    // `no-builtin-<function>` keeps the function's name.
                    let (key, rest) = FnStr::from_name(&name).expect("a kept string has a key");
                    let value = if rest.is_empty() { value } else { rest.to_vec() };
                    strings.insert((key.name().to_string(), value));
                }
                AttrClass::Rejected => {
                    rejected.insert(String::from_utf8_lossy(&name).into_owned());
                }
                AttrClass::Dropped => {}
            }
            continue;
        }
        let keyword = word.split(['(', '=']).next().expect("a keyword");
        if keyword == "uwtable" {
            uwtable = if word.contains("sync") && !word.contains("async") {
                UwTable::Sync
            } else {
                UwTable::Async
            };
        } else if keyword == "alignstack" {
            stack = Some(number(word));
        } else if let Some(flag) = FnAttrs::flag_named(keyword.as_bytes()) {
            flags |= flag;
        } else if attr_class(AttrSyntax::Keyword, keyword.as_bytes()) == AttrClass::Rejected {
            rejected.insert(keyword.to_string());
        }
    }
    assert_eq!(view.flags(), flags, "{context}: flags of {view:?}");
    assert_eq!(view.uwtable(), uwtable, "{context}: uwtable");
    assert_eq!(view.stack_align(), stack, "{context}: alignstack");
    let kept: BTreeSet<(String, Vec<u8>)> =
        view.strings().map(|(key, value)| (key.name().to_string(), value.to_vec())).collect();
    assert_eq!(kept, strings, "{context}: string attributes");
    let recorded: BTreeSet<String> = parsed
        .module
        .unsupported()
        .filter(|unsupported| unsupported.symbol == Some(sym))
        .filter_map(|unsupported| unsupported.construct.strip_prefix("attr:"))
        .map(ToString::to_string)
        .collect();
    for name in &rejected {
        assert!(recorded.contains(name), "{context}: rejected attribute {name} is not recorded");
    }
}

fn linkage_of(word: &str) -> Option<Linkage> {
    Some(match word {
        "private" => Linkage::Private,
        "internal" => Linkage::Internal,
        "available_externally" => Linkage::AvailableExternally,
        "linkonce" => Linkage::LinkOnceAny,
        "linkonce_odr" => Linkage::LinkOnceOdr,
        "weak" => Linkage::WeakAny,
        "weak_odr" => Linkage::WeakOdr,
        "extern_weak" => Linkage::ExternWeak,
        _ => return None,
    })
}

/// What stands before the name in a header: linkage, visibility,
/// `dso_local` and the calling convention.
fn expect_prefix(symbol: Symbol<'_>, cc: CallConv, before: &[String], context: &str) {
    let linkage = before.iter().find_map(|word| linkage_of(word)).unwrap_or(Linkage::External);
    assert_eq!(symbol.linkage(), linkage, "{context}: linkage");
    let visibility = if before.iter().any(|word| word == "hidden") {
        Visibility::Hidden
    } else if before.iter().any(|word| word == "protected") {
        Visibility::Protected
    } else {
        Visibility::Default
    };
    assert_eq!(symbol.visibility(), visibility, "{context}: visibility");
    let implied = linkage.is_local() || visibility != Visibility::Default;
    let dso_local = before.iter().any(|word| word == "dso_local") || implied;
    assert_eq!(symbol.dso_local(), dso_local, "{context}: dso_local");
    let numbered = |word: &&String| {
        word.strip_prefix("cc")
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
    };
    let convention = before
        .iter()
        .find(numbered)
        .map(|word| number(word) as u32)
        .or_else(|| {
            // The keywords `hod_ir` has, and the x86 ones LLVM prints
            // besides.
            let more = [(78, "x86_64_sysvcc"), (79, "win64cc"), (92, "x86_regcallcc")];
            (0..=u32::from(CallConv::MAX)).find(|&id| {
                let keyword = CallConv::from_llvm(id).expect("in range").keyword();
                let keyword =
                    keyword.or_else(|| more.iter().find(|(n, _)| *n == id).map(|&(_, word)| word));
                keyword.is_some_and(|keyword| before.iter().any(|word| word == keyword))
            })
        })
        .unwrap_or(0);
    assert_eq!(u32::from(cc.llvm_id()), convention, "{context}: calling convention");
}

/// What stands after the parameters in a header, apart from the
/// attribute groups: `unnamed_addr`, alignment, section, COMDAT and
/// personality.
fn expect_suffix(
    module: &Module,
    symbol: Symbol<'_>,
    personality: Option<SymId>,
    header: &Header,
    context: &str,
) {
    let after = &header.after;
    let unnamed = if after.iter().any(|word| word == "unnamed_addr") {
        UnnamedAddr::Global
    } else if after.iter().any(|word| word == "local_unnamed_addr") {
        UnnamedAddr::Local
    } else {
        UnnamedAddr::None
    };
    assert_eq!(symbol.unnamed_addr(), unnamed, "{context}: unnamed_addr");
    let following = |keyword: &str| {
        after.iter().position(|word| word == keyword).map(|at| after[at + 1].as_str())
    };
    assert_eq!(symbol.align(), following("align").map(number), "{context}: align");
    let section = following("section").map(name_bytes);
    assert_eq!(symbol.section().map(<[u8]>::to_vec), section, "{context}: section");
    let comdat = after.iter().find(|word| word.starts_with("comdat")).map(|word| {
        match word.strip_prefix("comdat($") {
            Some(name) => name_bytes(name.strip_suffix(')').expect("closing parenthesis")),
            None => name_bytes(&header.name),
        }
    });
    let mine = symbol.comdat().map(|id| module.comdat(id).name.to_vec());
    assert_eq!(mine, comdat, "{context}: comdat");
    let expected = after
        .iter()
        .position(|word| word == "personality")
        .map(|at| name_bytes(&after[at + 2][1..]));
    let mine = personality.map(|sym| module.symbol(sym).name().to_vec());
    assert_eq!(mine, expected, "{context}: personality");
}

/// One header of the canonical text against the module. False for a
/// function without a name: LLVM renumbers those.
fn expect_header(
    case: &Case,
    parsed: &ParsedModule<'_>,
    groups: &[(u32, Vec<&str>)],
    header: &Header,
) -> bool {
    let context = format!("{}: @{}", case.name, header.name);
    if header.name.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let sym = parsed
        .module
        .find_symbol(&name_bytes(&header.name))
        .unwrap_or_else(|| panic!("{context}: not a symbol"));
    let symbol = parsed.module.symbol(sym);
    let SymbolKind::Function { cc, attrs, ret_attrs, personality, has_body, .. } = symbol.kind()
    else {
        panic!("{context}: not a function");
    };
    assert_eq!(has_body, header.defined, "{context}: defined");
    expect_prefix(symbol, cc, &header.before, &context);
    expect_param(&parsed.module, &header.before, ret_attrs, &format!("{context}: return"));

    let variadic = header.params.last().is_some_and(|param| param == "...");
    let count = header.params.len() - usize::from(variadic);
    for (index, param) in header.params[..count].iter().enumerate() {
        let words: Vec<String> = split_top(param, b' ').iter().map(ToString::to_string).collect();
        let attrs = symbol.param_attrs(index);
        expect_param(&parsed.module, &words, attrs, &format!("{context}: param {index}"));
    }
    assert!(symbol.param_attrs(count).is_none(), "{context}: attributes past the end");

    let mut group_words = Vec::new();
    for word in &header.after {
        if let Some(id) = word.strip_prefix('#') {
            let id: u32 = id.parse().expect("a group id");
            let (_, words) = groups.iter().find(|(group, _)| *group == id).expect("group");
            group_words.extend_from_slice(words);
        }
    }
    expect_fn_attrs(parsed, sym, &group_words, attrs, &context);
    expect_suffix(&parsed.module, symbol, personality, header, &context);
    true
}

/// §5.6, §5.8, §7.3: every function header as the newest LLVM prints it
/// (linkage, visibility, `dso_local`, calling convention, return and
/// parameter attributes, the attribute groups with LLVM's upgrades
/// applied, `unnamed_addr`, section, COMDAT, alignment, personality)
/// against what `hod_ll` made of the module as written.
#[test]
fn function_headers_match_llvm_canonical_text() {
    let mut checked = 0;
    for case in corpus() {
        let parsed = parsed(case);
        let canonical = String::from_utf8_lossy(&case.upgraded);
        let groups = groups(&canonical);
        for header in headers(&canonical) {
            checked += usize::from(expect_header(case, &parsed, &groups, &header));
        }
    }
    println!("{checked} function headers checked against llvm-dis");
    assert!(corpus().is_empty() || checked > 10_000, "the corpus has thousands of functions");
}

fn display(module: &Module, sym: SymId, anonymous: &[SymId]) -> String {
    let symbol = module.symbol(sym);
    if symbol.is_anonymous() {
        let ordinal = anonymous.iter().position(|&other| other == sym).expect("a numbered global");
        return format!("<anonymous {ordinal}>");
    }
    String::from_utf8_lossy(symbol.name()).into_owned()
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn params_text(module: &Module, attrs: ParamAttrs) -> String {
    let size = attrs.mem_type().map(|ty| module.types.alloc_size_of(ty));
    format!(
        "{:?}/{size:?}/{:?}/{:?}/{}",
        attrs.mem(),
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

/// The image and fixups of an initializer, or why it has none.
fn image_text(module: &Module, init: ConstId, name: &dyn Fn(SymId) -> String) -> String {
    let (mut bytes, mut fixups) = (Vec::new(), Vec::new());
    if let Err(error) = module.consts.flatten(&module.types, init, &mut bytes, &mut fixups) {
        return format!(" no image: {:?}", error.kind);
    }
    let mut text = format!(" image={}/{:016x}", bytes.len(), hash(&bytes));
    for fixup in fixups {
        let target = match fixup.target {
            FixupTarget::Sym(sym) => name(sym),
            FixupTarget::Diff { a, b } => format!("{}-{}", name(a), name(b)),
            FixupTarget::PltDiff { a, b } => format!("plt({})-{}", name(a), name(b)),
            FixupTarget::Plt(sym) => format!("plt({})", name(sym)),
            FixupTarget::Block { func, block } => format!("block({}, {block:?})", name(func)),
            FixupTarget::BlockDiff { func, a, b } => {
                format!("block({}, {a:?})-block({}, {b:?})", name(func), name(func))
            }
        };
        let _ = write!(text, " [{}+{}: {target}{:+}]", fixup.offset, fixup.size, fixup.addend);
    }
    text
}

/// Everything `hod_ll` delivered for a module, as text that does not
/// depend on the order of the file: symbols by name with every field,
/// initializers as their image and fixups, and the module-level lists.
pub fn dump(parsed: &ParsedModule<'_>) -> String {
    let module = &parsed.module;
    let anonymous: Vec<SymId> = (0..4096).filter_map(|n| parsed.numbered_global(n)).collect();
    let name = |sym: SymId| display(module, sym, &anonymous);
    let mut lines = Vec::new();
    for symbol in module.symbols() {
        let mut line = format!(
            "{} {:?} {:?} dso_local={} {:?} section={:?} comdat={:?} align={:?} as={} tls={:?} \
             used={} compiler_used={} size={:?}",
            name(symbol.id()),
            symbol.linkage(),
            symbol.visibility(),
            symbol.dso_local(),
            symbol.unnamed_addr(),
            symbol.section().map(String::from_utf8_lossy),
            symbol.comdat().map(|id| String::from_utf8_lossy(module.comdat(id).name).into_owned()),
            symbol.align(),
            symbol.addr_space(),
            symbol.tls(),
            symbol.is_used(),
            symbol.is_compiler_used(),
            module.types.is_sized(symbol.ty()).then(|| module.types.alloc_size_of(symbol.ty())),
        );
        match symbol.kind() {
            SymbolKind::Unresolved => line.push_str(" unresolved"),
            SymbolKind::Variable { constant, init, .. } => {
                let _ = write!(line, " variable constant={constant}");
                if let Some(init) = init {
                    line.push_str(&image_text(module, init, &name));
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
                    params_text(module, ret_attrs),
                    param_attrs
                        .iter()
                        .map(|&attrs| params_text(module, attrs))
                        .collect::<Vec<_>>()
                        .join(", "),
                    personality.map(&name),
                );
            }
            SymbolKind::Alias { target, .. } => match module.consts.get(target) {
                Const::Addr { sym, addend } => {
                    let _ = write!(line, " alias of {}{addend:+}", name(sym));
                }
                other => {
                    let _ = write!(line, " alias of {other:?}");
                }
            },
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

/// §4, §5, §7: a module as written and the same module as `llvm-dis`
/// prints it (declarations reordered, attributes regrouped, constants
/// refolded, upgrades applied) deliver the same thing.
#[test]
fn the_canonical_text_delivers_the_same_module() {
    for case in corpus() {
        let written = dump(&parsed(case));
        let canonical = parse(&case.canonical, case.arch).expect("accepted");
        let canonical = dump(&canonical);
        if written != canonical {
            let theirs: BTreeSet<&str> = canonical.lines().collect();
            let mine: BTreeSet<&str> = written.lines().collect();
            let mut message = format!("{}: the two readings differ\n", case.name);
            for line in mine.difference(&theirs).take(6) {
                let _ = writeln!(message, "  as written:  {line}");
            }
            for line in theirs.difference(&mine).take(6) {
                let _ = writeln!(message, "  as printed:  {line}");
            }
            panic!("{message}");
        }
    }
}

/// The line-based oracle of function bodies: in the canonical text a
/// body opens with the `{` that ends a `define` line and closes with the
/// next line that is exactly `}`. The spans are of what is between the
/// braces.
fn body_lines(text: &[u8]) -> Vec<(usize, usize)> {
    let (mut spans, mut open, mut at) = (Vec::new(), None, 0);
    for line in text.split_inclusive(|&byte| byte == b'\n') {
        let content = line.strip_suffix(b"\n").unwrap_or(line);
        if open.is_none() && content.starts_with(b"define ") && content.ends_with(b"{") {
            open = Some(at + content.len());
        } else if content == b"}"
            && let Some(start) = open.take()
        {
            spans.push((start, at));
        }
        at += line.len();
    }
    spans
}

/// §4.2, §6: the recorded body spans are exactly the bodies, the
/// parameter spans exactly the parameter lists, and the functions come
/// in file order with their symbols.
#[test]
#[allow(clippy::cast_precision_loss)] // A size for display.
fn body_spans_are_exactly_the_bodies() {
    let (mut bodies, mut bytes) = (0, 0);
    for case in corpus() {
        // clang's output and llvm-dis's are both one header per line.
        let texts: &[&[u8]] =
            if case.from_clang { &[&case.text, &case.canonical] } else { &[&case.canonical] };
        for text in texts {
            let parsed = parse(text, case.arch).expect("accepted");
            let oracle = body_lines(text);
            assert_eq!(parsed.functions().len(), oracle.len(), "{}: definitions", case.name);
            for (function, &(start, end)) in parsed.functions().iter().zip(&oracle) {
                let body = function.body;
                assert_eq!((body.start as usize, body.end as usize), (start, end), "{}", case.name);
                assert_eq!(text[start - 1], b'{');
                assert_eq!(text[end], b'}');
                let params = function.params;
                assert_eq!(text[params.start as usize - 1], b'(', "{}: parameters", case.name);
                assert_eq!(text[params.end as usize], b')', "{}: parameters", case.name);
                let header = function.header.of(text);
                let define = &text[..function.header.start as usize];
                assert!(define.ends_with(b"define"), "{}: header", case.name);
                assert_eq!(function.header.end + 1, body.start, "{}: header end", case.name);
                let symbol = parsed.module.symbol(function.sym);
                let line = String::from_utf8_lossy(header);
                if !symbol.is_anonymous() && !symbol.name().starts_with(b"__unnamed_") {
                    let name = String::from_utf8_lossy(symbol.name()).into_owned();
                    let plain = format!("@{name}(");
                    assert!(
                        line.contains(&plain) || line.contains("@\""),
                        "{}: {name} is not the function of {line}",
                        case.name
                    );
                }
                let count = split_top(&String::from_utf8_lossy(params.of(text)), b',').len();
                let written = count - usize::from(params.of(text).ends_with(b"..."));
                let hod_ir::TypeKind::Function { params, .. } =
                    parsed.module.types.kind(symbol.ty())
                else {
                    panic!("{line}: not of a function type");
                };
                assert_eq!(params.len(), written, "{line}");
                bodies += 1;
                bytes += body.of(text).len();
            }
        }
    }
    println!("{bodies} bodies, {:.1} MiB of body text", bytes as f64 / 1_048_576.0);
}

fn assert_sync<T: Sync + Send>() {}

/// §6: what the body parser of WP-008 is handed: named types, attribute
/// groups and the metadata index, checked against line scans of the
/// canonical text; and the whole of it can be shared between threads.
#[test]
fn the_body_parser_interface_matches_the_text() {
    assert_sync::<ParsedModule<'static>>();
    assert_sync::<ModuleText<'static>>();
    for case in corpus() {
        let text = &case.canonical;
        let parsed = parse(text, case.arch).expect("accepted");
        let canonical = String::from_utf8_lossy(text);
        let (mut nodes, mut named, mut group_count, mut debug) = (0, 0, 0, false);
        for line in canonical.lines() {
            if let Some((name, rest)) = line.split_once(" = type ")
                && let Some(name) = name.strip_prefix('%')
            {
                let ty = match name.parse::<u32>() {
                    Ok(number) => parsed.numbered_type(number),
                    Err(_) => parsed.named_type(&name_bytes(name)),
                };
                let ty = ty.unwrap_or_else(|| panic!("{}: type %{name}", case.name));
                assert_eq!(parsed.module.types.is_sized(ty), rest != "opaque", "{line}");
            } else if let Some(rest) = line.strip_prefix("attributes #") {
                let id: u32 = rest.split(' ').next().expect("id").parse().expect("a group id");
                assert!(parsed.attr_group(id).is_some(), "{}: group #{id}", case.name);
                group_count += 1;
            } else if let Some(rest) = line.strip_prefix('!') {
                let (name, value) = rest.split_once(" = ").expect("a metadata definition");
                let Ok(id) = name.parse::<u32>() else {
                    assert!(
                        parsed
                            .metadata()
                            .named()
                            .iter()
                            .any(|entry| entry.name.of(text) == name.as_bytes()),
                        "{}: named metadata !{name}",
                        case.name
                    );
                    named += 1;
                    debug |= name == "llvm.dbg.cu";
                    continue;
                };
                let node = parsed.metadata().node(id).unwrap_or_else(|| panic!("node !{id}"));
                let distinct = value.starts_with("distinct ");
                let value = value.strip_prefix("distinct ").unwrap_or(value);
                assert_eq!(node.distinct, distinct, "{line}");
                let open = value.find(['{', '(']).expect("a node has braces or parentheses");
                let inner = &value[open + 1..value.len() - 1];
                assert_eq!(node.span.of(text), inner.as_bytes(), "{}: !{id}", case.name);
                let kind = match value.strip_prefix('!') {
                    Some(rest) if rest.starts_with('{') => MdKind::Tuple,
                    Some(rest) => {
                        let word = rest.split('(').next().expect("a node kind");
                        let kind = MdKind::from_name(word.as_bytes());
                        assert_ne!(kind, MdKind::Other, "{}: unknown node kind {word}", case.name);
                        kind
                    }
                    None => MdKind::Other,
                };
                assert_eq!(node.kind, kind, "{line}");
                nodes += 1;
            }
        }
        assert_eq!(parsed.metadata().nodes().len(), nodes, "{}: nodes", case.name);
        assert_eq!(parsed.metadata().named().len(), named, "{}: named metadata", case.name);
        assert_eq!(parsed.attr_groups().len(), group_count, "{}: groups", case.name);
        assert_eq!(parsed.metadata().has_debug_info(), debug, "{}: debug info", case.name);
        debug_attachments(case, &parsed, &canonical);
    }
}

/// The `!dbg` of each global value became the module's: a function's
/// subprogram, a variable's global-variable expressions (`spec/ll.md`
/// §7.4), each of the kind LLVM requires there.
fn debug_attachments(case: &Case, parsed: &ParsedModule<'_>, canonical: &str) {
    let (mut functions, mut variables) = (0, 0);
    for line in canonical.lines() {
        if !["define ", "declare ", "@"].iter().any(|start| line.starts_with(start)) {
            continue;
        }
        let words = split_top(line, b' ');
        let count = words.iter().filter(|word| **word == "!dbg" || **word == "!dbg,").count();
        if line.starts_with('@') {
            variables += count;
        } else {
            functions += count;
        }
    }
    let module = &parsed.module;
    let mut subprograms = 0;
    for symbol in module.symbols() {
        if let SymbolKind::Function { subprogram: Some(id), .. } = symbol.kind() {
            assert_eq!(module.debug.kind(id), DiKind::Subprogram, "{}", case.name);
            subprograms += 1;
        }
    }
    assert_eq!(subprograms, functions, "{}: !dbg of functions", case.name);
    assert_eq!(module.global_exprs().len(), variables, "{}: !dbg of variables", case.name);
    for &(_, expr) in module.global_exprs() {
        assert_eq!(module.debug.kind(expr), DiKind::GlobalVariableExpression, "{}", case.name);
    }
    for function in parsed.functions() {
        let SymbolKind::Function { subprogram, .. } = module.symbol(function.sym).kind() else {
            panic!("{}: a definition that is not a function", case.name);
        };
        assert_eq!(function.dbg.is_some(), subprogram.is_some(), "{}", case.name);
    }
}

/// §6: the text side is shared by readers on several threads while the
/// module is added to. Each thread lexes a share of the bodies from the
/// recorded spans, and together they see what one reader sees.
#[test]
fn bodies_are_read_from_several_threads_while_the_module_grows() {
    const READERS: usize = 4;
    let Some(case) = corpus().iter().max_by_key(|case| case.text.len()) else {
        return;
    };
    let mut parsed = parsed(case);
    let ParsedModule { module, text } = &mut parsed;
    let text: &ModuleText<'_> = text;
    let count = |function: &FunctionText| {
        let mut lexer = Lexer::new(text.source());
        lexer.set_pos(function.body.start as usize);
        let mut tokens = 0usize;
        loop {
            let token = lexer.next_token();
            if token.start >= function.body.end || token.kind == Tok::Eof {
                return tokens;
            }
            assert_ne!(token.kind, Tok::Error, "{}: {}", case.name, lexer.error());
            tokens += 1;
        }
    };
    let count = &count;
    let serial: usize = text.functions().iter().map(count).sum();
    let shared: usize = std::thread::scope(|scope| {
        let readers: Vec<_> = (0..READERS)
            .map(|reader| {
                let share = text.functions().iter().skip(reader).step_by(READERS);
                scope.spawn(move || share.map(count).sum::<usize>())
            })
            .collect();
        // Meanwhile the module takes a type that no text of the corpus has.
        let before = module.types.len();
        module.types.int(977).expect("an integer type");
        assert_eq!(module.types.len(), before + 1);
        readers.into_iter().map(|reader| reader.join().expect("a reader")).sum()
    });
    assert!(serial > text.functions().len(), "{}: bodies have tokens", case.name);
    assert_eq!(shared, serial, "{}", case.name);
}
