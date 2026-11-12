//! What `-O0`'s always-inliner would have done, for a module that never
//! met it (`spec/cli.md` "Always-inline", design 0001 point 6, WP-021).
//!
//! A front end's module before LLVM's `-O0` pipeline still has its
//! `alwaysinline` calls. hod does not inline. Out of line, a call means
//! what the inlined body means, except in three cases this module
//! settles:
//!
//! - An `available_externally` function is a definition for an
//!   optimizer's eyes: the object does not contain it. One that is not
//!   `alwaysinline` is discarded (its body is verified, never compiled)
//!   and referenced as an external symbol, as `llc` does. One that is
//!   `alwaysinline` and called directly by a body this object contains
//!   (transitively: a copy's calls count) is compiled as a local copy;
//!   module emission sends the direct calls to the copy and every other
//!   reference to the external symbol (`spec/codegen.md` §9.7.2). Its
//!   body is judged like any other only then: one nothing calls is never
//!   compiled, so it cannot decline the module.
//! - A body whose meaning depends on being inlined (its frame, its return
//!   address, a stack pointer or a local address that outlives it, a
//!   `returns_twice` call) declines the module when a direct call to it
//!   may have been inlined: [`reason`].
//! - Everything else (`llvm.is.constant`, `llvm.objectsize`, `musttail`,
//!   dynamic `alloca`, variadic bodies, ...) is correct out of line; the
//!   reasons are in the spec.
//!
//! The work is one scan of a body's call sites and returns when it is
//! accepted, and one pass over every accepted body's call sites at the
//! end, only for a module that holds a copy candidate or a body of
//! [`reason`].

use hod_ir::{
    Body, Callee, FnAttrs, FnAttrsId, InstKind, Linkage, Module, SymId, SymbolKind, Value,
    ValueKind,
};

use crate::front::symbol_name;
use crate::{CompileError, Construct, Unsupported};

/// The stable names of the bodies hod declines because their meaning
/// depends on being inlined (`spec/cli.md` "Always-inline"). Each is
/// reported in the function whose body has it.
pub mod reason {
    /// `llvm.frameaddress`, `llvm.sponentry`, `llvm.localaddress`,
    /// `llvm.eh.dwarf.cfa`: inlined, the frame is the caller's.
    pub const FRAME_ADDRESS: &str = "alwaysinline:frame-address";
    /// `llvm.returnaddress`, `llvm.addressofreturnaddress`: inlined, the
    /// return address is the caller's.
    pub const RETURN_ADDRESS: &str = "alwaysinline:return-address";
    /// `llvm.stacksave` whose result is used other than by
    /// `llvm.stackrestore` in the same body: inlined, it is the caller's
    /// stack pointer.
    pub const STACKSAVE: &str = "alwaysinline:stacksave-escapes";
    /// A call to a `returns_twice` function (`setjmp`, `vfork`): out of
    /// line, the frame it saves is dead when the caller jumps back.
    pub const RETURNS_TWICE: &str = "alwaysinline:returns-twice";
    /// A return of the address of a stack slot (an `alloca` of the entry
    /// block): inlined, `llc -O0` keeps the slot alive for the whole
    /// caller.
    pub const LOCAL_ADDRESS: &str = "alwaysinline:returns-local-address";
    /// Inline assembly that names a frame register (`sp`, `x29`, `fp`,
    /// `x30`, `lr`, ...): inlined, they are the caller's.
    pub const FRAME_REGISTER_ASM: &str = "alwaysinline:frame-register-asm";

    /// Every reason, in the order a body is searched for them.
    pub const ALL: [&str; 6] = [
        FRAME_ADDRESS,
        RETURN_ADDRESS,
        STACKSAVE,
        RETURNS_TWICE,
        LOCAL_ADDRESS,
        FRAME_REGISTER_ASM,
    ];
}

/// The session's part (`Session::inlining`).
#[derive(Debug, Default)]
pub(crate) struct Inlining {
    /// The bodies of `available_externally` `alwaysinline` functions, in
    /// arrival order, until [`Inlining::conclude`] decides which are
    /// copied.
    held: Vec<Body>,
    /// The accepted bodies whose meaning depends on being inlined, with
    /// the reason.
    sensitive: Vec<(SymId, &'static str)>,
}

/// The function attributes of `sym`, if it is a function.
fn fn_attrs(module: &Module, sym: SymId) -> Option<FnAttrsId> {
    match module.symbol(sym).kind() {
        SymbolKind::Function { attrs, .. } => Some(attrs),
        _ => None,
    }
}

fn has_flag(module: &Module, sym: SymId, flag: u32) -> bool {
    fn_attrs(module, sym).is_some_and(|attrs| module.fn_attrs(attrs).has(flag))
}

/// Whether `sym` is a function `llc` would not emit: `available_externally`.
fn is_available_externally(module: &Module, sym: SymId) -> bool {
    module.symbol(sym).linkage() == Linkage::AvailableExternally && fn_attrs(module, sym).is_some()
}

/// Whether any attribute set of `module` holds `alwaysinline`: a function's
/// or a call site's (the readers keep a call site's set in the same
/// table, but not which call uses it). Without one, nothing in the
/// module was ever inlined at `-O0`.
fn may_have_inlined(module: &Module) -> bool {
    (0..module.fn_attrs_count())
        .any(|index| module.fn_attrs(FnAttrsId::new(index as u32)).has(FnAttrs::ALWAYS_INLINE))
}

/// The direct callee of a call site, if it is a symbol.
fn direct_callee(callee: Callee<'_>) -> Option<SymId> {
    match callee {
        Callee::Value(value) => match value.kind() {
            ValueKind::Sym(sym) => Some(sym),
            _ => None,
        },
        Callee::Intrinsic(_) => None,
    }
}

/// Whether `word` names an AArch64 or x86-64 frame register.
fn is_frame_register(word: &[u8]) -> bool {
    matches!(
        word,
        b"sp"
            | b"wsp"
            | b"fp"
            | b"lr"
            | b"x29"
            | b"w29"
            | b"x30"
            | b"w30"
            | b"rsp"
            | b"esp"
            | b"rbp"
            | b"ebp"
    )
}

/// Whether inline assembly text names a frame register, as a word.
fn names_frame_register(text: &[u8]) -> bool {
    text.split(|&byte| !byte.is_ascii_alphanumeric()).any(is_frame_register)
}

/// Whether `value` is the address of one of the body's stack slots, or
/// derived from one by `ptradd`, `select` or `phi` (a few levels deep).
fn is_slot_address(body: &Body, value: Value, depth: u32) -> bool {
    match value.kind() {
        ValueKind::Slot(_) => true,
        ValueKind::Inst(inst) if depth < 8 => match body.inst(inst).kind() {
            InstKind::PtrAdd(add) => is_slot_address(body, add.base, depth + 1),
            InstKind::Select { if_true, if_false, .. } => {
                is_slot_address(body, if_true, depth + 1)
                    || is_slot_address(body, if_false, depth + 1)
            }
            InstKind::Phi(phi) => {
                phi.values().iter().any(|&incoming| is_slot_address(body, incoming, depth + 1))
            }
            _ => false,
        },
        _ => false,
    }
}

/// Whether the result of the `llvm.stacksave` call `saved` is used by
/// anything but `llvm.stackrestore`.
fn stacksave_escapes(body: &Body, saved: Value) -> bool {
    body.insts().any(|inst| {
        inst.operands().contains(&saved)
            && !inst.call().is_some_and(|site| {
                site.intrinsic().is_some_and(|i| i.name() == "llvm.stackrestore")
            })
    })
}

/// The first reason `body`'s meaning depends on being inlined, if any.
pub(crate) fn sensitivity(module: &Module, body: &Body) -> Option<&'static str> {
    for site in body.calls() {
        if site.returns_twice() {
            return Some(reason::RETURNS_TWICE);
        }
        if let Some(intrinsic) = site.intrinsic() {
            match intrinsic.name() {
                "llvm.frameaddress" | "llvm.sponentry" | "llvm.localaddress"
                | "llvm.eh.dwarf.cfa" => return Some(reason::FRAME_ADDRESS),
                "llvm.returnaddress" | "llvm.addressofreturnaddress" => {
                    return Some(reason::RETURN_ADDRESS);
                }
                "llvm.stacksave" if stacksave_escapes(body, body.inst(site.inst()).result()) => {
                    return Some(reason::STACKSAVE);
                }
                _ => {}
            }
            continue;
        }
        match site.callee() {
            Callee::Value(value) => match value.kind() {
                ValueKind::Sym(sym)
                    if sym.index() < module.symbol_count()
                        && has_flag(module, sym, FnAttrs::RETURNS_TWICE) =>
                {
                    return Some(reason::RETURNS_TWICE);
                }
                ValueKind::Asm(asm) => {
                    let asm = module.asm(asm);
                    if names_frame_register(asm.template) || names_frame_register(asm.constraints) {
                        return Some(reason::FRAME_REGISTER_ASM);
                    }
                }
                _ => {}
            },
            Callee::Intrinsic(_) => {}
        }
    }
    if !body.slots().is_empty() {
        for block in body.blocks() {
            if let InstKind::Ret { value: Some(value) } = block.terminator().kind()
                && is_slot_address(body, value, 0)
            {
                return Some(reason::LOCAL_ADDRESS);
            }
        }
    }
    None
}

fn decline(module: &Module, sym: SymId, why: &'static str) -> CompileError {
    Unsupported::in_function(Construct::Named(why.into()), symbol_name(module, sym)).into()
}

impl Inlining {
    /// A body as it arrives, before it is judged: the body to judge now,
    /// or `None` when it is an `available_externally` function's, which
    /// is verified and then held (`alwaysinline`) or dropped.
    ///
    /// # Errors
    /// `verify`'s refusal of an `available_externally` body (LLVM's
    /// verifier sees it too).
    pub(crate) fn divert(
        &mut self,
        module: &Module,
        body: Body,
    ) -> Result<Option<Body>, CompileError> {
        let sym = body.symbol();
        if !is_available_externally(module, sym) {
            return Ok(Some(body));
        }
        if let Err(error) = body.verify(module) {
            let function = symbol_name(module, sym).unwrap_or_default();
            return Err(CompileError::MalformedInput(format!("function '{function}': {error}")));
        }
        if has_flag(module, sym, FnAttrs::ALWAYS_INLINE) {
            self.held.push(body);
        }
        Ok(None)
    }

    /// Notes an accepted body whose meaning depends on being inlined.
    pub(crate) fn note(&mut self, module: &Module, body: &Body) {
        if let Some(why) = sensitivity(module, body) {
            self.sensitive.push((body.symbol(), why));
        }
    }

    /// The end of the module: the copies to compile (each still to be
    /// judged), or the decline of a body whose meaning depends on being
    /// inlined and that a direct call may have inlined.
    ///
    /// # Errors
    /// The decline, named in the called function.
    pub(crate) fn conclude(
        &mut self,
        module: &Module,
        bodies: &[Body],
    ) -> Result<Vec<Body>, CompileError> {
        if self.held.is_empty() && self.sensitive.is_empty() {
            return Ok(Vec::new());
        }
        let watch = !self.sensitive.is_empty() && may_have_inlined(module);
        if !watch && self.held.is_empty() {
            return Ok(Vec::new());
        }
        self.sensitive.sort_unstable_by_key(|&(sym, _)| sym.index());
        self.held.sort_unstable_by_key(|body| body.symbol().index());
        let mut reached = vec![false; self.held.len()];
        let mut work: Vec<usize> = Vec::new();
        let mut visit = |body: &Body, work: &mut Vec<usize>| -> Result<(), CompileError> {
            for site in body.calls() {
                let Some(callee) = direct_callee(site.callee()) else { continue };
                if watch
                    && let Ok(at) =
                        self.sensitive.binary_search_by_key(&callee.index(), |&(s, _)| s.index())
                {
                    return Err(decline(module, callee, self.sensitive[at].1));
                }
                if let Ok(at) =
                    self.held.binary_search_by_key(&callee.index(), |b| b.symbol().index())
                    && !reached[at]
                {
                    reached[at] = true;
                    // A copy stands for an inlined body: the module was
                    // inlined into, whatever `watch` says.
                    if let Some(why) = sensitivity(module, &self.held[at]) {
                        return Err(decline(module, callee, why));
                    }
                    work.push(at);
                }
            }
            Ok(())
        };
        for body in bodies {
            visit(body, &mut work)?;
        }
        while let Some(at) = work.pop() {
            visit(&self.held[at], &mut work)?;
        }
        let held = std::mem::take(&mut self.held);
        Ok(held
            .into_iter()
            .zip(reached)
            .filter_map(|(body, reached)| reached.then_some(body))
            .collect())
    }
}
