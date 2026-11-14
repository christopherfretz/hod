//! The LLVM rules `hod_ir` encodes for function bodies (`spec/ir.md` §5),
//! checked against LLVM 22's own tools: which opcodes end a block, which
//! casts, atomic accesses and aggregate index paths are valid, how
//! intrinsic names are mangled, and that the intrinsics a producer leaves
//! out generate no code.
//!
//! Each test asks hod (`Body::verify`, `aggregate_field`,
//! `resolve_intrinsic`, ...) and LLVM the same question about every case
//! of a grid and fails on any disagreement. Without the tools the tests
//! skip, unless `HOD_REQUIRE_TOOLS=1`.

mod body_support;
mod common;

use std::path::Path;
use std::process::Command;

use body_support::{Rng, assembles, llvm_tool, parse_type, type_text};
use hod_ir::{
    AARCH64_LINUX_DATA_LAYOUT, AtomicOrdering, FunctionBuilder, Intrinsic, MemFlags, Module,
    NoCode, Opcode, RmwOp, SymId, SyncScope, TargetSpec, TypeId, TypeKind, Value, aggregate_field,
    no_code_intrinsic, no_code_names, resolve_intrinsic,
};
use hod_testutil::Scratch;

/// The oracle. LLVM 19 lacks `ptrtoaddr`, four `atomicrmw` operations and
/// several intrinsics, and gives others different signatures.
const LLVM: u32 = 22;

/// The start of a module for the target of [`module`].
fn header() -> String {
    format!(
        "target datalayout = \"{AARCH64_LINUX_DATA_LAYOUT}\"\n\
         target triple = \"aarch64-unknown-linux-gnu\"\n"
    )
}

fn module() -> Module {
    Module::new(TargetSpec::aarch64_linux_gnu())
}

/// One question: LLVM text defining `@F`, and whether hod takes the same
/// thing.
struct Case {
    label: String,
    text: String,
    hod: bool,
}

/// Fails unless `llvm-as` accepts exactly the cases hod does.
///
/// The cases hod accepts go to `llvm-as` as one module; each case hod
/// rejects goes alone, since `llvm-as` stops at its first error.
fn agree(llvm_as: &Path, what: &str, prelude: &str, cases: &[Case]) {
    let text = |indices: &[usize]| {
        let mut text = header() + prelude;
        for &index in indices {
            text += &cases[index].text.replace("@F(", &format!("@f{index}("));
        }
        text
    };
    let first_line = |error: String| error.lines().next().unwrap_or_default().to_string();
    let (accepted, rejected): (Vec<usize>, Vec<usize>) =
        (0..cases.len()).partition(|&index| cases[index].hod);
    let mut wrong = Vec::new();
    if let Err(error) = assembles(llvm_as, &text(&accepted)) {
        for &index in &accepted {
            if let Err(error) = assembles(llvm_as, &text(&[index])) {
                let error = first_line(error);
                wrong.push(format!("hod accepts, LLVM rejects: {} ({error})", cases[index].label));
            }
        }
        assert!(!wrong.is_empty(), "{what}: the accepted cases fail only together:\n{error}");
    }
    // A few threads: each question is one short process.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let chunk = rejected.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = rejected
            .chunks(chunk)
            .map(|chunk| {
                let text = &text;
                scope.spawn(move || {
                    let taken = |&&index: &&usize| assembles(llvm_as, &text(&[index])).is_ok();
                    chunk.iter().filter(taken).copied().collect::<Vec<usize>>()
                })
            })
            .collect();
        for handle in handles {
            for index in handle.join().expect("the worker finishes") {
                wrong.push(format!("hod rejects, LLVM accepts: {}", cases[index].label));
            }
        }
    });
    eprintln!(
        "{what}: {} cases; hod and LLVM {LLVM} accept {} and reject {}",
        cases.len(),
        accepted.len(),
        rejected.len()
    );
    assert!(wrong.is_empty(), "{what}: {} disagreements:\n{}", wrong.len(), wrong.join("\n"));
}

/// Whether the body `ret (params) { entry: <build>; ret <its result> }`
/// verifies.
fn verifies(
    m: &mut Module,
    ret: TypeId,
    params: &[TypeId],
    build: impl FnOnce(&mut FunctionBuilder, &[Value]) -> Option<Value>,
) -> bool {
    let fn_ty = m.types.function(ret, params, false).unwrap();
    let mut b = FunctionBuilder::new(m, SymId::new(0), fn_ty).unwrap();
    let args: Vec<Value> = (0..b.arg_count()).map(|index| b.arg(index).unwrap()).collect();
    let entry = b.create_block().unwrap();
    b.begin_block(entry).unwrap();
    let result = build(&mut b, &args);
    b.ret(result).unwrap();
    b.finish().unwrap().verify(m).is_ok()
}

// ---- terminators ----

/// One instruction of each opcode, as LLVM writes it, for the function
/// [`placed`] makes.
#[allow(clippy::enum_glob_use, clippy::match_same_arms)]
fn sample(op: Opcode) -> &'static str {
    use Opcode::*;
    match op {
        Add => "%x = add i32 %a, %a",
        Sub => "%x = sub i32 %a, %a",
        Mul => "%x = mul i32 %a, %a",
        UDiv => "%x = udiv i32 %a, %a",
        SDiv => "%x = sdiv i32 %a, %a",
        URem => "%x = urem i32 %a, %a",
        SRem => "%x = srem i32 %a, %a",
        Shl => "%x = shl i32 %a, %a",
        LShr => "%x = lshr i32 %a, %a",
        AShr => "%x = ashr i32 %a, %a",
        And => "%x = and i32 %a, %a",
        Or => "%x = or i32 %a, %a",
        Xor => "%x = xor i32 %a, %a",
        FAdd => "%x = fadd float %f, %f",
        FSub => "%x = fsub float %f, %f",
        FMul => "%x = fmul float %f, %f",
        FDiv => "%x = fdiv float %f, %f",
        FRem => "%x = frem float %f, %f",
        FNeg => "%x = fneg float %f",
        ICmp => "%x = icmp eq i32 %a, %a",
        FCmp => "%x = fcmp oeq float %f, %f",
        Trunc => "%x = trunc i32 %a to i8",
        ZExt => "%x = zext i32 %a to i64",
        SExt => "%x = sext i32 %a to i64",
        FPTrunc => "%x = fptrunc double %d to float",
        FPExt => "%x = fpext float %f to double",
        FPToUI => "%x = fptoui float %f to i32",
        FPToSI => "%x = fptosi float %f to i32",
        UIToFP => "%x = uitofp i32 %a to float",
        SIToFP => "%x = sitofp i32 %a to float",
        PtrToInt => "%x = ptrtoint ptr %p to i64",
        PtrToAddr => "%x = ptrtoaddr ptr %p to i64",
        IntToPtr => "%x = inttoptr i64 %l to ptr",
        BitCast => "%x = bitcast i32 %a to float",
        AddrSpaceCast => "%x = addrspacecast ptr %p to ptr addrspace(1)",
        Load => "%x = load i32, ptr %p",
        Store => "store i32 %a, ptr %p",
        Alloca => "%x = alloca i32, i32 %a",
        Fence => "fence seq_cst",
        CmpXchg => "%x = cmpxchg ptr %p, i32 %a, i32 %a seq_cst seq_cst",
        AtomicRmw => "%x = atomicrmw add ptr %p, i32 %a seq_cst",
        PtrAdd => "%x = getelementptr i8, ptr %p, i64 %l",
        ExtractValue => "%x = extractvalue { i32, i64 } %s, 0",
        InsertValue => "%x = insertvalue { i32, i64 } %s, i32 %a, 0",
        ExtractElement => "%x = extractelement <4 x i32> %v, i32 %a",
        InsertElement => "%x = insertelement <4 x i32> %v, i32 %a, i32 %a",
        ShuffleVector => "%x = shufflevector <4 x i32> %v, <4 x i32> %v, <4 x i32> zeroinitializer",
        Phi => "%x = phi i32 [ 0, %entry ]",
        Select => "%x = select i1 %c, i32 %a, i32 %a",
        Freeze => "%x = freeze i32 %a",
        VaArg => "%x = va_arg ptr %p, i32",
        Call => "call void @g()",
        LandingPad => "%x = landingpad { ptr, i32 } cleanup",
        CatchPad => "%cp = catchpad within %cs [ptr null, i32 64, ptr null]",
        CleanupPad => "%cl = cleanuppad within none []",
        Br => "br label %b1",
        CondBr => "br i1 %c, label %b1, label %b1",
        Switch => "switch i32 %a, label %b1 [ i32 1, label %b1 ]",
        IndirectBr => "indirectbr ptr %p, [label %b1]",
        Ret => "ret void",
        Unreachable => "unreachable",
        CallBr => "callbr void asm \"\", \"\"() to label %b1 []",
        Invoke => "invoke void @g() to label %b1 unwind label %pad",
        Resume => "resume { ptr, i32 } undef",
        CatchSwitch => "%cs = catchswitch within none [label %handler] unwind to caller",
        CatchRet => "catchret from %cp to label %done",
        CleanupRet => "cleanupret from %cl unwind to caller",
    }
}

/// A module in which `op`'s sample is the last instruction of its block,
/// or, with `followed`, has a terminator after it.
fn placed(op: Opcode, followed: bool) -> String {
    let line = |op: Opcode| format!("  {}\n", sample(op));
    let after = |next: Opcode| if followed { line(next) } else { String::new() };
    let funclets = |blocks: String| {
        format!(
            "{}declare void @g()\ndeclare i32 @__CxxFrameHandler3(...)\n\
             define void @f() personality ptr @__CxxFrameHandler3 {{\n{blocks}\
             done:\n  ret void\n}}\n",
            header()
        )
    };
    let catch = |handler: String| {
        funclets(format!(
            "entry:\n  invoke void @g() to label %done unwind label %dispatch\n\
             dispatch:\n{}handler:\n{handler}",
            line(Opcode::CatchSwitch)
        ))
    };
    let cleanup = |pad: String| {
        funclets(format!(
            "entry:\n  invoke void @g() to label %done unwind label %cleanup\ncleanup:\n{pad}"
        ))
    };
    match op {
        Opcode::CatchSwitch | Opcode::CatchRet => {
            catch(line(Opcode::CatchPad) + &line(Opcode::CatchRet))
        }
        Opcode::CatchPad => catch(line(op) + &after(Opcode::CatchRet)),
        Opcode::CleanupRet => cleanup(line(Opcode::CleanupPad) + &line(op)),
        Opcode::CleanupPad => cleanup(line(op) + &after(Opcode::CleanupRet)),
        _ => {
            // A landing pad is reached by unwinding; everything else by a
            // branch, which gives the phi its predecessor.
            let enter = if op == Opcode::LandingPad {
                "invoke void @g() to label %b1 unwind label %here"
            } else {
                "br label %here"
            };
            format!(
                "{}declare void @g()\ndeclare i32 @__gxx_personality_v0(...)\n\
                 define void @f(i32 %a, i64 %l, float %f, double %d, ptr %p, i1 %c, \
                 <4 x i32> %v, {{ i32, i64 }} %s) personality ptr @__gxx_personality_v0 {{\n\
                 entry:\n  {enter}\nhere:\n{}{}b1:\n  ret void\n\
                 pad:\n  %lp = landingpad {{ ptr, i32 }} cleanup\n  ret void\n}}\n",
                header(),
                line(op),
                after(Opcode::Br)
            )
        }
    }
}

/// §5.4 "terminator": the opcodes hod calls terminators are the ones
/// LLVM ends a block with. For each opcode, LLVM accepts its instruction
/// as the last of a block exactly when `is_terminator` says so, and a
/// non-terminator is accepted once a terminator follows it.
#[test]
fn terminators_are_llvms() {
    let Some(llvm_as) = llvm_tool(LLVM, "llvm-as") else { return };
    let mut terminators = 0;
    for &op in Opcode::ALL {
        let keyword = match op {
            Opcode::PtrAdd => "getelementptr",
            Opcode::CondBr => "br",
            other => other.name(),
        };
        let text = sample(op);
        assert!(text.split([' ', '=']).any(|word| word == keyword), "{op:?}: {text}");
        let last = assembles(&llvm_as, &placed(op, false));
        assert_eq!(last.is_ok(), op.is_terminator(), "{op:?} as the last of a block: {last:?}");
        if op.is_terminator() {
            terminators += 1;
        } else if let Err(error) = assembles(&llvm_as, &placed(op, true)) {
            panic!("{op:?} before a terminator: {error}");
        }
    }
    eprintln!("terminators: {} opcodes, {terminators} of them terminators", Opcode::ALL.len());
}

// ---- casts ----

/// §5.4 "cast": `verify` accepts a cast exactly when LLVM does, over
/// every cast opcode and a grid of type pairs: integers, floats, pointers
/// in three address spaces (one of them 32 bits wide), fixed and scalable
/// vectors, and aggregates. This includes `ptrtoaddr`'s rule that the
/// result is exactly as wide as an address.
#[test]
fn cast_rules_are_llvms() {
    let Some(llvm_as) = llvm_tool(LLVM, "llvm-as") else { return };
    let mut m = module();
    let grid = [
        "i32",
        "i64",
        "float",
        "ptr",
        "ptr addrspace(1)",
        "ptr addrspace(270)",
        "<2 x i32>",
        "<2 x ptr>",
    ];
    let mut pairs: Vec<(&str, &str)> =
        grid.iter().flat_map(|&from| grid.iter().map(move |&to| (from, to))).collect();
    pairs.extend([
        ("i8", "i1"),
        ("i1", "i8"),
        ("half", "float"),
        ("float", "half"),
        ("half", "bfloat"),
        ("double", "float"),
        ("float", "double"),
        ("double", "i64"),
        ("i64", "double"),
        ("x86_fp80", "fp128"),
        ("fp128", "x86_fp80"),
        ("x86_fp80", "i80"),
        ("<1 x ptr>", "ptr"),
        ("ptr", "<1 x ptr>"),
        ("<2 x ptr>", "<1 x ptr>"),
        ("<2 x ptr>", "<2 x i64>"),
        ("<2 x i64>", "<2 x ptr>"),
        ("<2 x ptr>", "<3 x i64>"),
        ("<2 x ptr>", "<2 x ptr addrspace(1)>"),
        ("<2 x ptr addrspace(270)>", "<2 x i32>"),
        ("<4 x i16>", "<2 x i32>"),
        ("<2 x i32>", "<2 x i64>"),
        ("<2 x i64>", "<2 x i32>"),
        ("<2 x float>", "<2 x i32>"),
        ("<2 x i32>", "<2 x float>"),
        ("<2 x float>", "<2 x double>"),
        ("<2 x double>", "<2 x float>"),
        ("<2 x i32>", "i64"),
        ("i64", "<2 x i32>"),
        ("<vscale x 2 x i32>", "<vscale x 2 x i64>"),
        ("<vscale x 2 x i32>", "<vscale x 4 x i16>"),
        ("<vscale x 2 x i32>", "<2 x i32>"),
        ("<vscale x 2 x ptr>", "<vscale x 2 x i64>"),
        ("{ i32 }", "i32"),
        ("i32", "{ i32 }"),
        ("[1 x i32]", "i32"),
    ]);
    let ops: Vec<Opcode> = Opcode::ALL.iter().copied().filter(|op| op.is_cast()).collect();
    assert_eq!(ops.len(), 14);
    let mut cases = Vec::new();
    for (from, to) in pairs {
        let (src, dst) = (parse_type(&mut m.types, from), parse_type(&mut m.types, to));
        for &op in &ops {
            let hod = verifies(&mut m, dst, &[src], |b, args| b.cast(op, dst, args[0]).ok());
            let name = op.name();
            let text = format!(
                "define {to} @F({from} %a) {{\n  %r = {name} {from} %a to {to}\n  ret {to} %r\n}}\n"
            );
            cases.push(Case { label: format!("{name} {from} to {to}"), text, hod });
        }
    }
    agree(&llvm_as, "casts", "", &cases);
}

// ---- atomics ----

#[derive(Clone, Copy, Debug)]
enum Atomic {
    Load,
    Store,
    Rmw(RmwOp),
    /// With the failure ordering.
    CmpXchg(AtomicOrdering),
    Fence,
}

/// §5.4 "memory": the orderings each atomic instruction may have and the
/// types it may access are LLVM's: every ordering on `load`, `store`,
/// `atomicrmw` and `fence`, every pair of orderings on `cmpxchg`, every
/// `atomicrmw` operation on integers, floats, pointers and vectors, and
/// each instruction on types of every class and of awkward sizes.
#[test]
fn atomic_rules_are_llvms() {
    let Some(llvm_as) = llvm_tool(LLVM, "llvm-as") else { return };
    let mut m = module();
    let seq = AtomicOrdering::SeqCst;
    let mut grid: Vec<(Atomic, &str, AtomicOrdering)> = Vec::new();
    for &ordering in AtomicOrdering::ALL {
        for what in [Atomic::Load, Atomic::Store, Atomic::Rmw(RmwOp::Xchg), Atomic::Fence] {
            grid.push((what, "i32", ordering));
        }
        for &failure in AtomicOrdering::ALL {
            grid.push((Atomic::CmpXchg(failure), "i32", ordering));
        }
    }
    let types = [
        "i1",
        "i7",
        "i8",
        "i16",
        "i24",
        "i64",
        "i128",
        "half",
        "float",
        "double",
        "fp128",
        "x86_fp80",
        "ptr",
        "ptr addrspace(270)",
        "<2 x i32>",
        "<2 x float>",
        "<2 x ptr>",
        "<3 x i8>",
        "<4 x i1>",
        "<vscale x 2 x i32>",
        "<vscale x 2 x float>",
        "<vscale x 2 x ptr>",
        "<vscale x 3 x i8>",
        "<vscale x 4 x i1>",
        "<vscale x 8 x i1>",
        "{ i32 }",
        "[2 x i32]",
    ];
    for ty in types {
        let kinds = [
            Atomic::Load,
            Atomic::Store,
            Atomic::Rmw(RmwOp::Xchg),
            Atomic::Rmw(RmwOp::Add),
            Atomic::Rmw(RmwOp::FAdd),
            Atomic::CmpXchg(seq),
        ];
        grid.extend(kinds.map(|what| (what, ty, seq)));
    }
    for &op in RmwOp::ALL {
        for ty in ["i32", "float", "ptr", "<2 x float>", "<2 x i32>"] {
            grid.push((Atomic::Rmw(op), ty, seq));
        }
    }

    let mut cases = Vec::new();
    for (what, ty_text, ordering) in grid {
        let ty = parse_type(&mut m.types, ty_text);
        let pair = m.types.struct_type(&[ty, TypeId::I1], false).unwrap();
        let mem = MemFlags::aligned(16).unwrap().with_atomic(ordering, SyncScope::System);
        let hod = verifies(&mut m, TypeId::VOID, &[TypeId::PTR, ty], |b, args| {
            let (p, v) = (args[0], args[1]);
            match what {
                Atomic::Load => b.load(ty, p, mem),
                Atomic::Store => b.store(v, p, mem),
                Atomic::Rmw(op) => b.atomic_rmw(op, ty, p, v, mem),
                Atomic::CmpXchg(failure) => b.cmpxchg(pair, p, v, v, mem, failure, false),
                Atomic::Fence => b.fence(ordering, SyncScope::System),
            }
            .unwrap();
            None
        });
        let order = ordering.name();
        let line = match what {
            Atomic::Load => format!("%r = load atomic {ty_text}, ptr %p {order}, align 16"),
            Atomic::Store => format!("store atomic {ty_text} %v, ptr %p {order}, align 16"),
            Atomic::Rmw(op) => format!("%r = atomicrmw {} ptr %p, {ty_text} %v {order}", op.name()),
            Atomic::CmpXchg(failure) => {
                let failure = failure.name();
                format!("%r = cmpxchg ptr %p, {ty_text} %v, {ty_text} %v {order} {failure}")
            }
            Atomic::Fence => format!("fence {order}"),
        };
        let text = format!("define void @F(ptr %p, {ty_text} %v) {{\n  {line}\n  ret void\n}}\n");
        cases.push(Case { label: line, text, hod });
    }
    agree(&llvm_as, "atomics", "", &cases);
}

// ---- aggregate index paths ----

/// The byte offsets LLVM gives the `gep` expressions, which `llc` writes
/// as the elements of a table.
fn llvm_offsets(geps: &[String]) -> Option<Vec<u64>> {
    let tools = common::tools(LLVM)?;
    let elements: Vec<String> =
        geps.iter().map(|gep| format!("  i64 ptrtoint (ptr {gep} to i64)")).collect();
    let ir =
        format!("{}@q = global [{} x i64] [\n{}\n]\n", header(), geps.len(), elements.join(",\n"));
    let scratch = Scratch::new("body-offsets");
    let object = tools
        .compile(&scratch, "offsets", &ir, false)
        .unwrap_or_else(|error| panic!("llc rejected the offsets module:\n{error}"));
    let data = tools.section_bytes(&object, ".data");
    let table = tools.read_object(&object);
    let table = table.symbol("q");
    assert_eq!(table.size, geps.len() as u64 * 8);
    let bytes = &data[table.value as usize..][..geps.len() * 8];
    Some(
        bytes
            .chunks_exact(8)
            .map(|chunk| u64::from_le_bytes(chunk.try_into().expect("eight bytes")))
            .collect(),
    )
}

/// §5.4 "aggregate": `aggregate_field` resolves an index path exactly
/// when LLVM accepts it in `extractvalue` and `insertvalue`, to the field
/// type LLVM gives it and to the byte offset LLVM's layout gives it; and
/// `verify` accepts the instruction built from what it returns.
#[test]
fn aggregate_paths_are_llvms() {
    let Some(llvm_as) = llvm_tool(LLVM, "llvm-as") else { return };
    let mut m = module();
    let aggregates = [
        "{ i8, [3 x { i16, i64 }], <{ i8, i32 }>, <2 x i32>, {}, [0 x i32], ptr }",
        "[2 x [3 x { i8, double }]]",
        "<{ i8, { i8, i64 }, [2 x i16], i128 }>",
        "{ { { i1 } }, float }",
    ];
    let mut rng = Rng(0x004_5eed);
    let mut cases = Vec::new();
    let (mut geps, mut offsets) = (Vec::new(), Vec::new());
    for agg_text in aggregates {
        let agg = parse_type(&mut m.types, agg_text);
        // Every path that names a field (array elements up to the third),
        // the same paths one step too far, and random ones.
        let mut paths: Vec<Vec<u32>> = vec![Vec::new()];
        let mut next = 0;
        while next < paths.len() {
            let path = paths[next].clone();
            next += 1;
            // The empty path names nothing: LLVM wants an index.
            let field = match aggregate_field(&m.types, agg, &path) {
                Ok((_, field)) => field,
                Err(_) if path.is_empty() => agg,
                Err(_) => continue,
            };
            let inner = match m.types.kind(field) {
                TypeKind::Struct { fields, .. } => fields.len() as u32,
                TypeKind::Array { len, .. } => len.min(3) as u32,
                _ => 0,
            };
            // One more than there are, which no aggregate has.
            paths.extend((0..=inner.max(1)).map(|index| [&path[..], &[index]].concat()));
        }
        for _ in 0..40 {
            let path = (0..=rng.below(3)).map(|_| rng.below(8) as u32).collect();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        for path in paths {
            let indices: Vec<String> = path.iter().map(ToString::to_string).collect();
            let indices = indices.join(", ");
            let label = format!("{agg_text} at [{indices}]");
            // An empty list is a syntax error in LLVM.
            let comma = if path.is_empty() { "" } else { ", " };
            if let Ok((offset, field)) = aggregate_field(&m.types, agg, &path) {
                let field_text = type_text(&m.types, field);
                let extracts = verifies(&mut m, field, &[agg], |b, args| {
                    b.extract_value(field, args[0], offset).ok()
                });
                let inserts = verifies(&mut m, agg, &[agg, field], |b, args| {
                    b.insert_value(agg, args[0], args[1], offset).ok()
                });
                assert!(extracts && inserts, "verify refuses {label} at byte {offset}");
                let text = format!(
                    "define {field_text} @F({agg_text} %v, {field_text} %e) {{\n  \
                     %w = insertvalue {agg_text} %v, {field_text} %e, {indices}\n  \
                     %r = extractvalue {agg_text} %w, {indices}\n  ret {field_text} %r\n}}\n"
                );
                cases.push(Case { label: label.clone(), text, hod: true });
                let gep_indices: Vec<String> =
                    path.iter().map(|index| format!(", i32 {index}")).collect();
                geps.push(format!(
                    "getelementptr ({agg_text}, ptr null, i32 0{})",
                    gep_indices.concat()
                ));
                offsets.push((label, offset));
            } else {
                let text = format!(
                    "define void @F({agg_text} %v) {{\n  \
                     %r = extractvalue {agg_text} %v{comma}{indices}\n  ret void\n}}\n"
                );
                cases.push(Case { label, text, hod: false });
            }
        }
    }
    eprintln!("aggregate paths: {} resolve, {} do not", offsets.len(), cases.len() - offsets.len());
    assert!(offsets.len() > 40 && cases.len() - offsets.len() > 40, "the grid is lopsided");
    agree(&llvm_as, "aggregate paths", "", &cases);

    let Some(llvm) = llvm_offsets(&geps) else { return };
    let wrong: Vec<String> = offsets
        .iter()
        .zip(llvm)
        .filter(|((_, hod), llvm)| hod != llvm)
        .map(|((label, hod), llvm)| format!("{label}: hod {hod}, LLVM {llvm}"))
        .collect();
    eprintln!("aggregate offsets: {} compared with LLVM {LLVM}", offsets.len());
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ---- intrinsics ----

/// One declaration of every intrinsic on hod's list (§5.5): the name
/// LLVM 22 gives it for these overload types, and its function type.
const INTRINSICS: &[(&str, &str)] = &[
    ("llvm.memcpy.p0.p0.i64", "void (ptr, ptr, i64, i1)"),
    ("llvm.memcpy.inline.p0.p0.i64", "void (ptr, ptr, i64, i1)"),
    ("llvm.memmove.p0.p0.i64", "void (ptr, ptr, i64, i1)"),
    ("llvm.memset.p0.i64", "void (ptr, i8, i64, i1)"),
    ("llvm.memset.inline.p0.i64", "void (ptr, i8, i64, i1)"),
    ("llvm.va_start.p0", "void (ptr)"),
    ("llvm.va_end.p0", "void (ptr)"),
    ("llvm.va_copy.p0", "void (ptr, ptr)"),
    ("llvm.stacksave.p0", "ptr ()"),
    ("llvm.stackrestore.p0", "void (ptr)"),
    ("llvm.frameaddress.p0", "ptr (i32)"),
    ("llvm.returnaddress", "ptr (i32)"),
    ("llvm.trap", "void ()"),
    ("llvm.debugtrap", "void ()"),
    ("llvm.ubsantrap", "void (i8)"),
    ("llvm.bswap.i32", "i32 (i32)"),
    ("llvm.bitreverse.i32", "i32 (i32)"),
    ("llvm.ctpop.i32", "i32 (i32)"),
    ("llvm.ctlz.i32", "i32 (i32, i1)"),
    ("llvm.cttz.i32", "i32 (i32, i1)"),
    ("llvm.fshl.i32", "i32 (i32, i32, i32)"),
    ("llvm.fshr.i32", "i32 (i32, i32, i32)"),
    ("llvm.abs.i32", "i32 (i32, i1)"),
    ("llvm.smax.i32", "i32 (i32, i32)"),
    ("llvm.smin.i32", "i32 (i32, i32)"),
    ("llvm.umax.i32", "i32 (i32, i32)"),
    ("llvm.umin.i32", "i32 (i32, i32)"),
    ("llvm.scmp.i8.i32", "i8 (i32, i32)"),
    ("llvm.ucmp.i8.i32", "i8 (i32, i32)"),
    ("llvm.clmul.i32", "i32 (i32, i32)"),
    ("llvm.sadd.with.overflow.i32", "{ i32, i1 } (i32, i32)"),
    ("llvm.uadd.with.overflow.i32", "{ i32, i1 } (i32, i32)"),
    ("llvm.ssub.with.overflow.i32", "{ i32, i1 } (i32, i32)"),
    ("llvm.usub.with.overflow.i32", "{ i32, i1 } (i32, i32)"),
    ("llvm.smul.with.overflow.i32", "{ i32, i1 } (i32, i32)"),
    ("llvm.umul.with.overflow.i32", "{ i32, i1 } (i32, i32)"),
    ("llvm.sadd.sat.i32", "i32 (i32, i32)"),
    ("llvm.uadd.sat.i32", "i32 (i32, i32)"),
    ("llvm.ssub.sat.i32", "i32 (i32, i32)"),
    ("llvm.usub.sat.i32", "i32 (i32, i32)"),
    ("llvm.fabs.f64", "double (double)"),
    ("llvm.sqrt.f64", "double (double)"),
    ("llvm.floor.f64", "double (double)"),
    ("llvm.ceil.f64", "double (double)"),
    ("llvm.trunc.f64", "double (double)"),
    ("llvm.round.f64", "double (double)"),
    ("llvm.roundeven.f64", "double (double)"),
    ("llvm.nearbyint.f64", "double (double)"),
    ("llvm.rint.f64", "double (double)"),
    ("llvm.copysign.f64", "double (double, double)"),
    ("llvm.fma.f64", "double (double, double, double)"),
    ("llvm.fmuladd.f64", "double (double, double, double)"),
    ("llvm.minnum.f64", "double (double, double)"),
    ("llvm.maxnum.f64", "double (double, double)"),
    ("llvm.minimum.f64", "double (double, double)"),
    ("llvm.maximum.f64", "double (double, double)"),
    ("llvm.minimumnum.f64", "double (double, double)"),
    ("llvm.maximumnum.f64", "double (double, double)"),
    ("llvm.canonicalize.f64", "double (double)"),
    ("llvm.arithmetic.fence.f64", "double (double)"),
    ("llvm.pow.f64", "double (double, double)"),
    ("llvm.powi.f64.i32", "double (double, i32)"),
    ("llvm.ldexp.f64.i32", "double (double, i32)"),
    ("llvm.frexp.f64.i32", "{ double, i32 } (double)"),
    ("llvm.sin.f64", "double (double)"),
    ("llvm.cos.f64", "double (double)"),
    ("llvm.tan.f64", "double (double)"),
    ("llvm.asin.f64", "double (double)"),
    ("llvm.acos.f64", "double (double)"),
    ("llvm.atan.f64", "double (double)"),
    ("llvm.atan2.f64", "double (double, double)"),
    ("llvm.sinh.f64", "double (double)"),
    ("llvm.cosh.f64", "double (double)"),
    ("llvm.tanh.f64", "double (double)"),
    ("llvm.exp.f64", "double (double)"),
    ("llvm.exp2.f64", "double (double)"),
    ("llvm.exp10.f64", "double (double)"),
    ("llvm.log.f64", "double (double)"),
    ("llvm.log2.f64", "double (double)"),
    ("llvm.log10.f64", "double (double)"),
    ("llvm.lround.i64.f64", "i64 (double)"),
    ("llvm.llround.i64.f64", "i64 (double)"),
    ("llvm.lrint.i64.f64", "i64 (double)"),
    ("llvm.llrint.i64.f64", "i64 (double)"),
    ("llvm.fptosi.sat.i32.f64", "i32 (double)"),
    ("llvm.fptoui.sat.i32.f64", "i32 (double)"),
    ("llvm.is.fpclass.f64", "i1 (double, i32)"),
    ("llvm.eh.typeid.for.p0", "i32 (ptr)"),
    ("llvm.eh.sjlj.setjmp", "i32 (ptr)"),
    ("llvm.eh.sjlj.longjmp", "void (ptr)"),
    ("llvm.threadlocal.address.p0", "ptr (ptr)"),
    ("llvm.thread.pointer.p0", "ptr ()"),
    ("llvm.prefetch.p0", "void (ptr, i32, i32, i32)"),
    ("llvm.clear_cache", "void (ptr, ptr)"),
    ("llvm.readcyclecounter", "i64 ()"),
    ("llvm.read_register.i64", "i64 (metadata)"),
    ("llvm.read_volatile_register.i64", "i64 (metadata)"),
    ("llvm.write_register.i64", "void (metadata, i64)"),
    ("llvm.is.constant.i32", "i1 (i32)"),
    ("llvm.objectsize.i64.p0", "i64 (ptr, i1, i1, i1)"),
    ("llvm.ptrmask.p0.i64", "ptr (ptr, i64)"),
    ("llvm.load.relative.i64", "ptr (ptr, i64)"),
    ("llvm.instrprof.increment", "void (ptr, i64, i32, i32)"),
    ("llvm.instrprof.increment.step", "void (ptr, i64, i32, i32, i64)"),
    ("llvm.vector.reduce.add.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.mul.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.and.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.or.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.xor.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.smax.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.smin.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.umax.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.umin.v4i32", "i32 (<4 x i32>)"),
    ("llvm.vector.reduce.fmax.v4f32", "float (<4 x float>)"),
    ("llvm.vector.reduce.fmin.v4f32", "float (<4 x float>)"),
    ("llvm.vector.reduce.fadd.v4f32", "float (float, <4 x float>)"),
    ("llvm.vector.reduce.fmul.v4f32", "float (float, <4 x float>)"),
    ("llvm.vector.extract.v2i32.v4i32", "<2 x i32> (<4 x i32>, i64)"),
    ("llvm.vector.insert.v4i32.v2i32", "<4 x i32> (<4 x i32>, <2 x i32>, i64)"),
    ("llvm.coro.id", "token (i32, ptr, ptr, ptr)"),
    ("llvm.coro.alloc", "i1 (token)"),
    ("llvm.coro.begin", "ptr (token, ptr)"),
    ("llvm.coro.end", "void (ptr, i1, token)"),
    ("llvm.coro.free", "ptr (token, ptr)"),
    ("llvm.coro.size.i64", "i64 ()"),
    ("llvm.coro.align.i64", "i64 ()"),
    ("llvm.coro.save", "token (ptr)"),
    ("llvm.coro.suspend", "i8 (token, i1)"),
    ("llvm.coro.resume", "void (ptr)"),
    ("llvm.coro.destroy", "void (ptr)"),
    ("llvm.coro.done", "i1 (ptr)"),
    ("llvm.coro.promise", "ptr (ptr, i32, i1)"),
    ("llvm.coro.frame", "ptr ()"),
    ("llvm.coro.noop", "ptr ()"),
    ("llvm.coro.is_in_ramp", "i1 ()"),
    ("llvm.coro.await.suspend.void", "void (ptr, ptr, ptr)"),
    ("llvm.coro.await.suspend.bool", "i1 (ptr, ptr, ptr)"),
    ("llvm.coro.await.suspend.handle", "ptr (ptr, ptr, ptr)"),
    ("llvm.sshl.sat.i32", "i32 (i32, i32)"),
    ("llvm.ushl.sat.i32", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32b", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32h", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32w", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32x", "i32 (i32, i64)"),
    ("llvm.aarch64.crc32cb", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32ch", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32cw", "i32 (i32, i32)"),
    ("llvm.aarch64.crc32cx", "i32 (i32, i64)"),
    ("llvm.x86.sse2.pause", "void ()"),
    ("llvm.x86.sse.cmp.ss", "<4 x float> (<4 x float>, <4 x float>, i8)"),
    ("llvm.x86.sse.comieq.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.comige.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.comigt.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.comile.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.comilt.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.comineq.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.ucomieq.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.ucomige.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.ucomigt.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.ucomile.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.ucomilt.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.ucomineq.ss", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.cvtss2si", "i32 (<4 x float>)"),
    ("llvm.x86.sse.cvtss2si64", "i64 (<4 x float>)"),
    ("llvm.x86.sse.cvttss2si", "i32 (<4 x float>)"),
    ("llvm.x86.sse.cvttss2si64", "i64 (<4 x float>)"),
    ("llvm.x86.sse.max.ps", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.max.ss", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.min.ps", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.min.ss", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse.movmsk.ps", "i32 (<4 x float>)"),
    ("llvm.x86.sse.sfence", "void ()"),
    ("llvm.x86.sse.ldmxcsr", "void (ptr)"),
    ("llvm.x86.sse.rcp.ps", "<4 x float> (<4 x float>)"),
    ("llvm.x86.sse.rcp.ss", "<4 x float> (<4 x float>)"),
    ("llvm.x86.sse.rsqrt.ps", "<4 x float> (<4 x float>)"),
    ("llvm.x86.sse.rsqrt.ss", "<4 x float> (<4 x float>)"),
    ("llvm.x86.sse.stmxcsr", "void (ptr)"),
    ("llvm.x86.sse2.cmp.sd", "<2 x double> (<2 x double>, <2 x double>, i8)"),
    ("llvm.x86.sse2.comieq.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.comige.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.comigt.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.comile.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.comilt.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.comineq.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.ucomieq.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.ucomige.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.ucomigt.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.ucomile.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.ucomilt.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.ucomineq.sd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.cvtpd2dq", "<4 x i32> (<2 x double>)"),
    ("llvm.x86.sse2.cvtpd2ps", "<4 x float> (<2 x double>)"),
    ("llvm.x86.sse2.cvtps2dq", "<4 x i32> (<4 x float>)"),
    ("llvm.x86.sse2.cvtsd2si", "i32 (<2 x double>)"),
    ("llvm.x86.sse2.cvtsd2si64", "i64 (<2 x double>)"),
    ("llvm.x86.sse2.cvtsd2ss", "<4 x float> (<4 x float>, <2 x double>)"),
    ("llvm.x86.sse2.cvttpd2dq", "<4 x i32> (<2 x double>)"),
    ("llvm.x86.sse2.cvttps2dq", "<4 x i32> (<4 x float>)"),
    ("llvm.x86.sse2.cvttsd2si", "i32 (<2 x double>)"),
    ("llvm.x86.sse2.cvttsd2si64", "i64 (<2 x double>)"),
    ("llvm.x86.sse2.lfence", "void ()"),
    ("llvm.x86.sse2.mfence", "void ()"),
    ("llvm.x86.sse2.clflush", "void (ptr)"),
    ("llvm.x86.sse2.maskmov.dqu", "void (<16 x i8>, <16 x i8>, ptr)"),
    ("llvm.x86.sse2.max.pd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.max.sd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.min.pd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.min.sd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse2.movmsk.pd", "i32 (<2 x double>)"),
    ("llvm.x86.sse2.packssdw.128", "<8 x i16> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sse2.packsswb.128", "<16 x i8> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.packuswb.128", "<16 x i8> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.pavg.b", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.x86.sse2.pavg.w", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.pmadd.wd", "<4 x i32> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.pmovmskb.128", "i32 (<16 x i8>)"),
    ("llvm.x86.sse2.pmulh.w", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.pmulhu.w", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.psad.bw", "<2 x i64> (<16 x i8>, <16 x i8>)"),
    ("llvm.x86.sse2.psll.d", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sse2.psll.q", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.sse2.psll.w", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.pslli.d", "<4 x i32> (<4 x i32>, i32)"),
    ("llvm.x86.sse2.pslli.q", "<2 x i64> (<2 x i64>, i32)"),
    ("llvm.x86.sse2.pslli.w", "<8 x i16> (<8 x i16>, i32)"),
    ("llvm.x86.sse2.psra.d", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sse2.psra.w", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.psrai.d", "<4 x i32> (<4 x i32>, i32)"),
    ("llvm.x86.sse2.psrai.w", "<8 x i16> (<8 x i16>, i32)"),
    ("llvm.x86.sse2.psrl.d", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sse2.psrl.q", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.sse2.psrl.w", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.sse2.psrli.d", "<4 x i32> (<4 x i32>, i32)"),
    ("llvm.x86.sse2.psrli.q", "<2 x i64> (<2 x i64>, i32)"),
    ("llvm.x86.sse2.psrli.w", "<8 x i16> (<8 x i16>, i32)"),
    ("llvm.x86.sse3.addsub.pd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse3.addsub.ps", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse3.hadd.pd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse3.hadd.ps", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse3.hsub.pd", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.x86.sse3.hsub.ps", "<4 x float> (<4 x float>, <4 x float>)"),
    ("llvm.x86.sse3.ldu.dq", "<16 x i8> (ptr)"),
    ("llvm.x86.ssse3.phadd.d.128", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.ssse3.phadd.sw.128", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.ssse3.phadd.w.128", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.ssse3.phsub.d.128", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.ssse3.phsub.sw.128", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.ssse3.phsub.w.128", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.ssse3.pmadd.ub.sw.128", "<8 x i16> (<16 x i8>, <16 x i8>)"),
    ("llvm.x86.ssse3.pmul.hr.sw.128", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.ssse3.pshuf.b.128", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.x86.ssse3.psign.b.128", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.x86.ssse3.psign.d.128", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.ssse3.psign.w.128", "<8 x i16> (<8 x i16>, <8 x i16>)"),
    ("llvm.x86.flags.read.u64", "i64 ()"),
    ("llvm.x86.flags.write.u64", "void (i64)"),
    ("llvm.x86.fxsave", "void (ptr)"),
    ("llvm.x86.fxsave64", "void (ptr)"),
    ("llvm.x86.fxrstor", "void (ptr)"),
    ("llvm.x86.fxrstor64", "void (ptr)"),
    ("llvm.x86.rdtsc", "i64 ()"),
    ("llvm.x86.rdtscp", "{ i64, i32 } ()"),
    ("llvm.x86.rdpmc", "i64 (i32)"),
    ("llvm.x86.addcarry.32", "{ i8, i32 } (i8, i32, i32)"),
    ("llvm.x86.addcarry.64", "{ i8, i64 } (i8, i64, i64)"),
    ("llvm.x86.subborrow.32", "{ i8, i32 } (i8, i32, i32)"),
    ("llvm.x86.subborrow.64", "{ i8, i64 } (i8, i64, i64)"),
    ("llvm.x86.xgetbv", "i64 (i32)"),
    ("llvm.x86.xsetbv", "void (i32, i32, i32)"),
    ("llvm.x86.rdrand.16", "{ i16, i32 } ()"),
    ("llvm.x86.rdrand.32", "{ i32, i32 } ()"),
    ("llvm.x86.rdrand.64", "{ i64, i32 } ()"),
    ("llvm.x86.rdseed.16", "{ i16, i32 } ()"),
    ("llvm.x86.rdseed.32", "{ i32, i32 } ()"),
    ("llvm.x86.rdseed.64", "{ i64, i32 } ()"),
    ("llvm.x86.xsave", "void (ptr, i32, i32)"),
    ("llvm.x86.xsave64", "void (ptr, i32, i32)"),
    ("llvm.x86.xrstor", "void (ptr, i32, i32)"),
    ("llvm.x86.xrstor64", "void (ptr, i32, i32)"),
    ("llvm.x86.xsaveopt", "void (ptr, i32, i32)"),
    ("llvm.x86.xsaveopt64", "void (ptr, i32, i32)"),
    ("llvm.x86.xsavec", "void (ptr, i32, i32)"),
    ("llvm.x86.xsavec64", "void (ptr, i32, i32)"),
    ("llvm.x86.xsaves", "void (ptr, i32, i32)"),
    ("llvm.x86.xsaves64", "void (ptr, i32, i32)"),
    ("llvm.x86.xrstors", "void (ptr, i32, i32)"),
    ("llvm.x86.xrstors64", "void (ptr, i32, i32)"),
    ("llvm.x86.clflushopt", "void (ptr)"),
    ("llvm.x86.clwb", "void (ptr)"),
    ("llvm.x86.bmi.bextr.32", "i32 (i32, i32)"),
    ("llvm.x86.bmi.bextr.64", "i64 (i64, i64)"),
    ("llvm.x86.bmi.bzhi.32", "i32 (i32, i32)"),
    ("llvm.x86.bmi.bzhi.64", "i64 (i64, i64)"),
    ("llvm.x86.bmi.pdep.32", "i32 (i32, i32)"),
    ("llvm.x86.bmi.pdep.64", "i64 (i64, i64)"),
    ("llvm.x86.bmi.pext.32", "i32 (i32, i32)"),
    ("llvm.x86.bmi.pext.64", "i64 (i64, i64)"),
    ("llvm.x86.sse42.crc32.32.8", "i32 (i32, i8)"),
    ("llvm.x86.sse42.crc32.32.16", "i32 (i32, i16)"),
    ("llvm.x86.sse42.crc32.32.32", "i32 (i32, i32)"),
    ("llvm.x86.sse42.crc32.64.64", "i64 (i64, i64)"),
    ("llvm.x86.rdfsbase.32", "i32 ()"),
    ("llvm.x86.rdfsbase.64", "i64 ()"),
    ("llvm.x86.rdgsbase.32", "i32 ()"),
    ("llvm.x86.rdgsbase.64", "i64 ()"),
    ("llvm.x86.wrfsbase.32", "void (i32)"),
    ("llvm.x86.wrfsbase.64", "void (i64)"),
    ("llvm.x86.wrgsbase.32", "void (i32)"),
    ("llvm.x86.wrgsbase.64", "void (i64)"),
    ("llvm.x86.xbegin", "i32 ()"),
    ("llvm.x86.xend", "void ()"),
    ("llvm.x86.xabort", "void (i8)"),
    ("llvm.x86.xtest", "i32 ()"),
    ("llvm.x86.rdpid", "i32 ()"),
    ("llvm.x86.rdpkru", "i32 ()"),
    ("llvm.x86.wrpkru", "void (i32)"),
    ("llvm.x86.serialize", "void ()"),
    ("llvm.x86.incsspd", "void (i32)"),
    ("llvm.x86.incsspq", "void (i64)"),
    ("llvm.x86.rdsspd", "i32 (i32)"),
    ("llvm.x86.rdsspq", "i64 (i64)"),
    ("llvm.x86.sse41.round.ps", "<4 x float> (<4 x float>, i32)"),
    ("llvm.x86.sse41.round.pd", "<2 x double> (<2 x double>, i32)"),
    ("llvm.x86.sse41.round.ss", "<4 x float> (<4 x float>, <4 x float>, i32)"),
    ("llvm.x86.sse41.round.sd", "<2 x double> (<2 x double>, <2 x double>, i32)"),
    ("llvm.x86.sse41.blendvps", "<4 x float> (<4 x float>, <4 x float>, <4 x float>)"),
    ("llvm.x86.sse41.blendvpd", "<2 x double> (<2 x double>, <2 x double>, <2 x double>)"),
    ("llvm.x86.sse41.pblendvb", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.x86.sse41.dpps", "<4 x float> (<4 x float>, <4 x float>, i8)"),
    ("llvm.x86.sse41.dppd", "<2 x double> (<2 x double>, <2 x double>, i8)"),
    ("llvm.x86.sse41.insertps", "<4 x float> (<4 x float>, <4 x float>, i8)"),
    ("llvm.x86.sse41.mpsadbw", "<8 x i16> (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse41.packusdw", "<8 x i16> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sse41.phminposuw", "<8 x i16> (<8 x i16>)"),
    ("llvm.x86.sse41.ptestz", "i32 (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.sse41.ptestc", "i32 (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.sse41.ptestnzc", "i32 (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.sse42.pcmpistrm128", "<16 x i8> (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpistri128", "i32 (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpistria128", "i32 (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpistric128", "i32 (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpistrio128", "i32 (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpistris128", "i32 (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpistriz128", "i32 (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.sse42.pcmpestrm128", "<16 x i8> (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.sse42.pcmpestri128", "i32 (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.sse42.pcmpestria128", "i32 (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.sse42.pcmpestric128", "i32 (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.sse42.pcmpestrio128", "i32 (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.sse42.pcmpestris128", "i32 (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.sse42.pcmpestriz128", "i32 (<16 x i8>, i32, <16 x i8>, i32, i8)"),
    ("llvm.x86.aesni.aesenc", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.aesni.aesenclast", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.aesni.aesdec", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.aesni.aesdeclast", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.aesni.aesimc", "<2 x i64> (<2 x i64>)"),
    ("llvm.x86.aesni.aeskeygenassist", "<2 x i64> (<2 x i64>, i8)"),
    ("llvm.x86.pclmulqdq", "<2 x i64> (<2 x i64>, <2 x i64>, i8)"),
    ("llvm.x86.sha1rnds4", "<4 x i32> (<4 x i32>, <4 x i32>, i8)"),
    ("llvm.x86.sha1nexte", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sha1msg1", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sha1msg2", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sha256msg1", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sha256msg2", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.sha256rnds2", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.x86.avx.addsub.pd.256", "<4 x double> (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.addsub.ps.256", "<8 x float> (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.max.pd.256", "<4 x double> (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.max.ps.256", "<8 x float> (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.min.pd.256", "<4 x double> (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.min.ps.256", "<8 x float> (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.hadd.pd.256", "<4 x double> (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.hadd.ps.256", "<8 x float> (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.hsub.pd.256", "<4 x double> (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.hsub.ps.256", "<8 x float> (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.blendv.pd.256", "<4 x double> (<4 x double>, <4 x double>, <4 x double>)"),
    ("llvm.x86.avx.blendv.ps.256", "<8 x float> (<8 x float>, <8 x float>, <8 x float>)"),
    ("llvm.x86.avx.dp.ps.256", "<8 x float> (<8 x float>, <8 x float>, i8)"),
    ("llvm.x86.avx.cmp.pd.256", "<4 x double> (<4 x double>, <4 x double>, i8)"),
    ("llvm.x86.avx.cmp.ps.256", "<8 x float> (<8 x float>, <8 x float>, i8)"),
    ("llvm.x86.avx.cvt.pd2.ps.256", "<4 x float> (<4 x double>)"),
    ("llvm.x86.avx.cvt.ps2dq.256", "<8 x i32> (<8 x float>)"),
    ("llvm.x86.avx.cvt.pd2dq.256", "<4 x i32> (<4 x double>)"),
    ("llvm.x86.avx.cvtt.pd2dq.256", "<4 x i32> (<4 x double>)"),
    ("llvm.x86.avx.cvtt.ps2dq.256", "<8 x i32> (<8 x float>)"),
    ("llvm.x86.avx.rcp.ps.256", "<8 x float> (<8 x float>)"),
    ("llvm.x86.avx.rsqrt.ps.256", "<8 x float> (<8 x float>)"),
    ("llvm.x86.avx.round.pd.256", "<4 x double> (<4 x double>, i32)"),
    ("llvm.x86.avx.round.ps.256", "<8 x float> (<8 x float>, i32)"),
    ("llvm.x86.avx.vpermilvar.ps", "<4 x float> (<4 x float>, <4 x i32>)"),
    ("llvm.x86.avx.vpermilvar.pd", "<2 x double> (<2 x double>, <2 x i64>)"),
    ("llvm.x86.avx.vpermilvar.ps.256", "<8 x float> (<8 x float>, <8 x i32>)"),
    ("llvm.x86.avx.vpermilvar.pd.256", "<4 x double> (<4 x double>, <4 x i64>)"),
    ("llvm.x86.avx.vtestz.ps", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.avx.vtestz.pd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.avx.vtestz.ps.256", "i32 (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.vtestz.pd.256", "i32 (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.vtestc.ps", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.avx.vtestc.pd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.avx.vtestc.ps.256", "i32 (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.vtestc.pd.256", "i32 (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.vtestnzc.ps", "i32 (<4 x float>, <4 x float>)"),
    ("llvm.x86.avx.vtestnzc.pd", "i32 (<2 x double>, <2 x double>)"),
    ("llvm.x86.avx.vtestnzc.ps.256", "i32 (<8 x float>, <8 x float>)"),
    ("llvm.x86.avx.vtestnzc.pd.256", "i32 (<4 x double>, <4 x double>)"),
    ("llvm.x86.avx.ptestz.256", "i32 (<4 x i64>, <4 x i64>)"),
    ("llvm.x86.avx.ptestc.256", "i32 (<4 x i64>, <4 x i64>)"),
    ("llvm.x86.avx.ptestnzc.256", "i32 (<4 x i64>, <4 x i64>)"),
    ("llvm.x86.avx.movmsk.pd.256", "i32 (<4 x double>)"),
    ("llvm.x86.avx.movmsk.ps.256", "i32 (<8 x float>)"),
    ("llvm.x86.avx.ldu.dq.256", "<32 x i8> (ptr)"),
    ("llvm.x86.avx.maskload.ps", "<4 x float> (ptr, <4 x i32>)"),
    ("llvm.x86.avx.maskstore.ps", "void (ptr, <4 x i32>, <4 x float>)"),
    ("llvm.x86.avx.maskload.pd", "<2 x double> (ptr, <2 x i64>)"),
    ("llvm.x86.avx.maskstore.pd", "void (ptr, <2 x i64>, <2 x double>)"),
    ("llvm.x86.avx.maskload.ps.256", "<8 x float> (ptr, <8 x i32>)"),
    ("llvm.x86.avx.maskstore.ps.256", "void (ptr, <8 x i32>, <8 x float>)"),
    ("llvm.x86.avx.maskload.pd.256", "<4 x double> (ptr, <4 x i64>)"),
    ("llvm.x86.avx.maskstore.pd.256", "void (ptr, <4 x i64>, <4 x double>)"),
    ("llvm.x86.avx.vzeroall", "void ()"),
    ("llvm.x86.avx.vzeroupper", "void ()"),
    ("llvm.x86.avx2.packssdw", "<16 x i16> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.packsswb", "<32 x i8> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.packusdw", "<16 x i16> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.packuswb", "<32 x i8> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.pavg.b", "<32 x i8> (<32 x i8>, <32 x i8>)"),
    ("llvm.x86.avx2.pavg.w", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.pblendvb", "<32 x i8> (<32 x i8>, <32 x i8>, <32 x i8>)"),
    ("llvm.x86.avx2.phadd.d", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.phadd.w", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.phadd.sw", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.phsub.d", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.phsub.w", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.phsub.sw", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.pmadd.ub.sw", "<16 x i16> (<32 x i8>, <32 x i8>)"),
    ("llvm.x86.avx2.pmadd.wd", "<8 x i32> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.pmovmskb", "i32 (<32 x i8>)"),
    ("llvm.x86.avx2.pmul.hr.sw", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.pmulh.w", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.pmulhu.w", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.psad.bw", "<4 x i64> (<32 x i8>, <32 x i8>)"),
    ("llvm.x86.avx2.pshuf.b", "<32 x i8> (<32 x i8>, <32 x i8>)"),
    ("llvm.x86.avx2.psign.b", "<32 x i8> (<32 x i8>, <32 x i8>)"),
    ("llvm.x86.avx2.psign.w", "<16 x i16> (<16 x i16>, <16 x i16>)"),
    ("llvm.x86.avx2.psign.d", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.psll.w", "<16 x i16> (<16 x i16>, <8 x i16>)"),
    ("llvm.x86.avx2.psll.d", "<8 x i32> (<8 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.psll.q", "<4 x i64> (<4 x i64>, <2 x i64>)"),
    ("llvm.x86.avx2.pslli.w", "<16 x i16> (<16 x i16>, i32)"),
    ("llvm.x86.avx2.pslli.d", "<8 x i32> (<8 x i32>, i32)"),
    ("llvm.x86.avx2.pslli.q", "<4 x i64> (<4 x i64>, i32)"),
    ("llvm.x86.avx2.psrl.w", "<16 x i16> (<16 x i16>, <8 x i16>)"),
    ("llvm.x86.avx2.psrl.d", "<8 x i32> (<8 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.psrl.q", "<4 x i64> (<4 x i64>, <2 x i64>)"),
    ("llvm.x86.avx2.psrli.w", "<16 x i16> (<16 x i16>, i32)"),
    ("llvm.x86.avx2.psrli.d", "<8 x i32> (<8 x i32>, i32)"),
    ("llvm.x86.avx2.psrli.q", "<4 x i64> (<4 x i64>, i32)"),
    ("llvm.x86.avx2.psra.w", "<16 x i16> (<16 x i16>, <8 x i16>)"),
    ("llvm.x86.avx2.psra.d", "<8 x i32> (<8 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.psrai.w", "<16 x i16> (<16 x i16>, i32)"),
    ("llvm.x86.avx2.psrai.d", "<8 x i32> (<8 x i32>, i32)"),
    ("llvm.x86.avx2.psllv.d", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.psllv.d.256", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.psllv.q", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.avx2.psllv.q.256", "<4 x i64> (<4 x i64>, <4 x i64>)"),
    ("llvm.x86.avx2.psrlv.d", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.psrlv.d.256", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.psrlv.q", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.x86.avx2.psrlv.q.256", "<4 x i64> (<4 x i64>, <4 x i64>)"),
    ("llvm.x86.avx2.psrav.d", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.psrav.d.256", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.permd", "<8 x i32> (<8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.permps", "<8 x float> (<8 x float>, <8 x i32>)"),
    ("llvm.x86.avx2.mpsadbw", "<16 x i16> (<32 x i8>, <32 x i8>, i8)"),
    ("llvm.x86.avx2.maskload.d", "<4 x i32> (ptr, <4 x i32>)"),
    ("llvm.x86.avx2.maskstore.d", "void (ptr, <4 x i32>, <4 x i32>)"),
    ("llvm.x86.avx2.maskload.q", "<2 x i64> (ptr, <2 x i64>)"),
    ("llvm.x86.avx2.maskstore.q", "void (ptr, <2 x i64>, <2 x i64>)"),
    ("llvm.x86.avx2.maskload.d.256", "<8 x i32> (ptr, <8 x i32>)"),
    ("llvm.x86.avx2.maskstore.d.256", "void (ptr, <8 x i32>, <8 x i32>)"),
    ("llvm.x86.avx2.maskload.q.256", "<4 x i64> (ptr, <4 x i64>)"),
    ("llvm.x86.avx2.maskstore.q.256", "void (ptr, <4 x i64>, <4 x i64>)"),
    ("llvm.x86.fma.vfmaddsub.ps", "<4 x float> (<4 x float>, <4 x float>, <4 x float>)"),
    ("llvm.x86.fma.vfmaddsub.pd", "<2 x double> (<2 x double>, <2 x double>, <2 x double>)"),
    ("llvm.x86.fma.vfmaddsub.ps.256", "<8 x float> (<8 x float>, <8 x float>, <8 x float>)"),
    ("llvm.x86.fma.vfmaddsub.pd.256", "<4 x double> (<4 x double>, <4 x double>, <4 x double>)"),
    ("llvm.x86.vcvtps2ph.128", "<8 x i16> (<4 x float>, i32)"),
    ("llvm.x86.vcvtps2ph.256", "<8 x i16> (<8 x float>, i32)"),
    ("llvm.x86.vgf2p8affineqb.128", "<16 x i8> (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.vgf2p8affineqb.256", "<32 x i8> (<32 x i8>, <32 x i8>, i8)"),
    ("llvm.x86.vgf2p8affineinvqb.128", "<16 x i8> (<16 x i8>, <16 x i8>, i8)"),
    ("llvm.x86.vgf2p8affineinvqb.256", "<32 x i8> (<32 x i8>, <32 x i8>, i8)"),
    ("llvm.x86.vgf2p8mulb.128", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.x86.vgf2p8mulb.256", "<32 x i8> (<32 x i8>, <32 x i8>)"),
    ("llvm.aarch64.cls", "i32 (i32)"),
    ("llvm.aarch64.cls64", "i32 (i64)"),
    ("llvm.aarch64.crypto.aesd", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.crypto.aese", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.crypto.aesimc", "<16 x i8> (<16 x i8>)"),
    ("llvm.aarch64.crypto.aesmc", "<16 x i8> (<16 x i8>)"),
    ("llvm.aarch64.crypto.bcaxs.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.crypto.bcaxu.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.crypto.eor3s.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.crypto.eor3u.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.crypto.rax1", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.crypto.sha1c", "<4 x i32> (<4 x i32>, i32, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha1h", "i32 (i32)"),
    ("llvm.aarch64.crypto.sha1m", "<4 x i32> (<4 x i32>, i32, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha1p", "<4 x i32> (<4 x i32>, i32, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha1su0", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha1su1", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha256h", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha256h2", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha256su0", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha256su1", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sha512h", "<2 x i64> (<2 x i64>, <2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.crypto.sha512h2", "<2 x i64> (<2 x i64>, <2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.crypto.sha512su0", "<2 x i64> (<2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.crypto.sha512su1", "<2 x i64> (<2 x i64>, <2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.crypto.xar", "<2 x i64> (<2 x i64>, <2 x i64>, i64)"),
    ("llvm.aarch64.dmb", "void (i32)"),
    ("llvm.aarch64.dsb", "void (i32)"),
    ("llvm.aarch64.hint", "void (i32)"),
    ("llvm.aarch64.isb", "void (i32)"),
    ("llvm.aarch64.neon.abs.v1i64", "<1 x i64> (<1 x i64>)"),
    ("llvm.aarch64.neon.addp.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.bfcvt", "bfloat (float)"),
    ("llvm.aarch64.neon.bfcvtn", "<8 x bfloat> (<4 x float>)"),
    ("llvm.aarch64.neon.bfcvtn2", "<8 x bfloat> (<8 x bfloat>, <4 x float>)"),
    (
        "llvm.aarch64.neon.bfdot.v2f32.v4bf16",
        "<2 x float> (<2 x float>, <4 x bfloat>, <4 x bfloat>)",
    ),
    ("llvm.aarch64.neon.bfmlalb", "<4 x float> (<4 x float>, <8 x bfloat>, <8 x bfloat>)"),
    ("llvm.aarch64.neon.bfmlalt", "<4 x float> (<4 x float>, <8 x bfloat>, <8 x bfloat>)"),
    ("llvm.aarch64.neon.bfmmla", "<4 x float> (<4 x float>, <8 x bfloat>, <8 x bfloat>)"),
    ("llvm.aarch64.neon.cls.v16i8", "<16 x i8> (<16 x i8>)"),
    ("llvm.aarch64.neon.fabd.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.facge.v1i64.v1f64", "<1 x i64> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.facgt.v1i64.v1f64", "<1 x i64> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.faddp.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.aarch64.neon.faddv.f64.v2f64", "double (<2 x double>)"),
    ("llvm.aarch64.neon.fcvtas.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtau.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtms.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtmu.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtns.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtnu.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtps.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtpu.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtxn.v2f32.v2f64", "<2 x float> (<2 x double>)"),
    ("llvm.aarch64.neon.fcvtzs.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fcvtzu.v1i64.v1f64", "<1 x i64> (<1 x double>)"),
    ("llvm.aarch64.neon.fmax.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.fmaxnm.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.fmaxnmp.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.aarch64.neon.fmaxnmv.f64.v2f64", "double (<2 x double>)"),
    ("llvm.aarch64.neon.fmaxp.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.aarch64.neon.fmaxv.f64.v2f64", "double (<2 x double>)"),
    ("llvm.aarch64.neon.fmin.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.fminnm.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.fminnmp.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.aarch64.neon.fminnmv.f64.v2f64", "double (<2 x double>)"),
    ("llvm.aarch64.neon.fminp.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.aarch64.neon.fminv.f64.v2f64", "double (<2 x double>)"),
    ("llvm.aarch64.neon.fmlal.v2f32.v4f16", "<2 x float> (<2 x float>, <4 x half>, <4 x half>)"),
    ("llvm.aarch64.neon.fmlal2.v2f32.v4f16", "<2 x float> (<2 x float>, <4 x half>, <4 x half>)"),
    ("llvm.aarch64.neon.fmlsl.v2f32.v4f16", "<2 x float> (<2 x float>, <4 x half>, <4 x half>)"),
    ("llvm.aarch64.neon.fmlsl2.v2f32.v4f16", "<2 x float> (<2 x float>, <4 x half>, <4 x half>)"),
    ("llvm.aarch64.neon.fmulx.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.frecpe.v1f64", "<1 x double> (<1 x double>)"),
    ("llvm.aarch64.neon.frecps.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.frecpx.f64", "double (double)"),
    ("llvm.aarch64.neon.frint32x.v1f64", "<1 x double> (<1 x double>)"),
    ("llvm.aarch64.neon.frint32z.v1f64", "<1 x double> (<1 x double>)"),
    ("llvm.aarch64.neon.frint64x.v1f64", "<1 x double> (<1 x double>)"),
    ("llvm.aarch64.neon.frint64z.v1f64", "<1 x double> (<1 x double>)"),
    ("llvm.aarch64.neon.frsqrte.v1f64", "<1 x double> (<1 x double>)"),
    ("llvm.aarch64.neon.frsqrts.v1f64", "<1 x double> (<1 x double>, <1 x double>)"),
    ("llvm.aarch64.neon.ld1x2.v1f64.p0", "{ <1 x double>, <1 x double> } (ptr)"),
    ("llvm.aarch64.neon.ld1x3.v1f64.p0", "{ <1 x double>, <1 x double>, <1 x double> } (ptr)"),
    (
        "llvm.aarch64.neon.ld1x4.v1f64.p0",
        "{ <1 x double>, <1 x double>, <1 x double>, <1 x double> } (ptr)",
    ),
    ("llvm.aarch64.neon.ld2.v1f64.p0", "{ <1 x double>, <1 x double> } (ptr)"),
    (
        "llvm.aarch64.neon.ld2lane.v1f64.p0",
        "{ <1 x double>, <1 x double> } (<1 x double>, <1 x double>, i64, ptr)",
    ),
    ("llvm.aarch64.neon.ld2r.v1f64.p0", "{ <1 x double>, <1 x double> } (ptr)"),
    ("llvm.aarch64.neon.ld3.v1f64.p0", "{ <1 x double>, <1 x double>, <1 x double> } (ptr)"),
    (
        "llvm.aarch64.neon.ld3lane.v1f64.p0",
        "{ <1 x double>, <1 x double>, <1 x double> } (<1 x double>, <1 x double>, <1 x double>, i64, ptr)",
    ),
    ("llvm.aarch64.neon.ld3r.v1f64.p0", "{ <1 x double>, <1 x double>, <1 x double> } (ptr)"),
    (
        "llvm.aarch64.neon.ld4.v1f64.p0",
        "{ <1 x double>, <1 x double>, <1 x double>, <1 x double> } (ptr)",
    ),
    (
        "llvm.aarch64.neon.ld4lane.v1f64.p0",
        "{ <1 x double>, <1 x double>, <1 x double>, <1 x double> } (<1 x double>, <1 x double>, <1 x double>, <1 x double>, i64, ptr)",
    ),
    (
        "llvm.aarch64.neon.ld4r.v1f64.p0",
        "{ <1 x double>, <1 x double>, <1 x double>, <1 x double> } (ptr)",
    ),
    ("llvm.aarch64.neon.pmul.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.pmull.v8i16", "<8 x i16> (<8 x i8>, <8 x i8>)"),
    ("llvm.aarch64.neon.pmull64", "<16 x i8> (i64, i64)"),
    ("llvm.aarch64.neon.raddhn.v2i32", "<2 x i32> (<2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.neon.rshrn.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.rsubhn.v2i32", "<2 x i32> (<2 x i64>, <2 x i64>)"),
    ("llvm.aarch64.neon.sabd.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.saddlp.v1i64.v2i32", "<1 x i64> (<2 x i32>)"),
    ("llvm.aarch64.neon.saddlv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.saddv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.scalar.sqxtn.i32.i64", "i32 (i64)"),
    ("llvm.aarch64.neon.scalar.sqxtun.i32.i64", "i32 (i64)"),
    ("llvm.aarch64.neon.scalar.uqxtn.i32.i64", "i32 (i64)"),
    ("llvm.aarch64.neon.sdot.v2i32.v8i8", "<2 x i32> (<2 x i32>, <8 x i8>, <8 x i8>)"),
    ("llvm.aarch64.neon.shadd.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.shsub.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.smax.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.smaxp.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.smaxv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.smin.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.sminp.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.sminv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.smmla.v4i32.v16i8", "<4 x i32> (<4 x i32>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.smull.v2i64", "<2 x i64> (<2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.sqabs.v1i64", "<1 x i64> (<1 x i64>)"),
    ("llvm.aarch64.neon.sqadd.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.sqdmulh.v2i32", "<2 x i32> (<2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.sqdmulh.lane.v2i32.v2i32", "<2 x i32> (<2 x i32>, <2 x i32>, i32)"),
    ("llvm.aarch64.neon.sqdmulh.laneq.v2i32.v4i32", "<2 x i32> (<2 x i32>, <4 x i32>, i32)"),
    ("llvm.aarch64.neon.sqdmull.v2i64", "<2 x i64> (<2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.sqdmulls.scalar", "i64 (i32, i32)"),
    ("llvm.aarch64.neon.sqneg.v1i64", "<1 x i64> (<1 x i64>)"),
    ("llvm.aarch64.neon.sqrdmlah.v2i32", "<2 x i32> (<2 x i32>, <2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.sqrdmlsh.v2i32", "<2 x i32> (<2 x i32>, <2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.sqrdmulh.v2i32", "<2 x i32> (<2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.sqrdmulh.lane.v2i32.v2i32", "<2 x i32> (<2 x i32>, <2 x i32>, i32)"),
    ("llvm.aarch64.neon.sqrdmulh.laneq.v2i32.v4i32", "<2 x i32> (<2 x i32>, <4 x i32>, i32)"),
    ("llvm.aarch64.neon.sqrshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.sqrshrn.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.sqrshrun.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.sqshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.sqshlu.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.sqshrn.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.sqshrun.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.sqsub.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.sqxtn.v2i32", "<2 x i32> (<2 x i64>)"),
    ("llvm.aarch64.neon.sqxtun.v2i32", "<2 x i32> (<2 x i64>)"),
    ("llvm.aarch64.neon.srhadd.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.srshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.sshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.st1x2.v16i8.p0", "void (<16 x i8>, <16 x i8>, ptr)"),
    ("llvm.aarch64.neon.st1x3.v16i8.p0", "void (<16 x i8>, <16 x i8>, <16 x i8>, ptr)"),
    ("llvm.aarch64.neon.st1x4.v16i8.p0", "void (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, ptr)"),
    ("llvm.aarch64.neon.st2.v16i8.p0", "void (<16 x i8>, <16 x i8>, ptr)"),
    ("llvm.aarch64.neon.st2lane.v16i8.p0", "void (<16 x i8>, <16 x i8>, i64, ptr)"),
    ("llvm.aarch64.neon.st3.v16i8.p0", "void (<16 x i8>, <16 x i8>, <16 x i8>, ptr)"),
    ("llvm.aarch64.neon.st3lane.v16i8.p0", "void (<16 x i8>, <16 x i8>, <16 x i8>, i64, ptr)"),
    ("llvm.aarch64.neon.st4.v16i8.p0", "void (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, ptr)"),
    (
        "llvm.aarch64.neon.st4lane.v16i8.p0",
        "void (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, i64, ptr)",
    ),
    ("llvm.aarch64.neon.suqadd.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.tbl1.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.tbl2.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.tbl3.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>)"),
    (
        "llvm.aarch64.neon.tbl4.v16i8",
        "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>)",
    ),
    ("llvm.aarch64.neon.tbx1.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.tbx2.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>)"),
    (
        "llvm.aarch64.neon.tbx3.v16i8",
        "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>)",
    ),
    (
        "llvm.aarch64.neon.tbx4.v16i8",
        "<16 x i8> (<16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>, <16 x i8>)",
    ),
    ("llvm.aarch64.neon.uabd.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.uaddlp.v1i64.v2i32", "<1 x i64> (<2 x i32>)"),
    ("llvm.aarch64.neon.uaddlv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.uaddv.i64.v2i64", "i64 (<2 x i64>)"),
    ("llvm.aarch64.neon.udot.v2i32.v8i8", "<2 x i32> (<2 x i32>, <8 x i8>, <8 x i8>)"),
    ("llvm.aarch64.neon.uhadd.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.uhsub.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.umax.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.umaxp.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.umaxv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.umin.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.uminp.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.uminv.i32.v16i8", "i32 (<16 x i8>)"),
    ("llvm.aarch64.neon.ummla.v4i32.v16i8", "<4 x i32> (<4 x i32>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.umull.v2i64", "<2 x i64> (<2 x i32>, <2 x i32>)"),
    ("llvm.aarch64.neon.uqadd.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.uqrshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.uqrshrn.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.uqshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.uqshrn.v2i32", "<2 x i32> (<2 x i64>, i32)"),
    ("llvm.aarch64.neon.uqsub.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.uqxtn.v2i32", "<2 x i32> (<2 x i64>)"),
    ("llvm.aarch64.neon.urecpe.v2i32", "<2 x i32> (<2 x i32>)"),
    ("llvm.aarch64.neon.urhadd.v16i8", "<16 x i8> (<16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.urshl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.ursqrte.v2i32", "<2 x i32> (<2 x i32>)"),
    ("llvm.aarch64.neon.usdot.v2i32.v8i8", "<2 x i32> (<2 x i32>, <8 x i8>, <8 x i8>)"),
    ("llvm.aarch64.neon.ushl.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.usmmla.v4i32.v16i8", "<4 x i32> (<4 x i32>, <16 x i8>, <16 x i8>)"),
    ("llvm.aarch64.neon.usqadd.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>)"),
    ("llvm.aarch64.neon.vcadd.rot270.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    ("llvm.aarch64.neon.vcadd.rot90.v2f64", "<2 x double> (<2 x double>, <2 x double>)"),
    (
        "llvm.aarch64.neon.vcmla.rot0.v2f64",
        "<2 x double> (<2 x double>, <2 x double>, <2 x double>)",
    ),
    (
        "llvm.aarch64.neon.vcmla.rot180.v2f64",
        "<2 x double> (<2 x double>, <2 x double>, <2 x double>)",
    ),
    (
        "llvm.aarch64.neon.vcmla.rot270.v2f64",
        "<2 x double> (<2 x double>, <2 x double>, <2 x double>)",
    ),
    (
        "llvm.aarch64.neon.vcmla.rot90.v2f64",
        "<2 x double> (<2 x double>, <2 x double>, <2 x double>)",
    ),
    ("llvm.aarch64.neon.vcvtfp2fxs.v1i64.v1f64", "<1 x i64> (<1 x double>, i32)"),
    ("llvm.aarch64.neon.vcvtfp2fxu.v1i64.v1f64", "<1 x i64> (<1 x double>, i32)"),
    ("llvm.aarch64.neon.vcvtfp2hf", "<4 x i16> (<4 x float>)"),
    ("llvm.aarch64.neon.vcvtfxs2fp.v1f64.v1i64", "<1 x double> (<1 x i64>, i32)"),
    ("llvm.aarch64.neon.vcvtfxu2fp.v1f64.v1i64", "<1 x double> (<1 x i64>, i32)"),
    ("llvm.aarch64.neon.vcvthf2fp", "<4 x float> (<4 x i16>)"),
    ("llvm.aarch64.neon.vsli.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>, i32)"),
    ("llvm.aarch64.neon.vsri.v1i64", "<1 x i64> (<1 x i64>, <1 x i64>, i32)"),
    ("llvm.aarch64.sisd.fabd.f64", "double (double, double)"),
    ("llvm.aarch64.sisd.fcvtxn", "float (double)"),
    ("llvm.aarch64.crypto.sm3partw1", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sm3partw2", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sm3ss1", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sm3tt1a", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>, i64)"),
    ("llvm.aarch64.crypto.sm3tt1b", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>, i64)"),
    ("llvm.aarch64.crypto.sm3tt2a", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>, i64)"),
    ("llvm.aarch64.crypto.sm3tt2b", "<4 x i32> (<4 x i32>, <4 x i32>, <4 x i32>, i64)"),
    ("llvm.aarch64.crypto.sm4e", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.crypto.sm4ekey", "<4 x i32> (<4 x i32>, <4 x i32>)"),
    ("llvm.aarch64.frint32x.f32", "float (float)"),
    ("llvm.aarch64.frint32z.f64", "double (double)"),
    ("llvm.aarch64.frint64x.f32", "float (float)"),
    ("llvm.aarch64.frint64z.f64", "double (double)"),
    ("llvm.aarch64.fjcvtzs", "i32 (double)"),
];

/// Names on the list that LLVM 22 upgrades to other IR on reading (so
/// drops), and LLVM 19 keeps: kiln may emit them for LLVM 19 (WP-083).
const UPGRADED_BY_22: &[&str] =
    &["llvm.aarch64.neon.bfcvt", "llvm.aarch64.neon.bfcvtn", "llvm.aarch64.neon.bfcvtn2"];

/// `declare` for a function of type `fn_ty`.
fn declaration(m: &Module, name: &str, fn_ty: TypeId) -> String {
    let TypeKind::Function { ret, params, variadic } = m.types.kind(fn_ty) else {
        panic!("{name} needs a function type")
    };
    let mut params: Vec<String> = params.iter().map(|&ty| type_text(&m.types, ty)).collect();
    if variadic {
        params.push("...".to_string());
    }
    format!("declare {} @{name}({})\n", type_text(&m.types, ret), params.join(", "))
}

/// What LLVM makes of `module`: its text after `llvm-as` and `llvm-dis`.
fn round_trip(llvm_as: &Path, llvm_dis: &Path, module: &str) -> String {
    let run = |tool: &Path, input: &[u8]| {
        let outcome = hod_testutil::run_with(
            Command::new(tool).args(["-o", "-", "-"]),
            input,
            hod_testutil::DEFAULT_TIMEOUT,
        );
        assert!(outcome.success(), "{}:\n{}", tool.display(), outcome.stderr_text());
        outcome.stdout
    };
    String::from_utf8(run(llvm_dis, &run(llvm_as, module.as_bytes()))).expect("text")
}

/// §5.5: the one name mapping is LLVM's. For every intrinsic on the list,
/// LLVM 22 accepts a declaration under the name hod's mangling gives it
/// and keeps that name (a wrong name it would rewrite), and
/// `resolve_intrinsic` maps that declaration back to the member and its
/// overload types. The same declarations under a wrong suffix are renamed
/// by LLVM and are `Other` to hod.
#[test]
fn intrinsic_names_are_llvms() {
    let (Some(llvm_as), Some(llvm_dis)) = (llvm_tool(LLVM, "llvm-as"), llvm_tool(LLVM, "llvm-dis"))
    else {
        return;
    };
    let mut m = module();
    let mut seen = Vec::new();
    let (mut right, mut wrong) = (header(), header());
    let mut wrong_names = Vec::new();
    for &(name, signature) in INTRINSICS {
        let fn_ty = parse_type(&mut m.types, signature);
        let found = resolve_intrinsic(&m.types, name, fn_ty);
        let intrinsic = found.intrinsic;
        assert!(!matches!(intrinsic, Intrinsic::Other(_)), "{name} is not on the list");
        assert_eq!(found.overloads().len(), intrinsic.overload_count(), "{name}");
        let mangled = intrinsic.mangled_name(&m.types, found.overloads());
        assert_eq!(mangled.as_deref(), Some(name));
        assert!(!seen.contains(&intrinsic), "{name} is listed twice");
        seen.push(intrinsic);
        right += &declaration(&m, name, fn_ty);

        // The last overload suffix replaced by one that does not fit.
        if intrinsic.overload_count() > 0 {
            let stem = &name[..name.rfind('.').expect("a suffix")];
            let misnamed = format!("{stem}.i13");
            let found = resolve_intrinsic(&m.types, &misnamed, fn_ty);
            assert_eq!(found.intrinsic, Intrinsic::Other(&misnamed));
            wrong += &declaration(&m, &misnamed, fn_ty);
            wrong_names.push(misnamed);
        }
    }
    for &intrinsic in Intrinsic::ALL {
        assert!(seen.contains(&intrinsic), "{intrinsic:?} has no declaration in this test");
    }

    let kept = round_trip(&llvm_as, &llvm_dis, &right);
    for &(name, _) in INTRINSICS {
        if UPGRADED_BY_22.contains(&name) {
            continue;
        }
        assert!(kept.contains(&format!("@{name}(")), "LLVM renamed or dropped {name}:\n{kept}");
    }
    // The names LLVM 22 upgrades away are LLVM 19's.
    if let (Some(as19), Some(dis19)) = (llvm_tool(19, "llvm-as"), llvm_tool(19, "llvm-dis")) {
        let mut old = header();
        for &(name, signature) in INTRINSICS.iter().filter(|(n, _)| UPGRADED_BY_22.contains(n)) {
            let fn_ty = parse_type(&mut m.types, signature);
            old += &declaration(&m, name, fn_ty);
        }
        let kept = round_trip(&as19, &dis19, &old);
        for name in UPGRADED_BY_22 {
            assert!(
                kept.contains(&format!("@{name}(")),
                "LLVM 19 renamed or dropped {name}:\n{kept}"
            );
        }
    }
    let renamed = round_trip(&llvm_as, &llvm_dis, &wrong);
    for name in &wrong_names {
        assert!(!renamed.contains(&format!("@{name}(")), "LLVM kept the name {name}");
    }
    // The spelling without a suffix that hod accepts (§5.5) is one LLVM
    // accepts too, by renaming it.
    let old = round_trip(&llvm_as, &llvm_dis, "declare ptr @llvm.thread.pointer()\n");
    assert!(old.contains("@llvm.thread.pointer.p0("), "{old}");
    eprintln!(
        "intrinsics: {} names kept by LLVM {LLVM}, {} misnamed ones renamed",
        INTRINSICS.len(),
        wrong_names.len()
    );
}

/// A call of each intrinsic the no-code rule claims: the name, the
/// result type, the parameter types, the arguments.
const NO_CODE_CALLS: &[(&str, &str, &str, &str)] = &[
    ("llvm.lifetime.start.p0", "void", "ptr", "ptr %slot"),
    ("llvm.lifetime.end.p0", "void", "ptr", "ptr %slot"),
    ("llvm.invariant.start.p0", "ptr", "i64, ptr", "i64 8, ptr %p"),
    ("llvm.invariant.end.p0", "void", "ptr, i64, ptr", "ptr %p, i64 8, ptr %p"),
    ("llvm.assume", "void", "i1", "i1 %c"),
    ("llvm.donothing", "void", "", ""),
    (
        "llvm.experimental.noalias.scope.decl",
        "void",
        "metadata",
        "metadata !{!{!\"s\", !{!\"d\"}}}",
    ),
    (
        "llvm.var.annotation.p0.p0",
        "void",
        "ptr, ptr, ptr, i32, ptr",
        "ptr %p, ptr @s, ptr @s, i32 1, ptr null",
    ),
    ("llvm.expect.i64", "i64", "i64, i64", "i64 %a, i64 1"),
    (
        "llvm.expect.with.probability.i64",
        "i64",
        "i64, i64, double",
        "i64 %a, i64 1, double 5.000000e-01",
    ),
    ("llvm.annotation.i64.p0", "i64", "i64, ptr, ptr, i32", "i64 %a, ptr @s, ptr @s, i32 1"),
    (
        "llvm.ptr.annotation.p0.p0",
        "ptr",
        "ptr, ptr, ptr, i32, ptr",
        "ptr %p, ptr @s, ptr @s, i32 1, ptr null",
    ),
    ("llvm.launder.invariant.group.p0", "ptr", "ptr", "ptr %p"),
    ("llvm.strip.invariant.group.p0", "ptr", "ptr", "ptr %p"),
];

/// A module that reads a debug intrinsic call.
const DEBUG_CALL: &str = "\
define void @f(i32 %a) !dbg !4 {
  call void @llvm.dbg.value(metadata i32 %a, metadata !9, metadata !DIExpression()), !dbg !10
  ret void
}
declare void @llvm.dbg.value(metadata, metadata, metadata)
!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!3}
!0 = distinct !DICompileUnit(language: DW_LANG_C, file: !1, emissionKind: FullDebug)
!1 = !DIFile(filename: \"a.c\", directory: \"/\")
!3 = !{i32 2, !\"Debug Info Version\", i32 3}
!4 = distinct !DISubprogram(name: \"f\", scope: !1, file: !1, line: 1, type: !5, unit: !0, \
spFlags: DISPFlagDefinition)
!5 = !DISubroutineType(types: !6)
!6 = !{null}
!8 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)
!9 = !DILocalVariable(name: \"x\", scope: !4, file: !1, line: 1, type: !8)
!10 = !DILocation(line: 1, scope: !4)
";

/// §5.5 (the no-code rule): for each intrinsic a producer leaves out,
/// LLVM's own `-O0` code generator emits the same assembly for a function
/// with the call as for the function the rule makes of it: the call
/// dropped, or its result replaced by its first argument. The debug
/// intrinsics are not calls at all once LLVM has read them.
#[test]
fn no_code_intrinsics_generate_no_code_in_llvm() {
    let Some(tools) = common::tools(LLVM) else { return };
    let (Some(llvm_as), Some(llvm_dis)) = (llvm_tool(LLVM, "llvm-as"), llvm_tool(LLVM, "llvm-dis"))
    else {
        return;
    };
    let assembly = |function: &str, declarations: &str| {
        let text = format!(
            "{}{declarations}@s = private constant [2 x i8] c\"a\\00\"\n@out = global ptr null\n\
             define void @f(i64 %a, ptr %p, i1 %c) {{\n  %slot = alloca i64\n  \
             store i64 %a, ptr %slot\n{function}  ret void\n}}\n",
            header()
        );
        let outcome = hod_testutil::run_with(
            Command::new(&tools.llc).args(["-O0", "-o", "-", "-"]),
            text.as_bytes(),
            hod_testutil::DEFAULT_TIMEOUT,
        );
        assert!(outcome.success(), "llc:\n{}\n{text}", outcome.stderr_text());
        outcome.stdout_text()
    };
    let mut bases: Vec<&str> = Vec::new();
    for &(name, ret, params, args) in NO_CODE_CALLS {
        let rule = no_code_intrinsic(name).unwrap_or_else(|| panic!("{name} is not claimed"));
        let (base, _) = no_code_names()
            .filter(|(base, _)| name.starts_with(base))
            .max_by_key(|(base, _)| base.len())
            .expect("a base name");
        bases.push(base);
        let declaration = format!("declare {ret} @{name}({params})\n");
        let call = format!("call {ret} @{name}({args})\n");
        let (with, without) = match rule {
            NoCode::Drop if ret == "void" => (format!("  {call}"), String::new()),
            NoCode::Drop => (format!("  %r = {call}"), String::new()),
            NoCode::FirstArg => {
                let first = args.split(',').next().expect("an argument");
                let first = first.rsplit(' ').next().expect("a value");
                (
                    format!("  %r = {call}  store {ret} %r, ptr @out\n"),
                    format!("  store {ret} {first}, ptr @out\n"),
                )
            }
            NoCode::DebugRecord => panic!("{name}: a debug intrinsic is not a call"),
        };
        let (with, without) = (assembly(&with, &declaration), assembly(&without, &declaration));
        assert_eq!(with, without, "{name}: llc -O0 emits code for the call");
        assert!(with.contains("ret"), "{name}: no function was compiled:\n{with}");
    }
    // Every rule but the debug one was exercised.
    for (base, rule) in no_code_names() {
        assert_eq!(bases.contains(&base), rule != NoCode::DebugRecord, "{base}");
    }

    // LLVM reads a debug intrinsic call as a debug record, not a call.
    assert_eq!(no_code_intrinsic("llvm.dbg.value"), Some(NoCode::DebugRecord));
    let read = round_trip(&llvm_as, &llvm_dis, &(header() + DEBUG_CALL));
    assert!(read.contains("#dbg_value(i32 %a") && !read.contains("call "), "{read}");
    eprintln!("no-code intrinsics: {} calls compared at llc -O0", NO_CODE_CALLS.len());
}
