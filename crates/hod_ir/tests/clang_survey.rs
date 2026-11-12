//! What clang emits, through the module-level tables (`spec/ir.md` §2.1,
//! §5.7): for a small C and a small C++ translation unit, both targets,
//! clang 19 and 22, and the flag sets below, every module flag is known
//! or ignorable, every attribute is kept or dropped, and every kept
//! function attribute has a representation.
//!
//! This is the evidence for the "known" and "ignorable" flag lists and
//! for the additions to the spec's attribute lists. There is no IR
//! parser yet, so the test reads the three things it needs from clang's
//! text with a scanner of its own: the module-flag nodes, the attribute
//! groups, and the attribute keywords on `define`, `declare` and call
//! lines.

mod modlevel;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use hod_ir::{
    AttrClass, AttrSyntax, BranchProtection, FlagValue, FnAttrs, FnStr, FramePointer, Module,
    PicLevel, TargetSpec, UwTable, attr_class,
};
use hod_testutil::Scratch;
use modlevel::{MAJORS, llvm_tool};

/// C: thread-local and static data, a weak function, a constructor, a
/// variadic function, an `always_inline` one, a `noreturn` callee, an
/// alias, an IFUNC, an aggregate returned in memory.
const C_SOURCE: &str = r#"
struct S { long a, b, c; };
__thread int tls_var;
static int counter;
int global_init = 42;
extern int ext;
__attribute__((weak)) int weak_fn(void) { return 1; }
__attribute__((noreturn)) void die(void);
__attribute__((constructor)) static void ctor(void) { counter++; }
__attribute__((destructor)) static void dtor(void) { counter--; }
__attribute__((used)) static int used_var = 3;
struct S make(long a) { struct S s = { a, a, a }; return s; }
int sum(int n, ...) {
  __builtin_va_list ap; __builtin_va_start(ap, n); int t = 0;
  for (int i = 0; i < n; i++) t += __builtin_va_arg(ap, int);
  __builtin_va_end(ap); return t;
}
__attribute__((always_inline)) int ai(int x) { return x + 1; }
int call(int (*f)(int), int x) { if (x < 0) die(); return f(ai(x)) + ext + tls_var; }
void *mem(void *d, const void *s, unsigned long n) { return __builtin_memcpy(d, s, n); }
double fp(double a, float b) { return a * b; }
signed char narrow(signed char a, unsigned short b) { return a + b; }
int alias_target(void) { return 0; }
int the_alias(void) __attribute__((alias("alias_target")));
static void *resolve(void) { return alias_target; }
int ifn(void) __attribute__((ifunc("resolve")));
"#;

/// C++, with no header: a vtable, dynamic initialization, thread-local
/// with a destructor, templates, `new`/`delete`, exceptions, a function
/// static, a switch.
const CXX_SOURCE: &str = r"
typedef unsigned long size_t;
void *operator new(size_t); void operator delete(void *) noexcept;
void *operator new[](size_t); void operator delete[](void *) noexcept;
struct Base { virtual ~Base(); virtual int f() = 0; };
struct Derived final : Base { int x; int f() override { return x; } };
Base::~Base() {}
struct NT { NT(); ~NT(); int v; };
thread_local NT tls_s;
NT global_nt;
inline int inline_var = 7;
template <class T> T tmpl(T a) { return a + a; }
[[noreturn]] void thrower(int x) { throw x; }
int catcher(int x) { try { thrower(x); } catch (int e) { return e; } catch (...) { return 2; } return 0; }
Base *mk() { return new Derived(); }
int *alloc(unsigned long n) { return new int[n]; }
void dealloc(int *p) { delete[] p; }
static int sfn() { static int once = alloc(3)[0]; return once; }
int use() { return sfn() + inline_var + tmpl(3) + tls_s.v; }
int sw(int x) { switch (x) { case 1: return 5; case 2: return 7; case 3: return 9; case 4: return 1; default: return 0; } }
int main(int argc, char **argv) { return catcher(argc) + sw(argc); }
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arch {
    Aarch64,
    X86_64,
}

/// A flag set: its arguments, and the one architecture it is for, if it
/// is for one.
const FLAG_SETS: [(&[&str], Option<Arch>); 11] = [
    (&[], None),
    (&["-fPIC"], None),
    (&["-fPIE"], None),
    (&["-mbranch-protection=standard"], Some(Arch::Aarch64)),
    (&["-fcf-protection"], Some(Arch::X86_64)),
    (&["-fno-omit-frame-pointer"], None),
    (&["-funwind-tables"], None),
    (&["-funwind-tables", "-fno-asynchronous-unwind-tables"], None),
    (&["-gdwarf-4"], None),
    (
        &[
            "-fPIC",
            "-fno-omit-frame-pointer",
            "-funwind-tables",
            "-gdwarf-4",
            "-fstack-protector-strong",
        ],
        None,
    ),
    (&["-fPIE", "-mbranch-protection=standard", "-fno-omit-frame-pointer"], Some(Arch::Aarch64)),
];

/// The module flags of clang's output: `(behavior, name, value)`.
fn module_flags(ir: &str) -> Vec<(u32, String, Result<u64, String>)> {
    let nodes: BTreeMap<&str, &str> = ir
        .lines()
        .filter_map(|line| line.split_once(" = !{"))
        .filter(|(id, _)| id.starts_with('!'))
        .collect();
    let list = nodes.get("!llvm.module.flags").expect("clang writes module flags");
    let mut flags = Vec::new();
    for id in list.trim_end_matches('}').split(", ") {
        let node = nodes.get(id).unwrap_or_else(|| panic!("no node {id}"));
        let fields: Vec<&str> = node.trim_end_matches('}').splitn(3, ", ").collect();
        let behavior = fields[0].strip_prefix("i32 ").expect("a behavior").parse().unwrap();
        let name = fields[1].trim_start_matches("!\"").trim_end_matches('"').to_string();
        let value = match fields[2].split_once(' ') {
            Some(("i32" | "i64" | "i8" | "i1", int)) => Ok(int.parse().unwrap()),
            _ => Err(fields[2].to_string()),
        };
        flags.push((behavior, name, value));
    }
    flags
}

/// One attribute as written: `name`, `name(arg)`, `"name"` or
/// `"name"="value"`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Written {
    string: bool,
    name: String,
    value: String,
}

/// The attributes of an `attributes #N = { ... }` body.
fn group_attributes(body: &str) -> Vec<Written> {
    let bytes = body.as_bytes();
    let mut attrs = Vec::new();
    let mut at = 0;
    let quoted = |from: usize| {
        let end = from + 1 + body[from + 1..].find('"').expect("a closing quote");
        (body[from + 1..end].to_string(), end + 1)
    };
    while at < bytes.len() {
        match bytes[at] {
            b' ' => at += 1,
            b'"' => {
                let (name, next) = quoted(at);
                at = next;
                let mut value = String::new();
                if bytes.get(at) == Some(&b'=') {
                    (value, at) = quoted(at + 1);
                }
                attrs.push(Written { string: true, name, value });
            }
            _ => {
                let end = body[at..].find([' ', '(']).map_or(bytes.len(), |n| at + n);
                let name = body[at..end].to_string();
                at = end;
                let mut value = String::new();
                if bytes.get(at) == Some(&b'(') {
                    let close = at + body[at..].find(')').expect("a closing parenthesis");
                    value = body[at + 1..close].to_string();
                    at = close + 1;
                }
                attrs.push(Written { string: false, name, value });
            }
        }
    }
    attrs
}

/// The keyword attribute names of LLVM 19 and 22.
fn llvm_keywords() -> BTreeSet<&'static str> {
    [include_str!("data/attributes-llvm19.txt"), include_str!("data/attributes-llvm22.txt")]
        .into_iter()
        .flat_map(str::lines)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut words = line.split(' ');
            let form = words.next()?;
            let name = words.next()?;
            (!matches!(form, "strbool" | "complexstr")).then_some(name)
        })
        .collect()
}

/// The attribute keywords on the `define`, `declare`, `call` and
/// `invoke` lines of `ir`: every bare word that is an LLVM attribute
/// name. Names of values and quoted text are not words.
fn inline_keywords(ir: &str, keywords: &BTreeSet<&'static str>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in ir.lines() {
        let relevant = line.starts_with("define ")
            || line.starts_with("declare ")
            || line.contains(" call ")
            || line.contains(" invoke ");
        if !relevant {
            continue;
        }
        let mut in_quotes = false;
        let mut word = String::new();
        let mut sigil = false;
        for c in line.chars().chain([' ']) {
            if c == '"' {
                in_quotes = !in_quotes;
            }
            if !in_quotes && (c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                word.push(c);
                continue;
            }
            if !sigil && keywords.contains(word.as_str()) {
                found.insert(word.clone());
            }
            word.clear();
            // A word after one of these is a name, not a keyword.
            sigil = !in_quotes && matches!(c, '@' | '%' | '!' | '#' | '$' | '"');
        }
    }
    found
}

/// Puts the kept attribute `attr` of a function into `set`, or says why
/// it has no representation.
fn represent(set: &mut FnAttrs, attr: &Written) -> Result<(), String> {
    if attr.string {
        let Some((key, function)) = FnStr::from_name(attr.name.as_bytes()) else {
            return Err(format!("\"{}\" is kept and has no FnStr", attr.name));
        };
        let value = if key == FnStr::NoBuiltin { function } else { attr.value.as_bytes() };
        return set.set_string(key, value).map_err(|error| error.to_string());
    }
    match (attr.name.as_str(), attr.value.as_str()) {
        ("uwtable", "" | "async") => set.set_uwtable(UwTable::Async),
        ("uwtable", "sync") => set.set_uwtable(UwTable::Sync),
        ("alignstack", bytes) => {
            let bytes = bytes.parse().map_err(|_| format!("alignstack({bytes})"))?;
            set.set_stack_align(bytes).map_err(|error| error.to_string())?;
        }
        (name, "") => match FnAttrs::flag_named(name.as_bytes()) {
            Some(flag) => set.set(flag).map_err(|error| error.to_string())?,
            None => return Err(format!("{name} is kept and FnAttrs has no flag for it")),
        },
        (name, value) => return Err(format!("{name}({value}) is kept and has no representation")),
    }
    Ok(())
}

/// What the flag set must have made of the module properties.
fn check_props(module: &Module, arch: Arch, flags: &[&str], wrong: &mut Vec<String>) {
    let props = &module.props;
    let mut expect = |what: &str, holds: bool| {
        if !holds {
            wrong.push(format!("{what}: {props:?}"));
        }
    };
    let has = |flag: &str| flags.contains(&flag);
    expect("the debug-info version is 3", props.debug_info_version == Some(3));
    expect("the DWARF version", props.dwarf_version == Some(if has("-gdwarf-4") { 4 } else { 5 }));
    // Debian's clang makes PIE the default.
    expect("PIC level 2", props.pic == PicLevel::Big);
    let pie = if has("-fPIC") { PicLevel::None } else { PicLevel::Big };
    expect("the PIE level", props.pie == pie);
    // Asynchronous tables are the default on both targets, and
    // `-funwind-tables` alone does not lower them to synchronous ones.
    let uwtable =
        if has("-fno-asynchronous-unwind-tables") { UwTable::Sync } else { UwTable::Async };
    expect("the unwind-table kind", props.uwtable == uwtable);
    expect("a frame pointer at -O0", props.frame_pointer != FramePointer::None);
    if arch == Arch::X86_64 {
        expect("every function has a frame pointer", props.frame_pointer == FramePointer::All);
    }
    expect("the default code model", props.code_model.is_none());
    let protection = props.branch_protection;
    if has("-mbranch-protection=standard") {
        expect(
            "BTI and return-address signing",
            protection.has(BranchProtection::BTI | BranchProtection::PAC_RET),
        );
        expect(
            "signing of non-leaf functions with the A key",
            !protection.has(BranchProtection::PAC_RET_ALL)
                && !protection.has(BranchProtection::PAC_B_KEY),
        );
        expect(
            "no x86 protection",
            protection.bits() & (BranchProtection::CET_IBT | BranchProtection::CET_SHSTK) == 0,
        );
    } else if has("-fcf-protection") {
        expect(
            "IBT and shadow stack",
            protection.bits() == BranchProtection::CET_IBT | BranchProtection::CET_SHSTK,
        );
    } else {
        expect("no branch protection", protection == BranchProtection::NONE);
    }
}

/// What the runs found.
#[derive(Default)]
struct Survey {
    /// Everything hod would refuse or get wrong, one line each.
    wrong: Vec<String>,
    flags: BTreeSet<String>,
    /// `(is a string attribute, name)` to its class.
    attrs: BTreeMap<(bool, String), AttrClass>,
    runs: usize,
}

impl Survey {
    /// Puts the module flags and attributes of one clang output through
    /// a module.
    fn unit(
        &mut self,
        mut module: Module,
        arch: Arch,
        flags: &[&str],
        ir: &str,
        keywords: &BTreeSet<&'static str>,
    ) {
        self.runs += 1;
        for (behavior, name, value) in module_flags(ir) {
            let value = match &value {
                Ok(int) => FlagValue::Int(*int),
                Err(_) => FlagValue::Other,
            };
            if let Err(error) = module.set_module_flag(behavior, name.as_bytes(), value) {
                self.wrong.push(error.to_string());
            }
            self.flags.insert(name);
        }
        for flag in module.other_flags().filter(|flag| !flag.ignorable) {
            self.wrong.push(format!("module flag {:?}", String::from_utf8_lossy(flag.name)));
        }
        check_props(&module, arch, flags, &mut self.wrong);

        let groups = ir.lines().filter_map(|line| {
            let body = line.strip_prefix("attributes #")?.split_once(" = { ")?.1;
            body.strip_suffix(" }")
        });
        for body in groups {
            let mut set = FnAttrs::new();
            for attr in group_attributes(body) {
                let syntax = if attr.string { AttrSyntax::String } else { AttrSyntax::Keyword };
                let class = attr_class(syntax, attr.name.as_bytes());
                self.attrs.insert((attr.string, attr.name.clone()), class);
                match class {
                    AttrClass::Rejected => self.wrong.push(format!("attribute {attr:?}")),
                    AttrClass::Dropped => {}
                    AttrClass::Kept => {
                        if let Err(error) = represent(&mut set, &attr) {
                            self.wrong.push(error);
                        }
                    }
                }
            }
            let id = module.intern_fn_attrs(&set).unwrap();
            assert_eq!(module.fn_attrs(id).flags(), set.flags());
            assert!(module.fn_attrs(id).strings().eq(set.strings()));
        }
        for name in inline_keywords(ir, keywords) {
            let class = attr_class(AttrSyntax::Keyword, name.as_bytes());
            if class == AttrClass::Rejected {
                self.wrong.push(format!("attribute {name}"));
            }
            self.attrs.insert((false, name), class);
        }
    }
}

/// §2.1, §5.7: nothing clang emits for these inputs is unknown to hod.
#[test]
fn clang_output_has_nothing_unknown() {
    let keywords = llvm_keywords();
    let mut survey = Survey::default();
    for major in MAJORS {
        let Some(clang) = llvm_tool(major, "clang") else { continue };
        let scratch = Scratch::new(&format!("clang-survey-{major}"));
        let c = scratch.write("unit.c", C_SOURCE);
        let cxx = scratch.write("unit.cpp", CXX_SOURCE);
        for (arch, triple, target) in [
            (Arch::Aarch64, "aarch64-unknown-linux-gnu", TargetSpec::aarch64_linux_gnu()),
            (Arch::X86_64, "x86_64-unknown-linux-gnu", TargetSpec::x86_64_linux_gnu()),
        ] {
            for (flags, only) in FLAG_SETS {
                if only.is_some_and(|only| only != arch) {
                    continue;
                }
                for source in [&c, &cxx] {
                    let output = scratch.join("unit.ll");
                    let mut args: Vec<&std::ffi::OsStr> =
                        vec!["-O0".as_ref(), "-g".as_ref(), "-S".as_ref(), "-emit-llvm".as_ref()];
                    let target_arg = format!("--target={triple}");
                    args.push(target_arg.as_ref());
                    args.extend(flags.iter().map(std::ffi::OsStr::new));
                    args.extend(["-o".as_ref(), output.as_os_str(), source.as_os_str()]);
                    let outcome = hod_testutil::run(&clang, args);
                    let run = format!("clang {major} {triple} {flags:?} {}", source.display());
                    assert!(outcome.success(), "{run}: {}", outcome.stderr_text());
                    let ir = std::fs::read_to_string(&output).unwrap();
                    let before = survey.wrong.len();
                    survey.unit(Module::new(target.clone()), arch, flags, &ir, &keywords);
                    for message in &mut survey.wrong[before..] {
                        write!(message, " [{run}]").unwrap();
                    }
                }
            }
        }
    }
    let Survey { wrong, flags: seen_flags, attrs: seen_attrs, runs } = survey;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    if runs == 0 {
        return;
    }
    // The survey saw what the lists were built from: every flag below,
    // and attributes of both kept and dropped classes in both syntaxes.
    eprintln!("clang survey: {runs} runs");
    eprintln!("  module flags: {seen_flags:?}");
    for class in [AttrClass::Kept, AttrClass::Dropped] {
        let names: Vec<String> = seen_attrs
            .iter()
            .filter(|(_, seen)| **seen == class)
            .map(|((string, name), _)| if *string { format!("\"{name}\"") } else { name.clone() })
            .collect();
        eprintln!("  {class:?}: {}", names.join(" "));
    }
    for flag in [
        "PIC Level",
        "PIE Level",
        "uwtable",
        "frame-pointer",
        "Dwarf Version",
        "Debug Info Version",
        "wchar_size",
    ] {
        assert!(seen_flags.contains(flag), "no run had the module flag {flag}");
    }
    for (string, name) in [
        (false, "nounwind"),
        (false, "noreturn"),
        (false, "uwtable"),
        (false, "sret"),
        (false, "signext"),
        (false, "alwaysinline"),
        (false, "sspstrong"),
        (false, "noinline"),
        (false, "optnone"),
        (false, "noundef"),
        (false, "builtin"),
        (true, "target-cpu"),
        (true, "frame-pointer"),
        (true, "stack-protector-buffer-size"),
        (true, "no-trapping-math"),
    ] {
        assert!(seen_attrs.contains_key(&(string, name.to_string())), "no run had {name}");
    }
}
