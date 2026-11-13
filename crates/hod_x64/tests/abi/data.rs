//! The design's experiments (`tests/abi/data/*.ll`, a copy of design 0006
//! §3's data with `llc` 19 and 22's output beside each): every function
//! they define or declare is assigned (`spec/abi-x64.md` §10 names the
//! refused ones) and run through the grid against `llc`.

use std::collections::BTreeMap;
use std::path::Path;

use hod_cg::abi::{Assignment, Sig};
use hod_ir::{CallConv, ParamAttrs, ParamMem, SymbolKind, TypeId, TypeKind, TypeTable};
use hod_x64::abi::{Features, assign};

use crate::grid::run_cases;
use crate::run::Tools;
use crate::sigs::{Attr, Case, Param};
use crate::ty::Ty;

/// The functions of the data that `assign` refuses, and the name.
const REFUSED: [(&str, &str); 3] = [
    // §6.3.
    ("06-ptrattrs.ll:f_nest", "x64:nest"),
    // §1.2: `swiftcc`.
    ("06-ptrattrs.ll:f_swift", "abi:cc"),
    ("06-ptrattrs.ll:call_swift", "abi:cc"),
];

fn data_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/abi/data"))
}

/// The test form of `ty`, if the tests have one.
fn ty_of(types: &TypeTable, ty: TypeId) -> Option<Ty> {
    Some(match types.kind(ty) {
        TypeKind::Int(bits) => Ty::Int(bits),
        TypeKind::Ptr { addr_space: 0 } => Ty::Ptr,
        TypeKind::Half => Ty::Half,
        TypeKind::BFloat => Ty::BFloat,
        TypeKind::Float => Ty::Float,
        TypeKind::Double => Ty::Double,
        TypeKind::Fp128 => Ty::Fp128,
        TypeKind::X86Fp80 => Ty::X86Fp80,
        TypeKind::Vector { elem, len } => Ty::Vec(Box::new(ty_of(types, elem)?), len),
        TypeKind::Array { elem, len } => Ty::Arr(Box::new(ty_of(types, elem)?), len),
        TypeKind::Struct { fields, packed, .. } => {
            let fields = fields.iter().map(|&f| ty_of(types, f)).collect::<Option<Vec<_>>>()?;
            if packed { Ty::Packed(fields) } else { Ty::Struct(fields) }
        }
        _ => return None,
    })
}

fn attr_of(types: &TypeTable, attrs: ParamAttrs) -> Option<Attr> {
    Some(match attrs.mem() {
        ParamMem::Sret => Attr::Sret,
        ParamMem::ByRef => Attr::ByRef,
        ParamMem::ByVal => Attr::ByVal {
            pointee: ty_of(types, attrs.mem_type()?)?,
            align: attrs.align(),
            stack_align: attrs.stack_align(),
        },
        ParamMem::None if attrs.has(ParamAttrs::SIGNEXT) => Attr::Ext(true),
        ParamMem::None if attrs.has(ParamAttrs::ZEROEXT) => Attr::Ext(false),
        ParamMem::None if attrs.has(ParamAttrs::NEST) => Attr::Nest,
        ParamMem::None if attrs.has(ParamAttrs::INREG) => Attr::InReg,
        ParamMem::None if attrs.stack_align().is_some() => Attr::StackAlign(attrs.stack_align()?),
        ParamMem::None => Attr::None,
        _ => return None,
    })
}

/// One function of the data: its label (`file:name`), what `assign`
/// answers, and its test form when it has one.
struct Function {
    label: String,
    answer: Result<Assignment, &'static str>,
    case: Option<Case>,
}

fn functions() -> Vec<Function> {
    let mut files: Vec<_> = std::fs::read_dir(data_dir())
        .expect("the data directory")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ll"))
        .collect();
    files.sort();
    assert!(files.len() >= 15, "the data has {} files", files.len());
    let mut all = Vec::new();
    for file in files {
        let name = file.file_name().expect("a name").to_string_lossy().into_owned();
        let source = std::fs::read(&file).expect("the file");
        let options =
            hod_ll::ParseOptions { default_target: Some(hod_ir::TargetSpec::x86_64_linux_gnu()) };
        let parsed = hod_ll::parse_module(&source, &options)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let module = &parsed.module;
        let types = &module.types;
        for symbol in module.symbols() {
            let SymbolKind::Function { cc, ret_attrs, .. } = symbol.kind() else { continue };
            let function = String::from_utf8_lossy(symbol.name()).into_owned();
            if function.starts_with("llvm.") {
                continue;
            }
            let TypeKind::Function { ret, params, variadic } = types.kind(symbol.ty()) else {
                continue;
            };
            let attrs: Vec<ParamAttrs> = (0..params.len()).map(|i| symbol.param_attrs(i)).collect();
            let sig = Sig {
                cc,
                ret,
                ret_attrs,
                params,
                param_attrs: &attrs,
                fixed: params.len() as u32,
                variadic,
            };
            let mut out = Assignment::default();
            let answer = assign(types, &sig, Features::default(), &mut out)
                .map(|()| out)
                .map_err(|unsupported| unsupported.0);
            let case = (|| {
                let params = params
                    .iter()
                    .zip(&attrs)
                    .map(|(&ty, &attr)| {
                        Some(Param { ty: ty_of(types, ty)?, attr: attr_of(types, attr)? })
                    })
                    .collect::<Option<Vec<_>>>()?;
                let mut case = Case::new(params).cc(cc);
                if ret != TypeId::VOID {
                    let ty = ty_of(types, ret)?;
                    case = match attr_of(types, ret_attrs)? {
                        Attr::Ext(signed) => case.ret_ext(ty, signed),
                        Attr::None => case.ret(ty),
                        _ => return None,
                    };
                }
                if variadic {
                    let fixed = case.params.len();
                    case = case.variadic(fixed);
                }
                Some(case)
            })();
            all.push(Function { label: format!("{name}:{function}"), answer, case });
        }
    }
    all
}

/// `spec/abi-x64.md` §10: `assign` answers every function of the data,
/// refusing exactly the ones [`REFUSED`] names.
#[test]
fn data_signatures_assign() {
    let refused: BTreeMap<&str, &str> = REFUSED.into_iter().collect();
    let all = functions();
    assert!(all.len() > 80, "only {} functions", all.len());
    for function in &all {
        let want = refused.get(function.label.as_str()).copied();
        match (&function.answer, want) {
            (Ok(_), None) => {}
            (Err(got), Some(want)) => assert_eq!(*got, want, "{}", function.label),
            (got, want) => panic!("{}: got {got:?}, want refusal {want:?}", function.label),
        }
    }
    for label in refused.keys() {
        assert!(all.iter().any(|f| f.label == *label), "{label} is not in the data");
    }
}

/// `spec/abi-x64.md` §3–§9: every accepted function of the data, as hod
/// caller and callee against `llc` 22 and 19.
#[test]
fn data_signatures_against_llc() {
    let Some(tools) = Tools::find() else { return };
    let mut cases: Vec<Case> = Vec::new();
    for function in functions() {
        if function.answer.is_err() {
            continue;
        }
        let case = function.case.unwrap_or_else(|| panic!("{}: no test form", function.label));
        // §1.2: `fastcc` is checked as itself.
        assert!(case.cc == CallConv::C || case.cc == CallConv::FAST, "{}", function.label);
        if !cases.contains(&case) {
            cases.push(case);
        }
    }
    let report = run_cases("abi-data", &cases, 0xda7a, &tools);
    eprintln!("data: {} signatures; {}", cases.len(), report.summary());
    report.check("data");
}
