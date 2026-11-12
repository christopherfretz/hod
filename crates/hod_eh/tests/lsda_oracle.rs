//! `spec/eh.md` §9.8: `llc` is the oracle for the LSDA. Each case is
//! C++-shaped LLVM text that `llc` (every installed major, `-O0`, PIC, with
//! and without `-function-sections`) compiles for AArch64 and x86-64. Its
//! `.gcc_except_table` sections are decoded (with their relocations and the
//! `DW.stub` words they reach), turned into the rows `hod_cg` records
//! (`spec/codegen.md` §9.4), written again by `write_lsdas`, and compared:
//! the same sections (name, type, flags, alignment, group), every call
//! site with the same range, landing pad and action chain by meaning, the
//! same type table, the same stubs, each function's LSDA where llc put it,
//! and the bytes and relocations of every section identical.

#![allow(clippy::too_many_lines, clippy::format_push_string)]

mod common;

use common::lsda::lsda_trip;
use common::{compile_with, llc};
use hod_obj::Arch;
use hod_testutil::Scratch;

const PRELUDE: &str = r"
@_ZTIi = external constant ptr
@_ZTIl = external constant ptr
@_ZTId = external constant ptr
@_ZTI5Error = external constant ptr
@_ZTI4Base = external constant ptr
declare i32 @__gxx_personality_v0(...)
declare void @may_throw(i32)
declare void @no_throw(i32) nounwind
declare ptr @__cxa_begin_catch(ptr) nounwind
declare void @__cxa_end_catch()
declare void @__cxa_rethrow()
declare void @__cxa_call_unexpected(ptr)
declare void @__clang_call_terminate(ptr) nounwind
declare void @destroy(ptr) nounwind
declare i32 @llvm.eh.typeid.for(ptr) nounwind
";

/// The cases: a name and the module, without a target triple.
fn cases() -> Vec<(&'static str, String)> {
    let mut cases = vec![
        (
            // try { may_throw(); } catch (int) {} catch (long) {}
            // catch (Error &) {}
            "several_types",
            r"
define i32 @several_types(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw(i32 %x) to label %ok unwind label %lp
ok:
  ret i32 0
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi catch ptr @_ZTIl catch ptr @_ZTI5Error
  %exn = extractvalue { ptr, i32 } %l, 0
  %sel = extractvalue { ptr, i32 } %l, 1
  %ti = call i32 @llvm.eh.typeid.for(ptr @_ZTIi)
  %is_i = icmp eq i32 %sel, %ti
  br i1 %is_i, label %c_int, label %n1
n1:
  %tl = call i32 @llvm.eh.typeid.for(ptr @_ZTIl)
  %is_l = icmp eq i32 %sel, %tl
  br i1 %is_l, label %c_long, label %c_error
c_int:
  %p1 = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret i32 1
c_long:
  %p2 = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret i32 2
c_error:
  %p3 = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret i32 3
}
"
            .to_string(),
        ),
        (
            // try { may_throw(); may_throw(); } catch (int) {} catch (...) {}
            "catch_all",
            r"
define void @catch_all(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw(i32 %x) to label %a unwind label %lp
a:
  invoke void @may_throw(i32 1) to label %ok unwind label %lp
ok:
  ret void
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi catch ptr null
  %exn = extractvalue { ptr, i32 } %l, 0
  %p = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret void
}
"
            .to_string(),
        ),
        (
            // try { try { may_throw(); } catch (int) {} may_throw(); }
            // catch (Error &) {}: the inner pad's chain ends in the outer
            // pad's, which llc shares.
            "nested_try",
            r"
define void @nested_try(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw(i32 %x) to label %a unwind label %inner
a:
  invoke void @may_throw(i32 2) to label %ok unwind label %outer
ok:
  ret void
inner:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi catch ptr @_ZTI5Error
  %exn = extractvalue { ptr, i32 } %l, 0
  %sel = extractvalue { ptr, i32 } %l, 1
  %ti = call i32 @llvm.eh.typeid.for(ptr @_ZTIi)
  %is_i = icmp eq i32 %sel, %ti
  br i1 %is_i, label %c_int, label %to_outer
c_int:
  %p = call ptr @__cxa_begin_catch(ptr %exn)
  invoke void @__cxa_end_catch() to label %a unwind label %outer
to_outer:
  br label %c_error
outer:
  %o = landingpad { ptr, i32 } catch ptr @_ZTI5Error
  %oexn = extractvalue { ptr, i32 } %o, 0
  br label %c_error2
c_error:
  %q = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret void
c_error2:
  %q2 = call ptr @__cxa_begin_catch(ptr %oexn)
  call void @__cxa_end_catch()
  ret void
}
"
            .to_string(),
        ),
        (
            // { Guard a; may_throw(); { Guard b; may_throw(); } }
            "cleanups_only",
            r"
define void @cleanups_only(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  %a = alloca i32
  %b = alloca i32
  invoke void @may_throw(i32 %x) to label %s1 unwind label %lp_a
s1:
  invoke void @may_throw(i32 3) to label %s2 unwind label %lp_b
s2:
  call void @destroy(ptr %b)
  call void @destroy(ptr %a)
  ret void
lp_b:
  %lb = landingpad { ptr, i32 } cleanup
  call void @destroy(ptr %b)
  br label %cleanup_a
lp_a:
  %la = landingpad { ptr, i32 } cleanup
  br label %cleanup_a
cleanup_a:
  %e = phi { ptr, i32 } [ %lb, %lp_b ], [ %la, %lp_a ]
  call void @destroy(ptr %a)
  resume { ptr, i32 } %e
}
"
            .to_string(),
        ),
        (
            // void filtered() throw(int, long) { Guard g; may_throw(); }
            // void nothing() throw() { may_throw(); }
            // and a catch next to a filter that shares its tail.
            "filters",
            r"
define void @filtered(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  %g = alloca i32
  invoke void @may_throw(i32 %x) to label %ok unwind label %lp
ok:
  call void @destroy(ptr %g)
  ret void
lp:
  %l = landingpad { ptr, i32 } cleanup filter [2 x ptr] [ptr @_ZTIi, ptr @_ZTIl]
  %exn = extractvalue { ptr, i32 } %l, 0
  %sel = extractvalue { ptr, i32 } %l, 1
  call void @destroy(ptr %g)
  %bad = icmp slt i32 %sel, 0
  br i1 %bad, label %unexpected, label %resume
unexpected:
  call void @__cxa_call_unexpected(ptr %exn)
  unreachable
resume:
  resume { ptr, i32 } %l
}
define void @nothing(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw(i32 %x) to label %a unwind label %lp
a:
  invoke void @may_throw(i32 4) to label %ok unwind label %lp2
ok:
  ret void
lp:
  %l = landingpad { ptr, i32 } filter [0 x ptr] zeroinitializer
  %exn = extractvalue { ptr, i32 } %l, 0
  call void @__cxa_call_unexpected(ptr %exn)
  unreachable
lp2:
  %l2 = landingpad { ptr, i32 } catch ptr @_ZTId filter [1 x ptr] [ptr @_ZTIl]
  %exn2 = extractvalue { ptr, i32 } %l2, 0
  call void @__cxa_call_unexpected(ptr %exn2)
  unreachable
}
"
            .to_string(),
        ),
        (
            // try { may_throw(); } catch (Error &) { Guard g; throw; }
            "rethrow",
            r"
define void @rethrow(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  %g = alloca i32
  invoke void @may_throw(i32 %x) to label %ok unwind label %lp
ok:
  ret void
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTI5Error
  %exn = extractvalue { ptr, i32 } %l, 0
  %p = call ptr @__cxa_begin_catch(ptr %exn)
  invoke void @__cxa_rethrow() to label %unr unwind label %lp_end
lp_end:
  %l2 = landingpad { ptr, i32 } cleanup
  call void @destroy(ptr %g)
  invoke void @__cxa_end_catch() to label %res unwind label %term
res:
  resume { ptr, i32 } %l2
term:
  %t = landingpad { ptr, i32 } catch ptr null
  %texn = extractvalue { ptr, i32 } %t, 0
  call void @__clang_call_terminate(ptr %texn)
  unreachable
unr:
  unreachable
}
"
            .to_string(),
        ),
        (
            // template <class T> void templ() try { ... } catch (T) {}
            "comdat",
            r"
$_Z5templIiEvv = comdat any
define linkonce_odr void @_Z5templIiEvv() comdat personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw(i32 5) to label %ok unwind label %lp
ok:
  ret void
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %l, 0
  %p = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret void
}
define void @plain_next_to_comdat() personality ptr @__gxx_personality_v0 {
entry:
  invoke void @may_throw(i32 6) to label %ok unwind label %lp
ok:
  ret void
lp:
  %l = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %l
}
"
            .to_string(),
        ),
        (
            // Calls that cannot unwind between ones that can: a plain call
            // before the first invoke, a nounwind call between two invokes
            // of one pad (merged), a plain call between two invokes of one
            // pad (not merged), a call after the last.
            "nounwind_between",
            r"
define void @mixed(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  call void @may_throw(i32 0)
  invoke void @may_throw(i32 %x) to label %a unwind label %lp
a:
  call void @no_throw(i32 1)
  invoke void @may_throw(i32 2) to label %b unwind label %lp
b:
  call void @may_throw(i32 3)
  invoke void @may_throw(i32 4) to label %c unwind label %lp
c:
  call void @no_throw(i32 5)
  call void @may_throw(i32 6)
  ret void
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTI4Base
  %exn = extractvalue { ptr, i32 } %l, 0
  %p = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret void
}
define void @only_nounwind(i32 %x) personality ptr @__gxx_personality_v0 {
entry:
  call void @no_throw(i32 0)
  invoke void @may_throw(i32 %x) to label %a unwind label %lp
a:
  call void @no_throw(i32 1)
  ret void
lp:
  %l = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %l
}
"
            .to_string(),
        ),
    ];
    // Many call sites, pads and types: ULEB128 fields of two bytes and a
    // TType base offset beyond 127.
    let mut many = String::new();
    for i in 0..24 {
        many.push_str(&format!("@_ZTI1T{i} = external constant ptr\n"));
    }
    many.push_str("define void @many(i32 %x) personality ptr @__gxx_personality_v0 {\nentry:\n");
    for i in 0..48 {
        let pad = i % 6;
        many.push_str(&format!(
            "  invoke void @may_throw(i32 {i}) to label %s{i} unwind label %lp{pad}\ns{i}:\n"
        ));
        if i % 5 == 0 {
            many.push_str(&format!("  call void @may_throw(i32 {})\n", 100 + i));
        }
    }
    many.push_str("  ret void\n");
    for pad in 0..6 {
        let clauses: Vec<String> =
            (0..4).map(|k| format!("catch ptr @_ZTI1T{}", (pad * 4 + k) % 24)).collect();
        many.push_str(&format!(
            "lp{pad}:\n  %l{pad} = landingpad {{ ptr, i32 }} {}\n  ret void\n",
            clauses.join(" ")
        ));
    }
    many.push_str("}\n");
    cases.push(("many", many));
    cases
}

fn targets() -> [(Arch, &'static str); 2] {
    [(Arch::AArch64, "aarch64-unknown-linux-gnu"), (Arch::X86_64, "x86_64-unknown-linux-gnu")]
}

fn oracle(major: u32) {
    let Some(llc) = llc(major) else { return };
    let scratch = Scratch::new(&format!("lsda-llc{major}"));
    let mut summary = Vec::new();
    let mut features = std::collections::BTreeSet::new();
    for (arch, triple) in targets() {
        for (name, body) in cases() {
            let ir = format!("{PRELUDE}{body}");
            for function_sections in [false, true] {
                let fs = if function_sections { "-fs" } else { "" };
                let label = format!("{name}{fs}-{triple}-{major}");
                let extra: &[&str] = if function_sections { &["-function-sections"] } else { &[] };
                let object = compile_with(&llc, &scratch, &label, &ir, triple, extra);
                for reverse in [false, true] {
                    let t = lsda_trip(
                        &label,
                        arch,
                        &ir,
                        &object,
                        function_sections,
                        reverse,
                        &|_, _| {},
                        true,
                    );
                    assert!(t.identical, "{label}: byte-identical to llc");
                    if !reverse {
                        features.extend(t.features.iter().copied());
                        summary.push(format!(
                            "{label}: {} LSDA, {} call sites, byte-identical",
                            t.functions, t.sites
                        ));
                    }
                }
            }
        }
    }
    for f in [
        "catch",
        "catch-all",
        "cleanup",
        "filter",
        "empty filter",
        "shared tail",
        "no-pad entry",
        "merged rows",
        "comdat",
        "long ULEB128",
    ] {
        assert!(features.contains(f), "the cases exercise {f}: {features:?}");
    }
    eprintln!("{}", summary.join("\n"));
}

/// §9.8 with LLVM 19.
#[test]
fn llc19_lsda_round_trips() {
    oracle(19);
}

/// §9.8 with LLVM 22.
#[test]
fn llc22_lsda_round_trips() {
    oracle(22);
}

/// The comparison sees a wrong table: a pad's landing address moved by one
/// instruction, or a catch naming another type, fails the round trip.
#[test]
fn wrong_rows_fail_the_round_trip() {
    let Some(llc) = llc(22) else { return };
    let scratch = Scratch::new("lsda-wrong");
    let (name, body) = cases().swap_remove(0);
    let ir = format!("{PRELUDE}{body}");
    for (arch, triple) in targets() {
        let label = format!("{name}-{triple}-wrong");
        let object = compile_with(&llc, &scratch, &label, &ir, triple, &[]);
        let wrong_landing = |_: &str, rows: &mut common::lsda::Rows| {
            rows.pads[0].offset += 4;
            for c in &mut rows.call_sites {
                c.landing = 0;
            }
        };
        let wrong_type = |_: &str, rows: &mut common::lsda::Rows| {
            rows.clauses[0] = hod_eh::Clause::Catch(2);
        };
        for mutate in [&wrong_landing as &dyn Fn(&str, &mut common::lsda::Rows), &wrong_type] {
            let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                lsda_trip(&label, arch, &ir, &object, false, false, mutate, true)
            }))
            .is_err();
            assert!(failed, "{label}: a wrong table must fail the comparison");
        }
    }
}
