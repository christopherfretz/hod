//! Symbol and module-flag validity against `llvm-as` (`spec/ir.md` §2.1,
//! §2.2): what LLVM's parser and verifier refuse, hod's builder and
//! `Module::verify_symbols` refuse, and nothing else.
//!
//! Each case is a list of symbol descriptions that is written out twice:
//! as LLVM text for `llvm-as`, and as builder calls. The two verdicts
//! must agree, except for the cases in [`DIFFERENCES`], where hod's rule
//! is deliberately not LLVM's and the test pins both verdicts.

mod modlevel;

use std::fmt::Write as _;

use hod_ir::{
    AliasDesc, ComdatId, ConstId, FlagValue, FunctionDesc, IFuncDesc, IrResult, Linkage, Module,
    SymId, TargetSpec, TlsModel, TypeId, VariableDesc, Visibility,
};
use hod_testutil::Scratch;
use modlevel::{Assembler, MAJORS};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum What {
    Var,
    Func,
    Alias,
    IFunc,
}

/// The value type of a variable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    I32,
    Array,
    Opaque,
    Scalable,
    Void,
    Function,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Init {
    None,
    Zero,
    NonZero,
}

/// What an alias points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Sym(&'static str, i64),
    Null,
    Absolute,
}

/// One symbol of a case. `name` is empty for an anonymous symbol. The
/// fields are those of LLVM's text, flags and all.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::struct_excessive_bools)]
struct Spec {
    what: What,
    name: &'static str,
    linkage: Linkage,
    visibility: Visibility,
    dso_local: bool,
    ty: Ty,
    constant: bool,
    init: Init,
    tls: bool,
    comdat: Option<&'static str>,
    section: Option<&'static str>,
    align: Option<u64>,
    /// Function: defined, not declared.
    body: bool,
    /// Function: returns `ptr`, not `void`.
    ret_ptr: bool,
    personality: Option<&'static str>,
    /// Alias: the aliasee. IFUNC: the resolver.
    target: Target,
}

const BASE: Spec = Spec {
    what: What::Var,
    name: "",
    linkage: Linkage::External,
    visibility: Visibility::Default,
    dso_local: false,
    ty: Ty::I32,
    constant: false,
    init: Init::None,
    tls: false,
    comdat: None,
    section: None,
    align: None,
    body: false,
    ret_ptr: false,
    personality: None,
    target: Target::Null,
};

/// `@name = external global i32`.
const fn var(name: &'static str) -> Spec {
    Spec { name, ..BASE }
}

/// `@name = global i32 1`.
const fn def(name: &'static str) -> Spec {
    Spec { name, init: Init::NonZero, ..BASE }
}

/// `declare void @name()`.
const fn decl_fn(name: &'static str) -> Spec {
    Spec { what: What::Func, name, ..BASE }
}

/// `define void @name() { ret void }`.
const fn def_fn(name: &'static str) -> Spec {
    Spec { what: What::Func, name, body: true, ..BASE }
}

/// `define ptr @name() { ret ptr null }`: a resolver.
const fn resolver(name: &'static str) -> Spec {
    Spec { what: What::Func, name, body: true, ret_ptr: true, ..BASE }
}

/// `@name = alias i32, ptr @target`.
const fn alias(name: &'static str, target: &'static str) -> Spec {
    Spec { what: What::Alias, name, target: Target::Sym(target, 0), ..BASE }
}

/// `@name = ifunc void (), ptr @resolver`.
const fn ifunc(name: &'static str, resolver: &'static str) -> Spec {
    Spec { what: What::IFunc, name, target: Target::Sym(resolver, 0), ..BASE }
}

fn type_text(ty: Ty) -> &'static str {
    match ty {
        Ty::I32 => "i32",
        Ty::Array => "[2 x i32]",
        Ty::Opaque => "%opaque",
        Ty::Scalable => "<vscale x 4 x i32>",
        Ty::Void => "void",
        Ty::Function => "void ()",
    }
}

/// The case as LLVM text.
fn text(specs: &[Spec]) -> String {
    let mut out = String::from("%opaque = type opaque\n");
    let mut comdats: Vec<&str> = specs.iter().filter_map(|spec| spec.comdat).collect();
    comdats.sort_unstable();
    comdats.dedup();
    for comdat in comdats {
        writeln!(out, "${comdat} = comdat any").unwrap();
    }
    for spec in specs {
        let name = if spec.name.is_empty() { "0" } else { spec.name };
        let is_declaration = match spec.what {
            What::Var => spec.init == Init::None,
            What::Func => !spec.body,
            What::Alias | What::IFunc => false,
        };
        // `external` is spelled out only on a variable declaration.
        let mut prefix = match (spec.linkage, spec.what, is_declaration) {
            (Linkage::External, What::Var, true) => "external ".to_string(),
            (Linkage::External, ..) => String::new(),
            (linkage, ..) => format!("{} ", linkage.keyword()),
        };
        if spec.dso_local {
            prefix.push_str("dso_local ");
        }
        match spec.visibility {
            Visibility::Default => {}
            Visibility::Hidden => prefix.push_str("hidden "),
            Visibility::Protected => prefix.push_str("protected "),
        }
        if spec.tls {
            prefix.push_str("thread_local ");
        }
        let mut suffix = String::new();
        let comma = if spec.what == What::Var { "," } else { "" };
        if let Some(section) = spec.section {
            write!(suffix, "{comma} section \"{section}\"").unwrap();
        }
        if let Some(comdat) = spec.comdat {
            write!(suffix, "{comma} comdat(${comdat})").unwrap();
        }
        if let Some(align) = spec.align {
            write!(suffix, "{comma} align {align}").unwrap();
        }
        match spec.what {
            What::Var => {
                let ty = type_text(spec.ty);
                let init = match (spec.init, spec.ty) {
                    (Init::None, _) => "",
                    (Init::Zero, Ty::I32) => " 0",
                    (Init::NonZero, Ty::I32) => " 1",
                    (Init::NonZero, Ty::Array) => " [i32 1, i32 1]",
                    (..) => " zeroinitializer",
                };
                let kind = if spec.constant { "constant" } else { "global" };
                writeln!(out, "@{name} = {prefix}{kind} {ty}{init}{suffix}").unwrap();
            }
            What::Func => {
                let ret = if spec.ret_ptr { "ptr" } else { "void" };
                if let Some(personality) = spec.personality {
                    write!(suffix, " personality ptr @{personality}").unwrap();
                }
                if spec.body {
                    let value = if spec.ret_ptr { "ptr null" } else { "void" };
                    writeln!(out, "define {prefix}{ret} @{name}(){suffix} {{\n  ret {value}\n}}")
                        .unwrap();
                } else {
                    writeln!(out, "declare {prefix}{ret} @{name}(){suffix}").unwrap();
                }
            }
            What::Alias => {
                let target = match spec.target {
                    Target::Sym(target, 0) => format!("ptr @{target}"),
                    Target::Sym(target, offset) => {
                        format!("ptr getelementptr (i8, ptr @{target}, i64 {offset})")
                    }
                    Target::Null => "ptr null".to_string(),
                    Target::Absolute => "ptr inttoptr (i64 4 to ptr)".to_string(),
                };
                writeln!(out, "@{name} = {prefix}alias i32, {target}").unwrap();
            }
            What::IFunc => {
                let Target::Sym(target, 0) = spec.target else { panic!("an IFUNC has a resolver") };
                writeln!(out, "@{name} = {prefix}ifunc void (), ptr @{target}").unwrap();
            }
        }
    }
    out
}

fn variable_type(m: &mut Module, ty: Ty) -> IrResult<TypeId> {
    match ty {
        Ty::I32 => Ok(TypeId::I32),
        Ty::Array => m.types.array(TypeId::I32, 2),
        Ty::Opaque => m.types.opaque_struct(),
        Ty::Scalable => m.types.scalable_vector(TypeId::I32, 4),
        Ty::Void => Ok(TypeId::VOID),
        Ty::Function => m.types.function(TypeId::VOID, &[], false),
    }
}

fn initializer(m: &mut Module, ty: TypeId, spec: &Spec) -> IrResult<Option<ConstId>> {
    Ok(Some(match (spec.init, spec.ty) {
        (Init::None, _) => return Ok(None),
        (Init::Zero, Ty::I32) => m.consts.int(&m.types, ty, 0)?,
        (Init::NonZero, Ty::I32) => m.consts.int(&m.types, ty, 1)?,
        (Init::NonZero, Ty::Array) => {
            let one = m.consts.int(&m.types, TypeId::I32, 1)?;
            m.consts.aggregate(&m.types, ty, &[one, one])?
        }
        (..) => m.consts.zero(&m.types, ty)?,
    }))
}

/// The case through the builder.
fn build(specs: &[Spec]) -> IrResult<()> {
    let mut m = Module::new(TargetSpec::x86_64_linux_gnu());
    let void_fn = m.types.function(TypeId::VOID, &[], false)?;
    let ptr_fn = m.types.function(TypeId::PTR, &[], false)?;
    for spec in specs {
        let sym = if spec.name.is_empty() {
            m.anonymous_symbol()?
        } else {
            m.symbol_named(spec.name.as_bytes())?
        };
        let comdat: Option<ComdatId> = match spec.comdat {
            Some(name) => Some(m.comdat_named(name.as_bytes())?),
            None => None,
        };
        let section = spec.section.map(str::as_bytes);
        let tls = if spec.tls { TlsModel::GeneralDynamic } else { TlsModel::None };
        match spec.what {
            What::Var => {
                let ty = variable_type(&mut m, spec.ty)?;
                let init = initializer(&mut m, ty, spec)?;
                let desc = VariableDesc {
                    linkage: spec.linkage,
                    visibility: spec.visibility,
                    dso_local: spec.dso_local,
                    section,
                    comdat,
                    align: spec.align,
                    constant: spec.constant,
                    tls,
                    init,
                    ..VariableDesc::new(ty)
                };
                m.set_variable(sym, &desc)?;
            }
            What::Func => {
                let personality: Option<SymId> = match spec.personality {
                    Some(name) => Some(m.symbol_named(name.as_bytes())?),
                    None => None,
                };
                let desc = FunctionDesc {
                    linkage: spec.linkage,
                    visibility: spec.visibility,
                    dso_local: spec.dso_local,
                    section,
                    comdat,
                    align: spec.align,
                    personality,
                    has_body: spec.body,
                    ..FunctionDesc::new(if spec.ret_ptr { ptr_fn } else { void_fn })
                };
                m.set_function(sym, &desc)?;
            }
            What::Alias => {
                let target = match spec.target {
                    Target::Sym(name, offset) => {
                        let target = m.symbol_named(name.as_bytes())?;
                        m.consts.addr(target, offset)?
                    }
                    Target::Null => m.consts.null(&m.types, TypeId::PTR)?,
                    Target::Absolute => m.consts.addr_of_int(4)?,
                };
                let desc = AliasDesc {
                    linkage: spec.linkage,
                    visibility: spec.visibility,
                    dso_local: spec.dso_local,
                    tls,
                    ..AliasDesc::new(TypeId::I32, target)
                };
                m.set_alias(sym, &desc)?;
            }
            What::IFunc => {
                let Target::Sym(name, 0) = spec.target else { panic!("an IFUNC has a resolver") };
                let resolver = m.symbol_named(name.as_bytes())?;
                let desc = IFuncDesc {
                    linkage: spec.linkage,
                    visibility: spec.visibility,
                    dso_local: spec.dso_local,
                    ..IFuncDesc::new(void_fn, resolver)
                };
                m.set_ifunc(sym, &desc)?;
            }
        }
    }
    m.verify_symbols()
}

/// A case: its name, what `llvm-as` 22 says (`None`: whatever it says,
/// hod must say the same), and the symbols.
type Case = (String, Option<bool>, Vec<Spec>);

/// The cases where hod's verdict is not LLVM's, with both verdicts
/// `(llvm, hod)`. The reasons are in `spec/ir.md` §2.2.
const DIFFERENCES: [(&str, bool, bool); 7] = [
    // Describing a declaration again replaces it; LLVM's text has no
    // second mention of a name.
    ("declared-twice", false, true),
    ("declared-then-defined", false, true),
    ("declared-function-becomes-alias", false, true),
    ("declared-function-becomes-ifunc", false, true),
    // LLVM names an anonymous external symbol `__unnamed_N` when it
    // writes the object; hod leaves naming to the producer.
    ("anonymous-external", true, false),
    // The personality of a function that gets no body is never read.
    ("personality-on-declaration", false, true),
    // LLVM 19 accepts a definition of an opaque type (and LLVM 22 does
    // not); hod never does. Listed for LLVM 19 only, see the test.
    ("opaque-definition", true, false),
];

/// The cases: a table, so long.
#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    let mut case = |name: &str, llvm: bool, specs: &[Spec]| {
        cases.push((name.to_string(), Some(llvm), specs.to_vec()));
    };
    let weak = Linkage::WeakAny;
    let ae = Linkage::AvailableExternally;

    // Declarations and definitions.
    case("variable-declaration", true, &[var("x")]);
    case("variable-definition", true, &[def("x")]);
    case("function-declaration", true, &[decl_fn("f")]);
    case("function-definition", true, &[def_fn("f")]);
    case("extern-weak-definition", false, &[Spec { linkage: Linkage::ExternWeak, ..def("x") }]);
    case("defined-twice", false, &[def("x"), def("x")]);
    case("function-defined-twice", false, &[def_fn("f"), def_fn("f")]);
    case("variable-then-function", false, &[def("x"), def_fn("x")]);
    case("declared-twice", false, &[var("x"), var("x")]);
    case("declared-then-defined", false, &[decl_fn("f"), def_fn("f")]);
    case("declared-function-becomes-alias", false, &[def("x"), decl_fn("f"), alias("f", "x")]);
    case("declared-function-becomes-ifunc", false, &[resolver("r"), decl_fn("f"), ifunc("f", "r")]);
    case("reference-to-nothing", false, &[alias("a", "nothing")]);
    case("anonymous-external", true, &[def("")]);
    case("anonymous-internal", true, &[Spec { linkage: Linkage::Internal, ..def("") }]);
    case("anonymous-private", true, &[Spec { linkage: Linkage::Private, ..def("") }]);

    // Visibility and dso_local.
    let hidden = Visibility::Hidden;
    let protected = Visibility::Protected;
    case("hidden-declaration", true, &[Spec { visibility: hidden, ..var("x") }]);
    case("protected-declaration", true, &[Spec { visibility: protected, ..decl_fn("f") }]);
    case("hidden-definition", true, &[Spec { visibility: hidden, ..def("x") }]);
    case(
        "hidden-internal",
        false,
        &[Spec { linkage: Linkage::Internal, visibility: hidden, ..def("x") }],
    );
    case(
        "protected-private",
        false,
        &[Spec { linkage: Linkage::Private, visibility: protected, ..def_fn("f") }],
    );
    case(
        "hidden-extern-weak",
        true,
        &[Spec { linkage: Linkage::ExternWeak, visibility: hidden, ..var("x") }],
    );
    case("dso-local-declaration", true, &[Spec { dso_local: true, ..var("x") }]);
    case("dso-local-definition", true, &[Spec { dso_local: true, ..def_fn("f") }]);
    case("dso-local-weak", true, &[Spec { dso_local: true, linkage: weak, ..def("x") }]);

    // Common and appending.
    let common = Spec { linkage: Linkage::Common, init: Init::Zero, ..var("x") };
    case("common", true, &[common]);
    case("common-nonzero", false, &[Spec { init: Init::NonZero, ..common }]);
    case("common-constant", false, &[Spec { constant: true, ..common }]);
    case("common-in-comdat", false, &[Spec { comdat: Some("c"), ..common }]);
    case("common-with-section", true, &[Spec { section: Some(".mybss"), ..common }]);
    case("common-thread-local", true, &[Spec { tls: true, ..common }]);
    case("common-aligned", true, &[Spec { align: Some(64), ..common }]);
    let appending =
        Spec { linkage: Linkage::Appending, ty: Ty::Array, init: Init::Zero, ..var("x") };
    case("appending-array", true, &[appending]);
    case("appending-scalar", false, &[Spec { ty: Ty::I32, ..appending }]);

    // COMDATs, sections, alignment.
    case("definition-in-comdat", true, &[Spec { comdat: Some("c"), ..def("x") }]);
    case(
        "function-in-comdat",
        true,
        &[Spec { comdat: Some("f"), linkage: Linkage::LinkOnceOdr, ..def_fn("f") }],
    );
    case(
        "internal-in-comdat",
        true,
        &[Spec { comdat: Some("c"), linkage: Linkage::Internal, ..def("x") }],
    );
    case(
        "private-in-comdat",
        true,
        &[Spec { comdat: Some("c"), linkage: Linkage::Private, ..def_fn("f") }],
    );
    case(
        "two-in-one-comdat",
        true,
        &[Spec { comdat: Some("c"), ..def("x") }, Spec { comdat: Some("c"), ..def_fn("f") }],
    );
    case("declaration-in-comdat", false, &[Spec { comdat: Some("c"), ..var("x") }]);
    case("function-declaration-in-comdat", false, &[Spec { comdat: Some("c"), ..decl_fn("f") }]);
    case(
        "available-externally-in-comdat",
        false,
        &[Spec { comdat: Some("c"), linkage: ae, ..def("x") }],
    );
    case(
        "available-externally-function-in-comdat",
        false,
        &[Spec { comdat: Some("c"), linkage: ae, ..def_fn("f") }],
    );
    case("section-on-declaration", true, &[Spec { section: Some(".x"), ..var("x") }]);
    case("section-on-function-declaration", true, &[Spec { section: Some(".x"), ..decl_fn("f") }]);
    case("empty-section", true, &[Spec { section: Some(""), ..def("x") }]);
    case("align-on-declaration", true, &[Spec { align: Some(16), ..var("x") }]);
    case("align-on-function", true, &[Spec { align: Some(32), ..def_fn("f") }]);
    case("align-3", false, &[Spec { align: Some(3), ..def("x") }]);
    case("align-2^32", true, &[Spec { align: Some(1 << 32), ..def("x") }]);
    case("align-2^33", false, &[Spec { align: Some(1 << 33), ..def("x") }]);

    // Value types.
    case("void-variable", false, &[Spec { ty: Ty::Void, ..var("x") }]);
    case("function-typed-variable", false, &[Spec { ty: Ty::Function, ..var("x") }]);
    case("scalable-declaration", false, &[Spec { ty: Ty::Scalable, ..var("x") }]);
    case("scalable-definition", false, &[Spec { ty: Ty::Scalable, init: Init::Zero, ..var("x") }]);
    case("opaque-declaration", true, &[Spec { ty: Ty::Opaque, ..var("x") }]);
    case("opaque-definition", false, &[Spec { ty: Ty::Opaque, init: Init::Zero, ..var("x") }]);
    case("thread-local-declaration", true, &[Spec { tls: true, ..var("x") }]);

    // Personality.
    case(
        "personality",
        true,
        &[
            decl_fn("__gxx_personality_v0"),
            Spec { personality: Some("__gxx_personality_v0"), ..def_fn("f") },
        ],
    );
    case(
        "personality-on-declaration",
        false,
        &[
            decl_fn("__gxx_personality_v0"),
            Spec { personality: Some("__gxx_personality_v0"), ..decl_fn("f") },
        ],
    );
    case(
        "personality-is-a-variable",
        true,
        &[def("p"), Spec { personality: Some("p"), ..def_fn("f") }],
    );

    // Aliases.
    case("alias-of-definition", true, &[def("x"), alias("a", "x")]);
    case("alias-of-function", true, &[def_fn("f"), alias("a", "f")]);
    case("alias-before-its-target", true, &[alias("a", "x"), def("x")]);
    case(
        "alias-into-definition",
        true,
        &[def("x"), Spec { target: Target::Sym("x", 2), ..alias("a", "x") }],
    );
    case("alias-of-declaration", false, &[var("x"), alias("a", "x")]);
    case("alias-of-function-declaration", false, &[decl_fn("f"), alias("a", "f")]);
    case(
        "alias-of-available-externally",
        false,
        &[Spec { linkage: ae, ..def("x") }, alias("a", "x")],
    );
    case(
        "available-externally-alias",
        true,
        &[Spec { linkage: ae, ..def("x") }, Spec { linkage: ae, ..alias("a", "x") }],
    );
    case(
        "available-externally-alias-of-definition",
        false,
        &[def("x"), Spec { linkage: ae, ..alias("a", "x") }],
    );
    case(
        "available-externally-alias-with-offset",
        false,
        &[
            Spec { linkage: ae, ..def("x") },
            Spec { linkage: ae, target: Target::Sym("x", 2), ..alias("a", "x") },
        ],
    );
    case("alias-of-weak-variable", true, &[Spec { linkage: weak, ..def("x") }, alias("a", "x")]);
    case("alias-of-alias", true, &[def("x"), alias("a", "x"), alias("b", "a")]);
    case(
        "alias-of-weak-alias",
        false,
        &[def("x"), Spec { linkage: weak, ..alias("a", "x") }, alias("b", "a")],
    );
    case(
        "alias-of-linkonce-alias",
        false,
        &[def("x"), Spec { linkage: Linkage::LinkOnceAny, ..alias("a", "x") }, alias("b", "a")],
    );
    case(
        "alias-of-linkonce-odr-alias",
        true,
        &[def("x"), Spec { linkage: Linkage::LinkOnceOdr, ..alias("a", "x") }, alias("b", "a")],
    );
    case("alias-of-itself", false, &[alias("a", "a")]);
    case("alias-cycle", false, &[alias("a", "b"), alias("b", "c"), alias("c", "a")]);
    case("alias-of-null", false, &[Spec { target: Target::Null, ..alias("a", "") }]);
    case("alias-of-absolute-address", true, &[Spec { target: Target::Absolute, ..alias("a", "") }]);
    case("alias-of-ifunc", true, &[resolver("r"), ifunc("i", "r"), alias("a", "i")]);
    case(
        "thread-local-alias",
        true,
        &[Spec { tls: true, ..def("x") }, Spec { tls: true, ..alias("a", "x") }],
    );
    case("hidden-alias", true, &[def("x"), Spec { visibility: hidden, ..alias("a", "x") }]);

    // IFUNCs.
    case("ifunc", true, &[resolver("r"), ifunc("i", "r")]);
    case("ifunc-before-its-resolver", true, &[ifunc("i", "r"), resolver("r")]);
    case("ifunc-resolver-returns-void", false, &[def_fn("r"), ifunc("i", "r")]);
    case(
        "ifunc-resolver-declared",
        false,
        &[Spec { ret_ptr: true, ..decl_fn("r") }, ifunc("i", "r")],
    );
    case(
        "ifunc-resolver-available-externally",
        false,
        &[Spec { linkage: ae, ..resolver("r") }, ifunc("i", "r")],
    );
    case("ifunc-resolver-weak", true, &[Spec { linkage: weak, ..resolver("r") }, ifunc("i", "r")]);
    case(
        "ifunc-resolver-internal",
        true,
        &[Spec { linkage: Linkage::Internal, ..resolver("r") }, ifunc("i", "r")],
    );
    case("ifunc-resolver-is-a-variable", false, &[def("r"), ifunc("i", "r")]);
    case("ifunc-through-alias", true, &[resolver("r"), alias("a", "r"), ifunc("i", "a")]);
    case(
        "ifunc-through-two-aliases",
        true,
        &[resolver("r"), alias("a", "r"), alias("b", "a"), ifunc("i", "b")],
    );
    case(
        "ifunc-through-alias-with-offset",
        false,
        &[resolver("r"), Spec { target: Target::Sym("r", 4), ..alias("a", "r") }, ifunc("i", "a")],
    );
    case("ifunc-is-its-own-resolver", false, &[ifunc("i", "i")]);
    case(
        "hidden-weak-ifunc",
        true,
        &[resolver("r"), Spec { linkage: weak, visibility: hidden, ..ifunc("i", "r") }],
    );

    // Every linkage on every kind of symbol: hod says what LLVM says.
    for linkage in Linkage::ALL {
        let name = linkage.keyword();
        let mut grid = |kind: &str, specs: &[Spec]| {
            cases.push((format!("{name}-{kind}"), None, specs.to_vec()));
        };
        grid("variable-declaration", &[Spec { linkage, ..var("x") }]);
        grid("variable-definition", &[Spec { linkage, init: Init::Zero, ..var("x") }]);
        grid(
            "array-definition",
            &[Spec { linkage, ty: Ty::Array, init: Init::NonZero, ..var("x") }],
        );
        grid("function-declaration", &[Spec { linkage, ..decl_fn("f") }]);
        grid("function-definition", &[Spec { linkage, ..def_fn("f") }]);
        grid("alias", &[def("x"), Spec { linkage, ..alias("a", "x") }]);
        grid("ifunc", &[resolver("r"), Spec { linkage, ..ifunc("i", "r") }]);
        for visibility in [hidden, protected] {
            let kind = format!("{visibility:?}-definition").to_lowercase();
            grid(&kind, &[Spec { linkage, visibility, init: Init::Zero, ..var("x") }]);
        }
    }
    cases
}

/// §2.2: symbol validity is LLVM's, with the listed differences.
#[test]
fn symbol_rules_match_llvm_as() {
    let cases = cases();
    assert!(cases.len() > 180, "{} cases", cases.len());
    for major in MAJORS {
        let Some(assembler) = Assembler::new(major) else { continue };
        let scratch = Scratch::new(&format!("symbols-as-{major}"));
        let mut wrong = String::new();
        for (name, expected, specs) in &cases {
            let ir = text(specs);
            let llvm = assembler.assemble(&scratch, "case", &ir, true);
            let hod = build(specs);
            let difference = DIFFERENCES.iter().find(|(case, ..)| case == name).filter(|_| {
                // The one difference that is LLVM 19's alone.
                name != "opaque-definition" || major == 19
            });
            let (want_llvm, want_hod) = match (difference, expected) {
                (Some(&(_, llvm, hod)), _) => (llvm, hod),
                (None, Some(llvm)) => (*llvm, *llvm),
                (None, None) => (llvm.is_ok(), llvm.is_ok()),
            };
            if llvm.is_ok() != want_llvm || hod.is_ok() != want_hod {
                writeln!(
                    wrong,
                    "{name}: llvm-as {major} {:?} (expected {}), hod {:?} (expected {})\n{ir}",
                    llvm.as_ref().map_err(|error| error.lines().next().unwrap_or("").to_string()),
                    if want_llvm { "ok" } else { "an error" },
                    hod.as_ref().map_err(ToString::to_string),
                    if want_hod { "ok" } else { "an error" },
                )
                .unwrap();
            }
        }
        assert!(wrong.is_empty(), "{wrong}");
    }
    // A difference names a case.
    for (name, ..) in DIFFERENCES {
        assert!(cases.iter().any(|(case, ..)| case == name), "{name}");
    }
}

#[derive(Clone, Copy, Debug)]
enum Value {
    Int(u64),
    Str(&'static str),
    Node,
    /// A requirement: the flag of that name has that value.
    Require(&'static str, u64),
}

type Flag = (u32, &'static str, Value);

fn flags_text(flags: &[Flag]) -> String {
    let mut out = String::from("!llvm.module.flags = !{");
    for index in 0..flags.len() {
        write!(out, "{}!{index}", if index == 0 { "" } else { ", " }).unwrap();
    }
    out.push_str("}\n");
    for (index, (behavior, name, value)) in flags.iter().enumerate() {
        let value = match value {
            Value::Int(int) => format!("i32 {int}"),
            Value::Str(text) => format!("!\"{text}\""),
            Value::Node => "!{}".to_string(),
            Value::Require(flag, int) => format!("!{{!\"{flag}\", i32 {int}}}"),
        };
        writeln!(out, "!{index} = !{{i32 {behavior}, !\"{name}\", {value}}}").unwrap();
    }
    out
}

fn flags_build(flags: &[Flag]) -> IrResult<()> {
    let mut m = Module::new(TargetSpec::x86_64_linux_gnu());
    for (behavior, name, value) in flags {
        let value = match value {
            Value::Int(int) => FlagValue::Int(*int),
            Value::Str(text) => FlagValue::Str(text.as_bytes()),
            Value::Node | Value::Require(..) => FlagValue::Other,
        };
        m.set_module_flag(*behavior, name.as_bytes(), value)?;
    }
    Ok(())
}

/// §2.1: module-flag validity against `llvm-as`. `(name, llvm, hod,
/// flags)`: where the two verdicts differ, hod is the stricter one about
/// the values of flags it reads, and does not evaluate requirements.
#[test]
fn module_flag_rules_match_llvm_as() {
    let int = Value::Int;
    let cases: Vec<(&str, bool, bool, Vec<Flag>)> = vec![
        ("none", true, true, vec![]),
        ("pic", true, true, vec![(8, "PIC Level", int(2)), (7, "PIE Level", int(2))]),
        ("unknown-flag", true, true, vec![(1, "frobnicate", int(1))]),
        ("string-value", true, true, vec![(1, "frobnicate", Value::Str("x"))]),
        ("node-value", true, true, vec![(5, "frobnicate", Value::Node)]),
        ("behavior-0", false, false, vec![(0, "x", int(1))]),
        ("behavior-9", false, false, vec![(9, "x", int(1))]),
        (
            "every-behavior",
            true,
            true,
            vec![
                (1, "a", int(1)),
                (2, "b", int(1)),
                (4, "d", int(1)),
                (5, "e", Value::Node),
                (6, "f", Value::Node),
                (7, "g", int(1)),
                (8, "h", int(1)),
            ],
        ),
        ("set-twice", false, false, vec![(1, "x", int(1)), (1, "x", int(1))]),
        ("known-set-twice", false, false, vec![(8, "PIC Level", int(2)), (8, "PIC Level", int(2))]),
        ("max-string", false, false, vec![(7, "x", Value::Str("big"))]),
        ("min-node", false, false, vec![(8, "x", Value::Node)]),
        ("requirement-met", true, true, vec![(1, "x", int(1)), (3, "y", Value::Require("x", 1))]),
        (
            "requirement-on-own-name",
            true,
            true,
            vec![(1, "x", int(1)), (3, "x", Value::Require("x", 1))],
        ),
        // LLVM evaluates a requirement; hod keeps it for `classify`.
        (
            "requirement-not-met",
            false,
            true,
            vec![(1, "x", int(1)), (3, "y", Value::Require("x", 2))],
        ),
        ("requirement-on-nothing", false, true, vec![(3, "y", Value::Require("x", 2))]),
        // LLVM does not check the values of the flags hod reads.
        ("pic-level-9", true, false, vec![(8, "PIC Level", int(9))]),
        ("pie-level-3", true, false, vec![(7, "PIE Level", int(3))]),
        ("code-model-9", true, false, vec![(1, "Code Model", int(9))]),
        ("uwtable-3", true, false, vec![(7, "uwtable", int(3))]),
        ("frame-pointer-9", true, false, vec![(7, "frame-pointer", int(9))]),
        ("code-model-string", true, false, vec![(1, "Code Model", Value::Str("2"))]),
        ("uwtable-string", true, false, vec![(1, "uwtable", Value::Str("2"))]),
        // LLVM checks that some flags are integers; hod checks all it reads.
        ("pic-level-string", false, false, vec![(1, "PIC Level", Value::Str("2"))]),
        ("bti-string", false, false, vec![(1, "branch-target-enforcement", Value::Str("2"))]),
        // A flag hod ignores is not looked at.
        ("wchar-size-string", false, true, vec![(1, "wchar_size", Value::Str("4"))]),
    ];
    for major in MAJORS {
        let Some(assembler) = Assembler::new(major) else { continue };
        let scratch = Scratch::new(&format!("flags-as-{major}"));
        let mut wrong = String::new();
        for (name, want_llvm, want_hod, flags) in &cases {
            let ir = flags_text(flags);
            let llvm = assembler.assemble(&scratch, "flags", &ir, true);
            let hod = flags_build(flags);
            if llvm.is_ok() != *want_llvm || hod.is_ok() != *want_hod {
                writeln!(
                    wrong,
                    "{name}: llvm-as {major} {:?}, hod {:?}\n{ir}",
                    llvm.as_ref().map_err(|error| error.lines().next().unwrap_or("").to_string()),
                    hod.as_ref().map_err(ToString::to_string),
                )
                .unwrap();
            }
        }
        assert!(wrong.is_empty(), "{wrong}");
    }
}
