//! `hod::Session` (`spec/cli.md` "Session"; design 0004 §4): the seed
//! corpus fed through the public session gives `compile`'s bytes, bodies
//! in any order; what the session declines and when; every misuse is an
//! error value.

use std::path::PathBuf;
use std::time::Instant;

use hod::{
    CompileError, Construct, OptLevel, Options, OutputKind, RelocModel, Report, Session, Settings,
    Unsupported,
};
use hod_ir::{Body, Module, SymId, TargetSpec};

const TRIPLE: &str = "target triple = \"aarch64-unknown-linux-gnu\"\n";

fn options() -> Options {
    Options {
        output_kind: OutputKind::Object,
        opt_level: OptLevel::O0,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    }
}

/// A module read by `hod_ll`: its module level and every body it could
/// build (`Err` with the names it refused otherwise), in file order.
struct Read {
    module: Module,
    bodies: Vec<(SymId, Result<Body, Vec<String>>)>,
}

fn read(text: &[u8]) -> Read {
    let parsed = hod_ll::parse_module(text, &hod_ll::ParseOptions::default()).expect("readable");
    let mut module = parsed.module;
    let mut parser = hod_ll::BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        let body = parser.parse(&mut module, function).expect("a readable body");
        let body = body.ok_or_else(|| parser.unsupported().map(str::to_owned).collect());
        bodies.push((function.sym, body));
    }
    Read { module, bodies }
}

/// `read`'s module through the public session, the bodies in the order
/// `order` gives.
fn through_session(
    read: Read,
    options: &Options,
    settings: Settings,
    order: impl Fn(&mut Vec<(SymId, Result<Body, Vec<String>>)>),
) -> Result<(Vec<u8>, Report), CompileError> {
    let Read { module, mut bodies } = read;
    let mut session = Session::with_module(module, options, settings)?;
    session.end_module()?;
    order(&mut bodies);
    for (sym, body) in bodies {
        match body {
            Ok(body) => session.body(sym, body)?,
            Err(names) => session.unsupported_body(sym, names.iter().map(String::as_str))?,
        }
    }
    let mut out = Vec::new();
    let report = session.finish(&mut out).map_err(|(error, _)| error)?;
    Ok((out, report))
}

fn seed_modules() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ir");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/ir")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "ll"))
        .collect();
    files.sort();
    let mut modules = Vec::new();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).expect("a seed test");
        // `;@ module:` splits an entry of several modules.
        let mut parts: Vec<(String, String)> = Vec::new();
        for line in text.split_inclusive('\n') {
            if let Some(module) = line.strip_prefix(";@ module:") {
                parts.push((format!("{name}#{}", module.trim()), String::new()));
            }
            if let Some((_, part)) = parts.last_mut() {
                part.push_str(line);
            }
        }
        if parts.is_empty() {
            parts.push((name, text));
        }
        // The seed tests leave the target to `irdiff`: AArch64 here.
        modules.extend(parts.into_iter().map(|(name, text)| (name, format!("{TRIPLE}{text}"))));
    }
    modules
}

// spec/cli.md "Session": every seed module fed from `hod_ll` through the
// public session gives `compile`'s answer, bytes or reason; and with the
// bodies handed over last to first, an object's bytes are the same (the
// object lays them out in module order, not in arrival order).
#[test]
fn seed_corpus_through_the_session_is_compile() {
    let options = options();
    let mut built = 0;
    let mut reordered = 0;
    for (name, text) in seed_modules() {
        let compiled = hod::compile(text.as_bytes(), &options);
        let forward = through_session(read(text.as_bytes()), &options, Settings::default(), |_| {});
        assert_eq!(forward.as_ref().map(|(o, _)| o), compiled.as_ref(), "{name}");
        let Ok(object) = compiled else { continue };
        built += 1;
        let read = read(text.as_bytes());
        reordered += usize::from(read.bodies.len() > 1);
        let backward = through_session(read, &options, Settings::default(), |b| b.reverse());
        assert_eq!(backward.map(|(o, _)| o), Ok(object), "{name}, bodies last to first");
    }
    assert!(built >= 30 && reordered >= 30, "{built} built, {reordered} with several bodies");
}

// spec/cli.md "Session", "The answer": the object is appended; on an
// error the buffer is as it was.
#[test]
fn the_object_is_appended_and_an_error_leaves_the_buffer() {
    let text = format!("{TRIPLE}define i32 @f() nounwind {{\n  ret i32 7\n}}\n");
    let object = hod::compile(text.as_bytes(), &options()).expect("an object");
    let first = read(text.as_bytes());
    let mut session = Session::with_module(first.module, &options(), Settings::default()).unwrap();
    session.end_module().unwrap();
    for (sym, body) in first.bodies {
        session.body(sym, body.unwrap()).unwrap();
    }
    let mut out = b"prefix".to_vec();
    let report = session.finish(&mut out).expect("an object");
    assert_eq!(&out[..6], b"prefix");
    assert_eq!(&out[6..], &object[..]);
    let phases: Vec<&str> = report.phases.iter().map(|&(name, _)| name).collect();
    assert_eq!(phases, ["classify", "bodies", "emission", "object"]);

    // A refused body: every later call answers it, `finish` too, and no
    // object of the other bodies is written.
    let ghc = "define ghccc void @h() nounwind {\n  ret void\n}\n";
    let text = format!("{TRIPLE}{ghc}define i32 @f() nounwind {{\n  ret i32 7\n}}\n");
    let read = read(text.as_bytes());
    let mut session = Session::with_module(read.module, &options(), Settings::default()).unwrap();
    session.end_module().unwrap();
    let mut bodies = read.bodies.into_iter();
    let (h, h_body) = bodies.next().unwrap();
    let (f, f_body) = bodies.next().unwrap();
    let refused: CompileError =
        Unsupported::in_function(Construct::Named("cc:ghccc".into()), Some("h".into())).into();
    assert_eq!(session.body(h, h_body.unwrap()), Err(refused.clone()));
    assert_eq!(session.body(f, f_body.unwrap()), Err(refused.clone()));
    let mut out = b"prefix".to_vec();
    let (error, report) = session.finish(&mut out).expect_err("declined");
    assert_eq!(error, refused);
    assert_eq!(out, b"prefix");
    let phases: Vec<&str> = report.phases.iter().map(|&(name, _)| name).collect();
    assert_eq!(phases, ["classify"]);
}

// spec/cli.md "Session", "The calls": what `new` judges, and an empty
// module built from nothing.
#[test]
fn what_new_declines_and_an_empty_module() {
    let target = || TargetSpec::from_triple("aarch64-unknown-linux-gnu").unwrap();
    let declined = |options: &Options| {
        Session::new(target(), options, Settings::default()).map(|_| ()).unwrap_err()
    };
    let module = |construct| CompileError::from(Unsupported::module(construct));
    assert_eq!(declined(&Options::default()), module(Construct::AssemblyOutput));
    let march = Options { arch: Some("aarch64".into()), ..options() };
    assert_eq!(declined(&march), module(Construct::UnsupportedTarget));
    let other = Options { triple: Some("x86_64-pc-linux-gnu".into()), ..options() };
    assert_eq!(declined(&other), module(Construct::UnsupportedTarget));
    let bogus = Options { triple: Some("riscv64-unknown-linux-gnu".into()), ..options() };
    assert_eq!(declined(&bogus), module(Construct::UnsupportedTarget));

    let same = Options { triple: Some("aarch64-linux-gnu".into()), ..options() };
    let mut session = Session::new(target(), &same, Settings::default()).expect("a session");
    session.module().record_unsupported("global:partition", None);
    let refused = Err(module(Construct::Named("global:partition".into())));
    assert_eq!(session.end_module(), refused);
    let mut out = Vec::new();
    let (error, report) = session.finish(&mut out).expect_err("declined");
    assert_eq!(Err(error), refused);
    assert_eq!(report.target.as_deref(), Some("aarch64-linux-gnu"));

    let started = Instant::now();
    let mut session = Session::new(target(), &options(), Settings::default()).unwrap();
    session.end_module().unwrap();
    let report = session.finish(&mut out).expect("an empty object");
    assert!(out.starts_with(b"\x7FELF"));
    assert_eq!(report.target.as_deref(), Some("aarch64-unknown-linux-gnu"));
    // The two clocks cover disjoint parts of the session's life.
    assert!(report.bridge + report.session <= started.elapsed());
}

// spec/cli.md "Session", "The answer": with the census the calls go on,
// every reason is in the report, and the first is the answer.
#[test]
fn census_through_the_session() {
    let text = format!(
        "{TRIPLE}module asm \"nop\"\n\
         define ghccc void @h() nounwind {{\n  ret void\n}}\n\
         define void @g() nounwind {{\n  ret void\n}}\n"
    );
    let read = read(text.as_bytes());
    let census = Settings { census: true, ..Settings::default() };
    let mut session = Session::with_module(read.module, &options(), census).unwrap();
    assert_eq!(session.end_module(), Ok(()));
    let mut bodies = read.bodies.into_iter();
    let (h, _) = bodies.next().unwrap();
    assert_eq!(session.unsupported_body(h, ["call:reader-name", "call:second"]), Ok(()));
    let (g, g_body) = bodies.next().unwrap();
    assert_eq!(session.body(g, g_body.unwrap()), Ok(()));
    let (error, report) = session.finish(&mut Vec::new()).expect_err("declined");
    assert_eq!(error, Unsupported::module(Construct::Named("module:asm".into())).into());
    for name in ["module:asm", "call:reader-name", "call:second"] {
        assert!(report.reasons.iter().any(|r| r == name), "{name}: {:?}", report.reasons);
    }
    assert_eq!(report.reasons[0], "module:asm");

    // Without the census, the producer's refusal names its function.
    let read = self::read(text.replace("module asm \"nop\"\n", "").as_bytes());
    let mut session = Session::with_module(read.module, &options(), Settings::default()).unwrap();
    session.end_module().unwrap();
    let (h, _) = &read.bodies[0];
    let refused =
        Unsupported::in_function(Construct::Named("call:reader-name".into()), Some("h".into()));
    assert_eq!(session.unsupported_body(*h, ["call:reader-name"]), Err(refused.into()));
    let nothing = session.unsupported_body(*h, []);
    assert!(matches!(nothing, Err(CompileError::Unsupported(_))), "{nothing:?}");
}

/// A module of two defined functions and a declaration, read.
fn two_functions() -> (Read, SymId) {
    let text = format!(
        "{TRIPLE}declare void @d()\n\
         define void @a() nounwind {{\n  ret void\n}}\n\
         define void @b() nounwind {{\n  ret void\n}}\n"
    );
    let read = read(text.as_bytes());
    let d = read.module.find_symbol(b"d").expect("@d");
    (read, d)
}

fn misuse(message: &str) -> CompileError {
    CompileError::Internal(format!("session: {message}"))
}

fn session_of(read: Read) -> (Session, Vec<(SymId, Body)>) {
    let session = Session::with_module(read.module, &options(), Settings::default()).unwrap();
    let bodies = read.bodies.into_iter().map(|(sym, body)| (sym, body.unwrap())).collect();
    (session, bodies)
}

// spec/cli.md "Session", "Misuse": each is an error value, the session
// answers it from then on, and nothing panics.
#[test]
fn misuse_is_an_error() {
    // A body before `end_module`; `finish` then answers it.
    let (mut session, mut bodies) = session_of(two_functions().0);
    let (a, body) = bodies.remove(0);
    let before = misuse("a body before end_module");
    assert_eq!(session.body(a, body), Err(before.clone()));
    assert_eq!(session.end_module(), Err(before.clone()));
    assert_eq!(session.finish(&mut Vec::new()).map_err(|(e, _)| e), Err(before));

    // `unsupported_body` before `end_module`, too.
    let (mut session, bodies) = session_of(two_functions().0);
    let refused = session.unsupported_body(bodies[0].0, ["x:y"]);
    assert_eq!(refused, Err(misuse("a body before end_module")));

    // `finish` before `end_module`.
    let (session, _) = session_of(two_functions().0);
    let mut out = b"kept".to_vec();
    let (error, _) = session.finish(&mut out).unwrap_err();
    assert_eq!(error, misuse("finish before end_module"));
    assert_eq!(out, b"kept");

    // `end_module` twice.
    let (mut session, _) = session_of(two_functions().0);
    session.end_module().unwrap();
    assert_eq!(session.end_module(), Err(misuse("end_module after end_module")));

    // A symbol twice.
    let (read, _) = two_functions();
    let (mut session, mut bodies) = session_of(read);
    session.end_module().unwrap();
    let (a, body) = bodies.remove(0);
    session.body(a, body.clone()).unwrap();
    assert_eq!(session.body(a, body), Err(misuse("a second body for 'a'")));
    let (b, body) = bodies.remove(0);
    assert_eq!(session.body(b, body), Err(misuse("a second body for 'a'")));

    // A symbol the module does not have, a declaration, another
    // symbol's body.
    let (read, d) = two_functions();
    let cases: [(Box<dyn Fn(&mut Vec<(SymId, Body)>) -> (SymId, Body)>, &str); 3] = [
        (
            Box::new(|bodies| (SymId::new(1000), bodies.remove(0).1)),
            "a body for sym1000, not a symbol of the module",
        ),
        (
            Box::new(move |bodies| (d, bodies.remove(0).1)),
            "a body for 'd', not a function with a body",
        ),
        (
            Box::new(|bodies| {
                let (b, _) = bodies.remove(1);
                (b, bodies.remove(0).1)
            }),
            "the body of sym1 handed over for 'b'",
        ),
    ];
    let mut read = Some(read);
    for (pick, message) in cases {
        let (mut session, mut bodies) =
            session_of(read.take().unwrap_or_else(|| two_functions().0));
        session.end_module().unwrap();
        let (sym, body) = pick(&mut bodies);
        assert_eq!(session.body(sym, body), Err(misuse(message)), "{message}");
        let (error, _) = session.finish(&mut Vec::new()).unwrap_err();
        assert_eq!(error, misuse(message));
    }
    let (mut session, _) = session_of(two_functions().0);
    session.end_module().unwrap();
    let refused = session.unsupported_body(SymId::new(1000), ["x:y"]);
    assert_eq!(refused, Err(misuse("a body for sym1000, not a symbol of the module")));

    // A body never delivered.
    let (mut session, mut bodies) = session_of(two_functions().0);
    session.end_module().unwrap();
    let (a, body) = bodies.remove(0);
    session.body(a, body).unwrap();
    let mut out = Vec::new();
    let (error, _) = session.finish(&mut out).unwrap_err();
    assert_eq!(error, misuse("no body was delivered for 'b'"));
    assert!(out.is_empty());
}

/// clang 22's `-g -O0` IR of a small C function with a retained type
/// scoped in a lexical block, which the unit's plan refuses
/// (`debug:local-type-scope`), and a second function whose target
/// feature code generation refuses.
const PLAN_REFUSED: &str = r#"target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128"
target triple = "aarch64-unknown-linux-gnu"

define dso_local i32 @g(i32 noundef %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !15, !DIExpression(), !16)
  %3 = load i32, ptr %2, align 4, !dbg !16
  ret i32 %3, !dbg !16
}

define dso_local void @h() #1 {
  ret void
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="non-leaf" }
attributes #1 = { noinline nounwind optnone "target-features"="+bogus-feature" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None, retainedTypes: !29)
!1 = !DIFile(filename: "tiny.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "x", arg: 1, scope: !10, file: !1, line: 1, type: !13)
!16 = !DILocation(line: 1, column: 11, scope: !10)
!29 = !{!30}
!30 = !DIDerivedType(tag: DW_TAG_typedef, name: "T", scope: !31, file: !1, line: 2, baseType: !13)
!31 = distinct !DILexicalBlock(scope: !10, file: !1, line: 2, column: 3)
"#;

// spec/cli.md "Session", step 5, and spec/dwarf.md §6.12: `finish` runs
// the unit's plan before any code is generated, so the plan's refusal is
// the answer, not code generation's.
#[test]
fn the_unit_plan_comes_before_code_generation() {
    let refusal = |text: &str| {
        let session =
            through_session(read(text.as_bytes()), &options(), Settings::default(), |_| {});
        match session {
            Err(CompileError::Unsupported(u)) => u.construct.name().to_owned(),
            Err(other) => panic!("{other:?}"),
            Ok((_, report)) => panic!("built: {:?}", report.phases),
        }
    };
    // Each refusal alone, then both: the plan's wins.
    let without_plan = PLAN_REFUSED.replace(", retainedTypes: !29", "");
    assert!(
        refusal(&without_plan).starts_with("attr:target-feature-"),
        "{}",
        refusal(&without_plan)
    );
    let without_feature = PLAN_REFUSED.replace(r#" "target-features"="+bogus-feature""#, "");
    assert_eq!(refusal(&without_feature), "debug:local-type-scope");
    assert_eq!(refusal(PLAN_REFUSED), "debug:local-type-scope");
}
