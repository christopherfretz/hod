//! The coroutine plan on minimal bodies (`spec/xform.md` §3): each rule,
//! each refusal by name, the `coro.begin` copies, the suspended region.
//!
//! Each body is IR text in both dialects: LLVM 22 (`void coro.end`,
//! `coro.is_in_ramp`) and LLVM 19 (`i1 coro.end`; `lifetime.start` with a
//! size). Slots are numbered in `alloca` order.

mod common;

use std::fmt::Write as _;

use hod_ir::{InstId, InstKind, SlotId};
use hod_xform::coro::{
    self, ARGUMENT_ACROSS_SUSPEND, Analysis, BEGIN_SHAPE, CoroPlan, DYNAMIC_ALLOCA,
    FRAME_ACCESS_AFTER_SUSPEND, FRAME_BEFORE_BEGIN, FRAME_INTRINSIC, OUTSIDE_FRAME_ACROSS_SUSPEND,
    OVERALIGNED_FRAME, REFUSALS, Refusal, Region, SUSPEND_SHAPE, SlotClass,
    UNWIND_END_WITHOUT_FINAL, VARIADIC,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dialect {
    Llvm19,
    Llvm22,
}

const DIALECTS: [Dialect; 2] = [Dialect::Llvm19, Dialect::Llvm22];

impl Dialect {
    /// `llvm.lifetime.start` (or `end`) of `ptr`.
    fn lifetime(self, start: bool, ptr: &str) -> String {
        let which = if start { "start" } else { "end" };
        match self {
            Dialect::Llvm19 => format!("call void @llvm.lifetime.{which}.p0(i64 4, ptr {ptr})"),
            Dialect::Llvm22 => format!("call void @llvm.lifetime.{which}.p0(ptr {ptr})"),
        }
    }

    /// A non-unwinding `coro.end`.
    fn end(self) -> &'static str {
        match self {
            Dialect::Llvm19 => "%end = call i1 @llvm.coro.end(ptr null, i1 false, token none)",
            Dialect::Llvm22 => "call void @llvm.coro.end(ptr null, i1 false, token none)",
        }
    }

    /// An unwinding `coro.end` and the branch whose `ramp` side only the
    /// ramp takes.
    fn unwind_end(self, ramp: &str, other: &str) -> String {
        match self {
            Dialect::Llvm19 => format!(
                "%uend = call i1 @llvm.coro.end(ptr null, i1 true, token none)\n  br i1 %uend, label %{other}, label %{ramp}"
            ),
            Dialect::Llvm22 => format!(
                "call void @llvm.coro.end(ptr null, i1 true, token none)\n  %inramp = call i1 @llvm.coro.is_in_ramp()\n  br i1 %inramp, label %{ramp}, label %{other}"
            ),
        }
    }

    fn declarations(self) -> String {
        let (end, lifetime) = match self {
            Dialect::Llvm19 => (
                "declare i1 @llvm.coro.end(ptr, i1, token)",
                "declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture)\ndeclare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture)",
            ),
            Dialect::Llvm22 => (
                "declare void @llvm.coro.end(ptr, i1, token)\ndeclare i1 @llvm.coro.is_in_ramp()",
                "declare void @llvm.lifetime.start.p0(ptr captures(none))\ndeclare void @llvm.lifetime.end.p0(ptr captures(none))",
            ),
        };
        format!(
            "{end}\n{lifetime}\n{}",
            "declare token @llvm.coro.id(i32, ptr readnone, ptr readonly, ptr)
declare i1 @llvm.coro.alloc(token)
declare i64 @llvm.coro.size.i64()
declare i64 @llvm.coro.align.i64()
declare ptr @llvm.coro.begin(token, ptr writeonly)
declare token @llvm.coro.save(ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr readonly)
declare ptr @llvm.coro.frame()
declare void @llvm.coro.await.suspend.void(ptr, ptr, ptr)
declare i1 @llvm.coro.await.suspend.bool(ptr, ptr, ptr)
declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1 immarg)
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)
declare ptr @malloc(i64)
declare void @free(ptr)
declare void @use(ptr)
declare void @sink(i32)
declare i32 @get()
declare void @wrapper(ptr, ptr)
declare void @dtor(ptr)
!0 = !{}
"
        )
    }
}

/// The parts of a coroutine `@f` with one suspend point: `slots` (the
/// `alloca`s), `pre` (before `coro.id`), `before` (after `coro.begin`,
/// before the suspend), `after` (on the resume edge), `ramp` (after the
/// `coro.end` of the return block). The function takes `ptr %arg`.
#[derive(Default, Clone)]
struct Coro<'a> {
    signature: &'a str,
    slots: &'a str,
    pre: &'a str,
    promise: &'a str,
    before: &'a str,
    /// Between the `await_suspend` wrapper and the suspend.
    wrapped: &'a str,
    after: &'a str,
    cleanup: &'a str,
    ramp: &'a str,
}

impl Coro<'_> {
    fn text(&self, d: Dialect) -> String {
        let signature =
            if self.signature.is_empty() { "void @f(ptr %arg)" } else { self.signature };
        let promise = if self.promise.is_empty() { "null" } else { self.promise };
        format!(
            "define {signature} presplitcoroutine {{
entry:
{slots}
{pre}
  %id = call token @llvm.coro.id(i32 16, ptr {promise}, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
{before}
  %save = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr null, ptr %hdl, ptr @wrapper)
{wrapped}
  %sp = call i8 @llvm.coro.suspend(token %save, i1 false)
  switch i8 %sp, label %ret [
    i8 0, label %resume
    i8 1, label %cleanup
  ]
resume:
{after}
  br label %cleanup
cleanup:
  %fm = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @free(ptr %fm)
{cleanup}
  br label %ret
ret:
  {end}
{ramp}
  ret void
}}
{decls}",
            slots = self.slots,
            pre = self.pre,
            before = self.before,
            wrapped = self.wrapped,
            after = self.after,
            cleanup = self.cleanup,
            ramp = self.ramp,
            end = d.end(),
            decls = d.declarations(),
        )
    }
}

fn analyze_text(text: &str) -> Result<Option<Analysis>, Refusal> {
    let (module, body) = common::parse_one(text, "f");
    coro::analyze(&module, &body)
}

fn plan_text(text: &str) -> Result<Option<CoroPlan>, Refusal> {
    let (module, body) = common::parse_one(text, "f");
    coro::plan(&module, &body)
}

/// The analysis, which must exist and refuse nothing.
fn analysis(text: &str) -> Analysis {
    analyze_text(text).unwrap_or_else(|r| panic!("refused: {r}\n{text}")).expect("a coroutine")
}

/// The plan, which must exist.
fn planned(text: &str) -> CoroPlan {
    plan_text(text).unwrap_or_else(|r| panic!("refused: {r}\n{text}")).expect("a coroutine")
}

fn refusal(text: &str) -> &'static str {
    match plan_text(text) {
        Err(r) => r.construct,
        Ok(plan) => panic!("not refused: {plan:?}\n{text}"),
    }
}

const fn slot(k: u32) -> SlotId {
    SlotId::new(k)
}

/// The instruction defining the result that `pick` finds first.
fn find(text: &str, pick: impl Fn(&InstKind<'_>) -> bool) -> InstId {
    let (_, body) = common::parse_one(text, "f");
    body.blocks()
        .flat_map(|b| b.insts().map(|i| (i.id(), i.kind())).collect::<Vec<_>>())
        .find(|(_, kind)| pick(kind))
        .map(|(id, _)| id)
        .expect("no such instruction")
}

fn is_call_to(text: &str, name: &str) -> InstId {
    let (module, body) = common::parse_one(text, "f");
    for block in body.blocks() {
        for inst in block.insts() {
            if let InstKind::Call(call) = inst.kind()
                && let hod_ir::ValueKind::Sym(sym) = call.callee_value().kind()
                && module.symbol(sym).name() == name.as_bytes()
            {
                return inst.id();
            }
        }
    }
    panic!("no call to {name}");
}

#[test]
fn a_function_without_presplitcoroutine_has_no_plan() {
    let text = "define void @f() {\n  ret void\n}\n";
    assert_eq!(plan_text(text), Ok(None));
}

#[test]
fn the_shape_is_read() {
    for d in DIALECTS {
        let text = Coro::default().text(d);
        let plan = planned(&text);
        assert_eq!(plan.suspends.len(), 1);
        let s = plan.suspends[0];
        assert_eq!((s.index, s.is_final), (0, false));
        assert!(s.wrapper.is_some());
        assert_eq!(plan.final_index, None);
        assert!(!plan.has_unwind_end);
        assert_eq!((plan.promise, plan.promise_offset, plan.frame_align), (None, 16, 16));
        assert!(plan.heap_slots.is_empty() && plan.heap_values.is_empty());
        // `ret` is reached by the −1 edge.
        assert_eq!(plan.suspended_region.count(), 1);
    }
}

// ---- rule 1 ----

#[test]
fn rule_1_the_promise_is_a_header_field() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %p = alloca i32, align 4",
            promise: "%p",
            before: "  store i32 1, ptr %p\n  call void @use(ptr %p)",
            after: "  %v = load i32, ptr %p\n  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        assert_eq!(a.classes[0], SlotClass::Promise);
        assert_eq!(a.plan.promise, Some(slot(0)));
        assert!(!a.plan.heap_slots.contains(slot(0)));
        assert_eq!(a.plan.promise_offset, 16);
    }
}

#[test]
fn an_overaligned_promise_moves_and_needs_coro_align() {
    for d in DIALECTS {
        let mut c = Coro {
            slots: "  %p = alloca i64, align 32",
            promise: "%p",
            after: "  call void @use(ptr %p)",
            ..Coro::default()
        };
        assert_eq!(refusal(&c.text(d)), OVERALIGNED_FRAME);
        c.before = "  %al = call i64 @llvm.coro.align.i64()";
        let plan = planned(&c.text(d));
        assert_eq!((plan.promise_offset, plan.frame_align), (32, 32));
    }
}

// ---- rule 2 ----

#[test]
fn rule_2_outside_frame_stays_on_the_stack() {
    for d in DIALECTS {
        let mut c = Coro {
            slots: "  %o = alloca i32, align 4, !coro.outside.frame !0",
            before: "  call void @use(ptr %o)",
            ..Coro::default()
        };
        let a = analysis(&c.text(d));
        assert_eq!(a.classes[0], SlotClass::OutsideFrame);
        assert!(a.plan.heap_slots.is_empty());
        c.after = "  %v = load i32, ptr %o\n  call void @sink(i32 %v)";
        assert_eq!(refusal(&c.text(d)), OUTSIDE_FRAME_ACROSS_SUSPEND);
    }
}

// ---- rule 3 ----

#[test]
fn rule_3_a_load_that_sees_a_store_across_a_resumption_edge() {
    for d in DIALECTS {
        let crossing = Coro {
            slots: "  %x = alloca i32, align 4",
            before: "  store i32 1, ptr %x",
            after: "  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            ..Coro::default()
        };
        let a = analysis(&crossing.text(d));
        assert_eq!(a.classes[0], SlotClass::Unescaped);
        assert!(a.plan.heap_slots.contains(slot(0)));

        // Stored again on the resumed side before the load: stack.
        let killed = Coro {
            after: "  store i32 2, ptr %x\n  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            ..crossing.clone()
        };
        assert!(planned(&killed.text(d)).heap_slots.is_empty());

        // Only read before the suspend: stack.
        let before_only = Coro {
            before: "  store i32 1, ptr %x\n  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            after: "  store i32 2, ptr %x",
            ..crossing.clone()
        };
        assert!(planned(&before_only.text(d)).heap_slots.is_empty());
    }
}

#[test]
fn rule_3_only_whole_stores_kill() {
    for d in DIALECTS {
        // A partial store after the edge does not hide the earlier one.
        let partial = Coro {
            slots: "  %x = alloca i64, align 8",
            before: "  store i64 1, ptr %x",
            after: "  store i32 2, ptr %x\n  %v = load i64, ptr %x\n  %t = trunc i64 %v to i32\n  call void @sink(i32 %t)",
            ..Coro::default()
        };
        assert!(planned(&partial.text(d)).heap_slots.contains(slot(0)));
        // A partial store before the edge is still a store.
        let partial_before = Coro {
            before: "  store i32 1, ptr %x",
            after: "  %v = load i64, ptr %x\n  %t = trunc i64 %v to i32\n  call void @sink(i32 %t)",
            ..partial.clone()
        };
        assert!(planned(&partial_before.text(d)).heap_slots.contains(slot(0)));
        // A whole `memcpy` kills; the source of one is read.
        let copied = Coro {
            slots: "  %x = alloca i64, align 8\n  %y = alloca i64, align 8",
            before: "  store i64 1, ptr %x\n  store i64 1, ptr %y",
            after: "  call void @llvm.memcpy.p0.p0.i64(ptr %x, ptr %y, i64 8, i1 false)\n  %v = load i64, ptr %x\n  %t = trunc i64 %v to i32\n  call void @sink(i32 %t)",
            ..Coro::default()
        };
        let plan = planned(&copied.text(d));
        assert!(!plan.heap_slots.contains(slot(0)), "a whole memcpy kills");
        assert!(plan.heap_slots.contains(slot(1)), "the memcpy source is read");
    }
}

// ---- rule 4 ----

#[test]
fn rule_4a_a_use_reached_from_a_start_through_a_resumption_edge() {
    for d in DIALECTS {
        let start = d.lifetime(true, "%e");
        let end = d.lifetime(false, "%e");
        let crossing = Coro {
            slots: "  %e = alloca i32, align 4",
            before: &format!("  {start}\n  call void @use(ptr %e)"),
            after: &format!("  call void @use(ptr %e)\n  {end}"),
            ..Coro::default()
        };
        let a = analysis(&crossing.text(d));
        assert_eq!(a.classes[0], SlotClass::EscapedWithMarkers);
        assert!(a.plan.heap_slots.contains(slot(0)));

        let after_only = format!("  {start}\n  call void @use(ptr %e)\n  {end}");
        let local = Coro { before: "", after: &after_only, ..crossing.clone() };
        let a = analysis(&local.text(d));
        assert_eq!(a.classes[0], SlotClass::EscapedWithMarkers);
        assert!(a.plan.heap_slots.is_empty());

        // The start through an alias at offset 0 counts.
        let alias = Coro {
            before: &format!(
                "  %g = getelementptr i8, ptr %e, i64 0\n  {}\n  call void @use(ptr %e)",
                d.lifetime(true, "%g")
            ),
            ..crossing.clone()
        };
        assert_eq!(analysis(&alias.text(d)).classes[0], SlotClass::EscapedWithMarkers);
    }
}

/// Rule 4 (b): a start reached from a start through a resumption edge
/// (a loop around the suspend), though no use crosses.
#[test]
fn rule_4b_a_start_reached_from_a_start_across_the_edge() {
    for d in DIALECTS {
        let text = format!(
            "define void @f() presplitcoroutine {{
entry:
  %e = alloca i32, align 4
  %id = call token @llvm.coro.id(i32 16, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)
  br label %loop
loop:
  {start}
  call void @use(ptr %e)
  {end}
  %save = call token @llvm.coro.save(ptr null)
  call void @llvm.coro.await.suspend.void(ptr null, ptr %hdl, ptr @wrapper)
  %sp = call i8 @llvm.coro.suspend(token %save, i1 false)
  switch i8 %sp, label %ret [
    i8 0, label %loop
    i8 1, label %cleanup
  ]
cleanup:
  %fm = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @free(ptr %fm)
  br label %ret
ret:
  {coro_end}
  ret void
}}
{decls}",
            start = d.lifetime(true, "%e"),
            end = d.lifetime(false, "%e"),
            coro_end = d.end(),
            decls = d.declarations(),
        );
        let a = analysis(&text);
        assert_eq!(a.classes[0], SlotClass::EscapedWithMarkers);
        assert!(a.plan.heap_slots.contains(slot(0)));
    }
}

// ---- rule 5 ----

#[test]
fn rule_5_escaped_without_markers_is_always_in_the_heap() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %e = alloca i32, align 4",
            before: "  call void @use(ptr %e)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        assert_eq!(a.classes[0], SlotClass::EscapedWithoutMarkers);
        assert!(a.plan.heap_slots.contains(slot(0)));
        // A marker on another slot does not make this one rule 4.
        let other = Coro {
            slots: "  %e = alloca i32, align 4\n  %m = alloca i32, align 4",
            before: &format!("  {}\n  call void @use(ptr %e)", d.lifetime(true, "%m")),
            ..Coro::default()
        }
        .text(d);
        assert_eq!(analysis(&other).classes[0], SlotClass::EscapedWithoutMarkers);
        // A pointer stored to memory escapes.
        let stored = Coro {
            slots: "  %e = alloca i32, align 4\n  %q = alloca ptr, align 8",
            before: "  store ptr %e, ptr %q",
            ..Coro::default()
        }
        .text(d);
        assert_eq!(analysis(&stored).classes[0], SlotClass::EscapedWithoutMarkers);
    }
}

// ---- aliases ----

#[test]
fn aliases_are_followed() {
    for d in DIALECTS {
        // Stored through one alias, loaded through another.
        let text = Coro {
            slots: "  %x = alloca [2 x i32], align 4",
            before: "  %g = getelementptr i32, ptr %x, i64 1\n  store i32 1, ptr %g",
            after: "  %h = getelementptr i8, ptr %x, i64 4\n  %c = addrspacecast ptr %h to ptr addrspace(1)\n  %v = load i32, ptr addrspace(1) %c\n  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        assert_eq!(a.classes[0], SlotClass::Unescaped);
        assert!(a.plan.heap_slots.contains(slot(0)));
        // Escaping through an alias escapes the slot.
        let text = Coro {
            slots: "  %x = alloca [2 x i32], align 4",
            before: "  %g = getelementptr i32, ptr %x, i64 1\n  call void @use(ptr %g)",
            ..Coro::default()
        }
        .text(d);
        assert_eq!(analysis(&text).classes[0], SlotClass::EscapedWithoutMarkers);
        // `ptrtoint` publishes the address.
        let text = Coro {
            slots: "  %x = alloca i32, align 4",
            before: "  %i = ptrtoint ptr %x to i64",
            ..Coro::default()
        }
        .text(d);
        assert_eq!(analysis(&text).classes[0], SlotClass::EscapedWithoutMarkers);
    }
}

#[test]
fn an_alias_live_across_a_suspend_is_a_heap_value_and_escapes_its_slot() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %x = alloca [2 x i32], align 4",
            before: "  %g = getelementptr i32, ptr %x, i64 1\n  store i32 1, ptr %g",
            after: "  store i32 2, ptr %g",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        let g = find(&text, |k| matches!(k, InstKind::PtrAdd(_)));
        assert!(a.plan.heap_values.contains(g));
        assert_eq!(a.classes[0], SlotClass::EscapedWithoutMarkers);
        assert!(a.plan.heap_slots.contains(slot(0)));
    }
}

// ---- SSA values ----

#[test]
fn values_live_in_on_a_resumption_edge_are_heap_values() {
    for d in DIALECTS {
        let crossing = Coro {
            before: "  %v = call i32 @get()",
            after: "  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&crossing);
        let v = is_call_to(&crossing, "get");
        assert!(a.plan.heap_values.contains(v));
        assert_eq!(a.plan.heap_values.count(), 1, "tokens and the frame are not heap values");
        assert!(a.plan.begin_value_copies.is_empty());

        let local =
            Coro { after: "  %v = call i32 @get()\n  call void @sink(i32 %v)", ..Coro::default() }
                .text(d);
        assert!(analysis(&local).plan.heap_values.is_empty());
    }
}

#[test]
fn a_phi_incoming_on_a_resumption_edge_is_a_heap_value() {
    for d in DIALECTS {
        let text = Coro {
            before: "  %v = call i32 @get()",
            after: "  br label %join\njoin:\n  %w = phi i32 [ 7, %resume ]\n  call void @sink(i32 %w)",
            ..Coro::default()
        }
        .text(d)
        .replace(
            "resume:\n",
            "resume:\n  %ph = phi i32 [ %v, %entry ]\n  call void @sink(i32 %ph)\n",
        );
        let a = analysis(&text);
        assert!(a.plan.heap_values.contains(is_call_to(&text, "get")));
    }
}

// ---- before coro.begin ----

#[test]
fn a_heap_slot_used_before_begin_is_copied() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %x = alloca i32, align 4\n  %y = alloca i32, align 4",
            pre: "  store i32 5, ptr %x\n  store i32 6, ptr %y",
            after: "  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        let plan = planned(&text);
        assert!(plan.heap_slots.contains(slot(0)) && !plan.heap_slots.contains(slot(1)));
        assert_eq!(plan.begin_copies, [slot(0)]);
    }
}

/// kiln's item 1: `this` is loaded before `coro.id` and used after a
/// `co_await`: a heap value, stored to its home right after `coro.begin`.
#[test]
fn a_member_function_coroutine_uses_this_after_a_suspend() {
    for d in DIALECTS {
        let text = Coro {
            signature: "void @f(ptr %this)",
            slots: "  %this.addr = alloca ptr, align 8",
            pre: "  store ptr %this, ptr %this.addr\n  %t = load ptr, ptr %this.addr",
            after: "  call void @use(ptr %t)",
            ..Coro::default()
        }
        .text(d);
        let plan = planned(&text);
        let t = find(&text, |k| matches!(k, InstKind::Load { .. }));
        assert!(plan.heap_values.contains(t));
        assert_eq!(plan.begin_value_copies, [t]);
        assert!(plan.heap_slots.is_empty(), "this.addr is read only before begin");
    }
}

/// kiln's item 2: every parameter is copied right after `coro.begin`; the
/// parameter alloca is read by the allocation and by that copy only.
#[test]
fn parameter_copies_keep_the_parameter_alloca_on_the_stack() {
    for d in DIALECTS {
        let text = Coro {
            signature: "void @f(i64 %n)",
            slots: "  %n.addr = alloca i64, align 8\n  %n.copy = alloca i64, align 8",
            pre: "  store i64 %n, ptr %n.addr\n  %sz = load i64, ptr %n.addr",
            before: "  call void @llvm.memcpy.p0.p0.i64(ptr %n.copy, ptr %n.addr, i64 8, i1 false)",
            after: "  %v = load i64, ptr %n.copy\n  %t = trunc i64 %v to i32\n  call void @sink(i32 %t)",
            ..Coro::default()
        }
        .text(d);
        let plan = planned(&text);
        assert!(!plan.heap_slots.contains(slot(0)), "the parameter alloca stays");
        assert!(plan.heap_slots.contains(slot(1)), "its copy is in the frame");
        assert!(plan.begin_copies.is_empty());
    }
}

/// kiln's item 2 (c): a callee-destroyed parameter's destructor runs in
/// the ramp after `coro.end`. Without `!coro.outside.frame` the alloca is
/// rule 5 (heap), and that use is in the suspended region: refused.
#[test]
fn a_callee_destroyed_parameter_without_outside_frame_is_refused() {
    for d in DIALECTS {
        let mut c = Coro {
            signature: "void @f(i64 %n)",
            slots: "  %n.addr = alloca i64, align 8",
            pre: "  store i64 %n, ptr %n.addr",
            ramp: "  call void @dtor(ptr %n.addr)",
            ..Coro::default()
        };
        let a = analysis(&c.text(d));
        assert_eq!(a.classes[0], SlotClass::EscapedWithoutMarkers);
        assert!(
            a.violations.iter().any(|v| v.slot == Some(slot(0)) && v.region == Region::Suspended)
        );
        assert_eq!(refusal(&c.text(d)), FRAME_ACCESS_AFTER_SUSPEND);
        // Once kiln marks it (its WP-1374), it stays on the stack.
        c.slots = "  %n.addr = alloca i64, align 8, !coro.outside.frame !0";
        let plan = planned(&c.text(d));
        assert!(plan.heap_slots.is_empty());
    }
}

#[test]
fn an_alias_computed_before_begin_and_used_after_is_refused() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %x = alloca [2 x i32], align 4",
            pre: "  %g = getelementptr i32, ptr %x, i64 1\n  store i32 1, ptr %g",
            before: "  store i32 2, ptr %g",
            after: "  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        assert_eq!(refusal(&text), FRAME_BEFORE_BEGIN);
    }
}

// ---- arguments ----

#[test]
fn an_argument_used_after_a_resumption_edge_is_refused() {
    for d in DIALECTS {
        let text = Coro { after: "  call void @use(ptr %arg)", ..Coro::default() }.text(d);
        assert_eq!(refusal(&text), ARGUMENT_ACROSS_SUSPEND);
    }
}

/// kiln's item 3: the `sret` pointer and an indirect callee-destroyed
/// parameter are used after a suspend only in the ramp after `coro.end`.
#[test]
fn an_argument_used_in_the_ramp_after_coro_end_is_not_refused() {
    for d in DIALECTS {
        let text = Coro {
            signature: "void @f(ptr sret(i32) %arg, ptr %p)",
            ramp: "  store i32 0, ptr %arg\n  call void @dtor(ptr %p)",
            ..Coro::default()
        }
        .text(d);
        planned(&text);
        // After an unwinding `coro.end`, on the ramp-only side.
        let text = Coro {
            signature: "void @f(ptr sret(i32) %arg, ptr %p)",
            ..Coro::default()
        }
        .text(d)
        .replace(
            "  br label %cleanup\ncleanup:",
            &format!(
                "  br label %cleanup\nunwind:\n  {}\nrampside:\n  call void @dtor(ptr %p)\n  br label %cleanup\ncleanup:",
                d.unwind_end("rampside", "cleanup")
            ),
        );
        let text = text.replace(
            "  br label %cleanup\nunwind:",
            "  br i1 true, label %cleanup, label %unwind\nunwind:",
        );
        let text = with_final(&text);
        let plan = planned(&text);
        assert!(plan.has_unwind_end);
        // The same use on the other side is a use after resumption.
        let flipped = text
            .replace("call void @dtor(ptr %p)", "")
            .replace("cleanup:\n  %fm", "cleanup:\n  call void @use(ptr %p)\n  %fm");
        assert_eq!(refusal(&flipped), ARGUMENT_ACROSS_SUSPEND);
    }
}

/// Makes the template's suspend final (an unwinding `coro.end` needs one).
fn with_final(text: &str) -> String {
    text.replace(
        "@llvm.coro.suspend(token %save, i1 false)",
        "@llvm.coro.suspend(token %save, i1 true)",
    )
}

// ---- the safety conditions ----

#[test]
fn a_heap_slot_used_after_the_wrapper_is_refused() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %x = alloca i32, align 4",
            before: "  store i32 1, ptr %x",
            wrapped: "  store i32 3, ptr %x",
            after: "  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        assert_eq!(a.violations.len(), 1);
        assert_eq!(a.violations[0].region, Region::Suspended);
        assert_eq!(refusal(&text), FRAME_ACCESS_AFTER_SUSPEND);
    }
}

#[test]
fn a_heap_value_used_on_the_minus_one_path_is_refused() {
    for d in DIALECTS {
        let text = Coro {
            before: "  %v = call i32 @get()",
            after: "  call void @sink(i32 %v)",
            ramp: "  call void @sink(i32 %v)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        assert!(a.violations.iter().any(|v| v.value.is_some() && v.region == Region::Suspended));
        assert_eq!(refusal(&text), FRAME_ACCESS_AFTER_SUSPEND);
    }
}

#[test]
fn a_heap_slot_used_after_the_free_is_refused() {
    for d in DIALECTS {
        let text = Coro {
            slots: "  %x = alloca i32, align 4",
            before: "  store i32 1, ptr %x",
            after: "  %v = load i32, ptr %x\n  call void @sink(i32 %v)",
            cleanup: "  %w = load i32, ptr %x\n  call void @sink(i32 %w)",
            ..Coro::default()
        }
        .text(d);
        let a = analysis(&text);
        assert!(a.violations.iter().any(|v| v.region == Region::AfterFree));
        assert_eq!(refusal(&text), FRAME_ACCESS_AFTER_SUSPEND);
    }
}

// ---- the structural refusals ----

#[test]
fn structural_refusals() {
    for d in DIALECTS {
        let base = Coro::default().text(d);
        let cases: Vec<(&str, String)> = vec![
            (VARIADIC, base.replace("void @f(ptr %arg)", "void @f(ptr %arg, ...)")),
            (
                BEGIN_SHAPE,
                base.replace(
                    "  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)",
                    "  %hdl = call ptr @llvm.coro.begin(token %id, ptr %mem)\n  %hdl2 = call ptr @llvm.coro.begin(token %id, ptr %mem)",
                ),
            ),
            (BEGIN_SHAPE, base.replace("@llvm.coro.id(i32 16, ptr null", "@llvm.coro.id(i32 16, ptr %arg")),
            (
                BEGIN_SHAPE,
                Coro { slots: "  %p = alloca [2 x i32], align 4", pre: "  %q = getelementptr i32, ptr %p, i64 1", promise: "%q", ..Coro::default() }.text(d),
            ),
            (
                FRAME_INTRINSIC,
                Coro { before: "  %fr = call ptr @llvm.coro.frame()\n  call void @use(ptr %fr)", ..Coro::default() }.text(d),
            ),
            (
                DYNAMIC_ALLOCA,
                Coro { after: "  %dyn = alloca i32, i64 4, align 4\n  call void @use(ptr %dyn)", ..Coro::default() }.text(d),
            ),
            (
                DYNAMIC_ALLOCA,
                Coro { after: "  %ss = call ptr @llvm.stacksave.p0()\n  call void @llvm.stackrestore.p0(ptr %ss)", ..Coro::default() }.text(d),
            ),
            (SUSPEND_SHAPE, base.replace("    i8 1, label %cleanup\n", "    i8 2, label %cleanup\n")),
            (
                SUSPEND_SHAPE,
                base.replace("  switch i8 %sp,", "  %sp2 = add i8 %sp, 0\n  switch i8 %sp,"),
            ),
            (SUSPEND_SHAPE, base.replace("@llvm.coro.suspend(token %save,", "@llvm.coro.suspend(token none,")),
            (
                UNWIND_END_WITHOUT_FINAL,
                Coro { cleanup: "  br label %unwind\nunwind:", ..Coro::default() }
                    .text(d)
                    .replace("  br label %unwind\nunwind:\n  br label %ret", &format!("  br label %unwind\nunwind:\n  {}\nrampside:\n  br label %ret", d.unwind_end("rampside", "ret"))),
            ),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (name, text) in &cases {
            assert_eq!(refusal(text), *name, "{d:?}\n{text}");
            seen.insert(*name);
        }
        // Two suspends, both final.
        let two_finals = with_final(&Coro::default().text(d)).replace(
            "resume:\n",
            "resume:\n  %save2 = call token @llvm.coro.save(ptr null)\n  call void @llvm.coro.await.suspend.void(ptr null, ptr %hdl, ptr @wrapper)\n  %sp2 = call i8 @llvm.coro.suspend(token %save2, i1 true)\n  switch i8 %sp2, label %ret [\n    i8 0, label %resume2\n    i8 1, label %cleanup\n  ]\nresume2:\n",
        );
        assert_eq!(refusal(&two_finals), SUSPEND_SHAPE);
        // Every name is produced by some test of this file.
        for name in [
            OUTSIDE_FRAME_ACROSS_SUSPEND,
            ARGUMENT_ACROSS_SUSPEND,
            FRAME_BEFORE_BEGIN,
            FRAME_ACCESS_AFTER_SUSPEND,
            OVERALIGNED_FRAME,
        ] {
            seen.insert(name);
        }
        assert_eq!(seen.len(), REFUSALS.len());
    }
}

#[test]
fn suspends_are_numbered_in_order_with_the_final_last() {
    for d in DIALECTS {
        // A final suspend first in layout, then an ordinary one.
        let text = with_final(&Coro::default().text(d)).replace(
            "resume:\n",
            "resume:\n  %save2 = call token @llvm.coro.save(ptr null)\n  %b = call i1 @llvm.coro.await.suspend.bool(ptr null, ptr %hdl, ptr @wrapper)\n  br i1 %b, label %go, label %resume2\ngo:\n  %sp2 = call i8 @llvm.coro.suspend(token %save2, i1 false)\n  switch i8 %sp2, label %ret [\n    i8 0, label %resume2\n    i8 1, label %cleanup\n  ]\nresume2:\n",
        );
        let plan = planned(&text);
        assert_eq!(plan.suspends.len(), 2);
        assert!(!plan.suspends[0].is_final && plan.suspends[1].is_final);
        assert!(plan.suspends[0].suspend > plan.suspends[1].suspend);
        assert_eq!(plan.final_index, Some(1));
        // The `.bool` wrapper's path to its suspend (`go`) is suspended.
        let (_, body) = common::parse_one(&text, "f");
        let go = body.blocks().find(|b| b.insts().any(|i| i.id() == plan.suspends[0].suspend));
        assert!(plan.suspended_region.contains(go.expect("go").id()));
    }
}

/// A coroutine whose resumed side is `n` blocks of five instructions on
/// four slots: the instructions grow with `n`, the tracked objects do not.
fn synthetic(n: usize) -> String {
    let mut after = String::new();
    for i in 0..n {
        let _ = write!(
            after,
            "  %a{i} = load i32, ptr %x\n  %b{i} = add i32 %a{i}, 1\n  store i32 %b{i}, ptr %y\n  call void @sink(i32 %b{i})\n  br label %s{i}\ns{i}:\n"
        );
    }
    Coro {
        slots: "  %x = alloca i32, align 4\n  %y = alloca i32, align 4\n  %z = alloca i32, align 4\n  %w = alloca i32, align 4",
        before: "  store i32 1, ptr %x\n  call void @use(ptr %z)",
        after: &after,
        ..Coro::default()
    }
    .text(Dialect::Llvm22)
}

/// The plan's cost per instruction, the best of `runs`.
#[allow(clippy::cast_precision_loss, reason = "a printed measurement")]
fn ns_per_inst(text: &str, runs: u32) -> (f64, usize) {
    let (module, body) = common::parse_one(text, "f");
    let mut best = f64::INFINITY;
    for _ in 0..runs {
        let start = std::time::Instant::now();
        let plan = coro::plan(&module, &body);
        let elapsed = start.elapsed().as_secs_f64() * 1e9;
        assert!(plan.is_ok());
        best = best.min(elapsed);
    }
    (best / body.inst_count() as f64, body.inst_count())
}

/// The throughput and the linearity of `plan` (`spec/xform.md` §5); run
/// by hand, `--release -- --ignored --nocapture`. It asserts only that
/// the plan exists: wall time is no test result.
#[test]
#[ignore = "a measurement, not a test"]
#[allow(clippy::cast_precision_loss, reason = "a printed measurement")]
fn throughput() {
    let n = 20_000;
    let (one, insts) = ns_per_inst(&synthetic(n), 9);
    let (two, insts2) = ns_per_inst(&synthetic(2 * n), 9);
    println!(
        "synthetic: {insts} instructions {one:.1} ns/inst, {insts2} instructions {two:.1} ns/inst; 2N/N time ratio {:.2}",
        two * insts2 as f64 / (one * insts as f64)
    );
    let heap = planned(&synthetic(4)).heap_slots;
    assert!(heap.contains(slot(0)) && heap.contains(slot(2)) && !heap.contains(slot(1)));
}
