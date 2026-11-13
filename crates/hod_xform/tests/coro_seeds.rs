//! Design 0005's seeds (`docs/design/0005-data/`), compiled by kiln before
//! LLVM's passes and cut to their coroutines with `llvm-extract`
//! (`tests/data/seeds/llvm{19,22}/`), at the plan level: both dialects,
//! each with kiln's lifetime markers and without them.

mod common;

use std::path::PathBuf;

use hod_ir::{Body, Module, SlotId};
use hod_xform::coro::{self, Analysis, FRAME_ACCESS_AFTER_SUSPEND, Region, SlotClass};

/// The seed's text: as kiln wrote it, or without its lifetime markers.
fn seed(name: &str, major: u32, markers: bool) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/data/seeds/llvm{major}/{name}-{major}.ll"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if markers {
        return text;
    }
    let kept: Vec<&str> =
        text.lines().filter(|line| !line.contains("call void @llvm.lifetime.")).collect();
    kept.join("\n")
}

/// Every (major, markers) variant of the seed.
fn variants(name: &str) -> impl Iterator<Item = (String, String)> + '_ {
    [19, 22].into_iter().flat_map(move |major| {
        [true, false].into_iter().map(move |markers| {
            (format!("{name}-{major} markers={markers}"), seed(name, major, markers))
        })
    })
}

/// The `index`-th entry alloca of `function` whose line contains `pattern`.
fn alloca(text: &str, function: &str, pattern: &str, index: usize) -> SlotId {
    let define = format!("@{function}(");
    let body = text
        .split("\ndefine ")
        .find(|chunk| chunk.contains(&define))
        .unwrap_or_else(|| panic!("no {function}"));
    let allocas: Vec<&str> = body.lines().filter(|line| line.contains(" = alloca ")).collect();
    let k = allocas
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(pattern))
        .nth(index)
        .unwrap_or_else(|| panic!("{function}: no alloca {pattern} #{index}"))
        .0;
    SlotId::new(k as u32)
}

fn body_of(text: &str, function: &str) -> (Module, Body) {
    common::parse_one(text, function)
}

fn analysis(module: &Module, body: &Body, what: &str) -> Analysis {
    coro::analyze(module, body)
        .unwrap_or_else(|r| panic!("{what}: refused {r}"))
        .unwrap_or_else(|| panic!("{what}: not a coroutine"))
}

/// Rule 4 or rule 5, as the variant has markers or not.
fn escaped(markers: bool) -> SlotClass {
    if markers { SlotClass::EscapedWithMarkers } else { SlotClass::EscapedWithoutMarkers }
}

/// A local whose address escapes before a suspend and is written through
/// it after; an object registered by address before a suspend.
#[test]
fn escaped_local() {
    for (what, text) in variants("escaped-local") {
        let markers = what.ends_with("true");
        let (module, body) = body_of(&text, "_Z7pointerv");
        let plan = coro::plan(&module, &body).unwrap().unwrap();
        let classes = analysis(&module, &body, &what).classes;
        let x = alloca(&text, "_Z7pointerv", "alloca i32", 1);
        let y = alloca(&text, "_Z7pointerv", "alloca i32", 2);
        let p = alloca(&text, "_Z7pointerv", "alloca ptr", 1);
        let junk = alloca(&text, "_Z7pointerv", "[64 x i32]", 0);
        assert_eq!(classes[x.index()], escaped(markers), "{what}");
        assert!(plan.heap_slots.contains(x), "{what}: x");
        assert!(plan.heap_slots.contains(y), "{what}: y");
        assert!(plan.heap_slots.contains(p), "{what}: p");
        // `junk` lives after the suspend only: on the stack when markers
        // say so; rule 5 without them.
        assert_eq!(plan.heap_slots.contains(junk), !markers, "{what}: junk");

        let (module, body) = body_of(&text, "_Z10registeredv");
        let plan = coro::plan(&module, &body).unwrap().unwrap();
        let t = alloca(&text, "_Z10registeredv", "%struct.T", 0);
        let junk = alloca(&text, "_Z10registeredv", "[64 x i64]", 0);
        assert!(plan.heap_slots.contains(t), "{what}: t");
        assert_eq!(plan.heap_slots.contains(junk), !markers, "{what}: junk");
    }
}

/// The promise's `operator new` takes the parameters by reference before
/// `coro.begin`: their allocas are rule 5 and copied at `coro.begin`.
#[test]
fn params_before_begin() {
    for (what, text) in variants("params-before-begin") {
        let (module, body) = body_of(&text, "_Z1filc");
        let plan = coro::plan(&module, &body).unwrap().unwrap();
        let a = analysis(&module, &body, &what);
        let params = [
            alloca(&text, "_Z1filc", "alloca i32", 0),
            alloca(&text, "_Z1filc", "alloca i64", 0),
            alloca(&text, "_Z1filc", "alloca i8", 0),
        ];
        assert_eq!(plan.begin_copies, params, "{what}");
        for slot in params {
            assert_eq!(a.classes[slot.index()], SlotClass::EscapedWithoutMarkers, "{what}");
            assert!(plan.heap_slots.contains(slot), "{what}");
        }
        assert!(plan.begin_value_copies.is_empty(), "{what}");
    }
}

/// `get_return_object` throws, and a body throws past its locals: the
/// plan holds (no frame access on the −1 path or after the free), and a
/// local with a destructor is in the frame when it lives across a suspend.
#[test]
fn gro_throws_uaf() {
    for (what, text) in variants("gro-throws-uaf") {
        for function in ["_Z4leafi", "_Z5chaini"] {
            let (module, body) = body_of(&text, function);
            let plan = coro::plan(&module, &body)
                .unwrap_or_else(|r| panic!("{what} {function}: {r}"))
                .unwrap();
            assert!(plan.has_unwind_end, "{what} {function}");
            assert!(plan.final_index.is_some(), "{what} {function}");
            // `chain`'s `d` lives across its `co_await`s; `leaf`'s across
            // none, so it is in the frame only by rule 5.
            let d = alloca(&text, function, "%struct.Dtor", 0);
            let markers = what.ends_with("true");
            let crosses = function == "_Z5chaini";
            assert_eq!(plan.heap_slots.contains(d), crosses || !markers, "{what} {function}: d");
        }
    }
}

/// A contract postcondition reads the return object (a rule-5 slot)
/// after the ramp freed the frame: refused by name.
#[test]
fn post_sync() {
    for (what, text) in variants("post-sync") {
        let (module, body) = body_of(&text, "_Z1fv");
        let a = analysis(&module, &body, &what);
        let ret = alloca(&text, "_Z1fv", "%struct.sync", 0);
        assert!(a.plan.heap_slots.contains(ret), "{what}");
        assert!(!a.violations.is_empty(), "{what}");
        assert!(a.violations.iter().all(|v| v.slot == Some(ret) && v.region == Region::Suspended));
        let refusal = coro::plan(&module, &body).unwrap_err();
        assert_eq!((refusal.construct, refusal.slot), (FRAME_ACCESS_AFTER_SUSPEND, Some(ret)));
    }
}

/// `await_suspend` resumes the coroutine to completion before it returns
/// (`.void`, `.bool`, `.handle`): the plan holds, every suspend has its
/// wrapper, and the `.bool` wrapper's path to its suspend is suspended.
#[test]
fn resume_inside_await_suspend() {
    for (what, text) in variants("resume-inside-await-suspend") {
        for function in [
            "_Z3runI15resume_now_voidE4taskPKc",
            "_Z3runI15resume_now_boolE4taskPKc",
            "_Z3runI17resume_now_handleE4taskPKc",
        ] {
            let (module, body) = body_of(&text, function);
            let plan = coro::plan(&module, &body)
                .unwrap_or_else(|r| panic!("{what} {function}: {r}"))
                .unwrap();
            assert!(plan.suspends.iter().all(|s| s.wrapper.is_some()), "{what} {function}");
            assert!(!plan.suspended_region.is_empty(), "{what} {function}");
            if function.contains("bool") {
                // The block after the `.bool` wrapper's own block.
                let suspend_block = |s: &coro::Suspend| {
                    body.blocks().find(|b| b.insts().any(|i| i.id() == s.suspend)).unwrap().id()
                };
                let wrapper_block = |s: &coro::Suspend| {
                    body.blocks()
                        .find(|b| b.insts().any(|i| Some(i.id()) == s.wrapper))
                        .unwrap()
                        .id()
                };
                let bool_suspend = plan
                    .suspends
                    .iter()
                    .find(|s| wrapper_block(s) != suspend_block(s))
                    .unwrap_or_else(|| panic!("{what}: no .bool suspend in its own block"));
                assert!(plan.suspended_region.contains(suspend_block(bool_suspend)));
            }
        }
    }
}

/// `hod_bc` reads the same markers and `!coro.outside.frame` flags as
/// `hod_ll`, so the plan is the same from bitcode (`llvm-as` of the major;
/// skipped without it unless `HOD_REQUIRE_TOOLS` is set).
#[test]
fn bitcode_gives_the_same_plans() {
    for name in ["escaped-local", "params-before-begin", "gro-throws-uaf", "post-sync", "outside"] {
        for major in [19, 22] {
            // `outside`: `escaped-local` with its `junk` arrays marked
            // `!coro.outside.frame`.
            let text = if name == "outside" {
                seed("escaped-local", major, true)
                    .replace(
                        " = alloca [64 x i32], align 4",
                        " = alloca [64 x i32], align 4, !coro.outside.frame !{}",
                    )
                    .replace(
                        " = alloca [64 x i64], align 8",
                        " = alloca [64 x i64], align 8, !coro.outside.frame !{}",
                    )
            } else {
                seed(name, major, true)
            };
            let Some(bitcode) = common::assemble(major, text.as_bytes()) else { continue };
            let (ll_module, ll) = common::parse_all(text.as_bytes());
            let (bc_module, bc) = common::read_bitcode(&bitcode);
            for (l, b) in ll.iter().zip(&bc) {
                assert_eq!(l.name, b.name);
                let (Ok(lb), Ok(bb)) = (&l.body, &b.body) else { continue };
                assert_eq!(
                    lb.lifetime_markers(),
                    bb.lifetime_markers(),
                    "{name}-{major} {}",
                    l.name
                );
                let flags = |body: &Body| -> Vec<bool> {
                    body.slots().iter().map(|s| s.outside_frame()).collect()
                };
                assert_eq!(flags(lb), flags(bb), "{name}-{major} {}", l.name);
                assert_eq!(
                    flags(lb).contains(&true),
                    name == "outside",
                    "{name}-{major} {}",
                    l.name
                );
                assert_eq!(
                    coro::analyze(&ll_module, lb).map(|a| a.map(|a| a.plan)),
                    coro::analyze(&bc_module, bb).map(|a| a.map(|a| a.plan)),
                    "{name}-{major} {}",
                    l.name
                );
            }
        }
    }
}
