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
//!   address, a stack pointer or a local address that outlives it)
//!   declines the module when a direct call to it is one LLVM's
//!   always-inliner inlines ([`inlined`], WP-188): [`reason`]. Except
//!   (WP-197) a local body whose only such constructs are
//!   `llvm.returnaddress` and `llvm.frameaddress` and that runs only
//!   where LLVM inlines it, always at the same number `b` of frames
//!   below the frame LLVM's inlined code runs in: the depth of each is
//!   raised by `b`, which names the same frame, since every hod function
//!   has its frame record ([`frame_bias`]).
//! - Everything else (`llvm.is.constant`, `llvm.objectsize`, `musttail`,
//!   dynamic `alloca`, and every body LLVM does not inline: variadic,
//!   recursive, a `call` that may return twice in a body that is not
//!   `returns_twice`, `indirectbr`, `llvm.localescape`, `llvm.va_start`)
//!   is correct out of line; the reasons are in the spec.
//!
//! The work is one scan of a body's call sites and returns when it is
//! accepted, and one pass over every accepted body's call sites at the
//! end, only for a module that holds a copy candidate or a body of
//! [`reason`].

use hod_ir::{
    Body, CallSite, Callee, Const, FnAttrs, FnAttrsId, InstKind, Linkage, Module, Opcode, SymId,
    SymbolKind, TailKind, TypeKind, Value, ValueKind,
};

use crate::front::symbol_name;
use crate::{CompileError, Construct, Unsupported};

/// The stable names of the bodies hod declines because their meaning
/// depends on being inlined (`spec/cli.md` "Always-inline"). Each is
/// reported in the function whose body has it.
pub mod reason {
    /// `llvm.frameaddress`, `llvm.sponentry`, `llvm.localaddress`,
    /// `llvm.eh.dwarf.cfa`, a named-register intrinsic of a frame
    /// register: inlined, the frame is the caller's.
    pub const FRAME_ADDRESS: &str = "alwaysinline:frame-address";
    /// `llvm.returnaddress`, `llvm.addressofreturnaddress`: inlined, the
    /// return address is the caller's.
    pub const RETURN_ADDRESS: &str = "alwaysinline:return-address";
    /// `llvm.stacksave` whose result is used other than by
    /// `llvm.stackrestore` in the same body: inlined, it is the caller's
    /// stack pointer.
    pub const STACKSAVE: &str = "alwaysinline:stacksave-escapes";
    /// A call that may return twice (`setjmp`, `vfork`) in a body LLVM
    /// inlines: an `invoke` of it, or a `call` in a `returns_twice` body
    /// (WP-197). A `call` in any other body keeps the body out of line,
    /// as `opt` keeps it (WP-188).
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
    sensitive: Vec<(SymId, Sensitive)>,
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
pub(crate) fn is_available_externally(module: &Module, sym: SymId) -> bool {
    module.symbol(sym).linkage() == Linkage::AvailableExternally && fn_attrs(module, sym).is_some()
}

/// Whether `opt -passes=always-inline` (LLVM 19 and 22) inlines the
/// direct call `site` of `callee` (WP-188, `spec/cli.md`
/// "Always-inline"): the callee or the call site is `alwaysinline`, the
/// call site is not `noinline`. Whether the callee's body can be inlined
/// at all is [`inline_viable`].
fn inlined(module: &Module, site: &CallSite<'_>, callee: SymId) -> bool {
    !site.is_noinline()
        && (site.is_always_inline() || has_flag(module, callee, FnAttrs::ALWAYS_INLINE))
}

/// Whether LLVM's always-inliner can inline a body at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Viable {
    Yes,
    No,
    /// A block of the body has its address taken: LLVM inlines it only
    /// when every use is a `callbr`, which hod does not tell apart.
    Unsure,
}

/// Whether the call `site` may return twice (LLVM's `canReturnTwice`):
/// `returns_twice` on the call or on its direct callee.
fn returns_twice(module: &Module, site: &CallSite<'_>) -> bool {
    site.returns_twice()
        || direct_callee(site.callee()).is_some_and(|callee| {
            callee.index() < module.symbol_count()
                && has_flag(module, callee, FnAttrs::RETURNS_TWICE)
        })
}

/// Whether LLVM's always-inliner can inline `body` at all (its
/// `isInlineViable` and the always-inliner's own checks, measured with
/// `opt` 19 and 22): not variadic, not a pre-split coroutine, no call to
/// itself, no `indirectbr`, no `llvm.localescape` or `llvm.va_start`, and
/// no `call` (not `invoke`) that may return twice unless the function is
/// `returns_twice` itself (WP-197: `opt` inlines an `invoke` of `setjmp`,
/// and a `returns_twice` body's `setjmp`). A block address of the body
/// leaves it [`Viable::Unsure`].
fn inline_viable(module: &Module, body: &Body) -> Viable {
    let sym = body.symbol();
    if matches!(module.types.kind(body.fn_type()), TypeKind::Function { variadic: true, .. })
        || has_flag(module, sym, FnAttrs::PRESPLIT_COROUTINE)
    {
        return Viable::No;
    }
    if body.blocks().any(|block| block.terminator().opcode() == Opcode::IndirectBr) {
        return Viable::No;
    }
    let twice_ok = has_flag(module, sym, FnAttrs::RETURNS_TWICE);
    let refused = body.calls().any(|site| {
        (!twice_ok && site.opcode() == Opcode::Call && returns_twice(module, &site))
            || match site.callee() {
                Callee::Intrinsic(intrinsic) => {
                    matches!(intrinsic.name(), "llvm.localescape" | "llvm.va_start")
                }
                Callee::Value(value) => {
                    matches!(value.kind(), ValueKind::Sym(callee) if callee == sym)
                }
            }
    });
    if refused {
        Viable::No
    } else if module.consts.has_address_taken_block(sym) {
        Viable::Unsure
    } else {
        Viable::Yes
    }
}

/// Whether LLVM certainly inlines the call `site` in `caller` of the body
/// `callee`, which [`inlined`] and [`inline_viable`] accept: no reason of
/// `InlineFunction`'s to refuse (a `callbr`, an operand bundle, a call
/// made with another function type, another personality) and none of
/// hod's to doubt the frame the inlined code runs in (a `musttail` call,
/// a coroutine caller).
fn surely_inlined(module: &Module, caller: &Body, site: &CallSite<'_>, callee: &Body) -> bool {
    let personality = |sym: SymId| match module.symbol(sym).kind() {
        SymbolKind::Function { personality, .. } => personality,
        _ => None,
    };
    let (theirs, ours) = (personality(callee.symbol()), personality(caller.symbol()));
    site.opcode() != Opcode::CallBr
        && site.bundle_count() == 0
        && site.fn_type() == callee.fn_type()
        && site.tail() != TailKind::MustTail
        && !has_flag(module, caller.symbol(), FnAttrs::PRESPLIT_COROUTINE)
        && (theirs.is_none() || ours.is_none() || theirs == ours)
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
    scan(module, body, false)
}

/// [`sensitivity`], passing over `llvm.returnaddress` and
/// `llvm.frameaddress` when `depth_ok` (they can be given a deeper frame).
fn scan(module: &Module, body: &Body, depth_ok: bool) -> Option<&'static str> {
    for site in body.calls() {
        if returns_twice(module, &site) {
            return Some(reason::RETURNS_TWICE);
        }
        if let Some(intrinsic) = site.intrinsic() {
            match intrinsic.name() {
                "llvm.frameaddress" if depth_ok => {}
                "llvm.returnaddress" if depth_ok => {}
                "llvm.frameaddress" | "llvm.sponentry" | "llvm.localaddress"
                | "llvm.eh.dwarf.cfa" => return Some(reason::FRAME_ADDRESS),
                "llvm.returnaddress" | "llvm.addressofreturnaddress" => {
                    return Some(reason::RETURN_ADDRESS);
                }
                // WP-064: a named register that is a frame register
                // (`sp`, `x29`, `x30`) is the caller's once inlined.
                "llvm.read_register" | "llvm.read_volatile_register" | "llvm.write_register"
                    if site.args().first().and_then(|arg| arg.as_meta()).is_some_and(|meta| {
                        body.metadata(meta).is_none_or(|meta| {
                            meta.strings().any(|name| names_frame_register(name.as_bytes()))
                        })
                    }) =>
                {
                    return Some(reason::FRAME_ADDRESS);
                }
                "llvm.stacksave" if stacksave_escapes(body, body.inst(site.inst()).result()) => {
                    return Some(reason::STACKSAVE);
                }
                _ => {}
            }
            continue;
        }
        if let Callee::Value(value) = site.callee()
            && let ValueKind::Asm(asm) = value.kind()
        {
            let asm = module.asm(asm);
            if names_frame_register(asm.template) || names_frame_register(asm.constraints) {
                return Some(reason::FRAME_REGISTER_ASM);
            }
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

/// Why a body that LLVM can inline declines a call LLVM inlines.
#[derive(Clone, Copy, Debug)]
struct Sensitive {
    /// The reason ([`sensitivity`]).
    why: &'static str,
    /// Whether its only constructs of a reason are `llvm.returnaddress`
    /// and `llvm.frameaddress`, so a frame bias can give it the inlined
    /// meaning ([`frame_bias`]).
    biasable: bool,
}

/// The reason `body` declines a call LLVM inlines: its [`sensitivity`],
/// when LLVM may inline it at all ([`inline_viable`]).
fn inline_sensitivity(module: &Module, body: &Body) -> Option<Sensitive> {
    let why = sensitivity(module, body)?;
    match inline_viable(module, body) {
        Viable::No => None,
        Viable::Unsure => Some(Sensitive { why, biasable: false }),
        Viable::Yes => Some(Sensitive { why, biasable: scan(module, body, true).is_none() }),
    }
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
        if let Some(sensitive) = inline_sensitivity(module, body) {
            self.sensitive.push((body.symbol(), sensitive));
        }
    }

    /// The end of the module: the copies to compile (each still to be
    /// judged), or the decline of a body whose meaning depends on being
    /// inlined and that a direct call may have inlined. A body a frame
    /// bias gives its inlined meaning ([`frame_bias`]) is rewritten in
    /// place, in `bodies` or in a copy.
    ///
    /// # Errors
    /// The decline, named in the called function.
    pub(crate) fn conclude(
        &mut self,
        module: &mut Module,
        bodies: &mut [Body],
    ) -> Result<Vec<Body>, CompileError> {
        if self.held.is_empty() && self.sensitive.is_empty() {
            return Ok(Vec::new());
        }
        self.sensitive.sort_unstable_by_key(|&(sym, _)| sym.index());
        self.held.sort_unstable_by_key(|body| body.symbol().index());
        let mut reached = vec![false; self.held.len()];
        // Per held copy: its reason, judged once, at its first inlined
        // call.
        let mut judged: Vec<Option<Option<Sensitive>>> = vec![None; self.held.len()];
        // The bodies a frame bias may give their inlined meaning.
        let mut biased: Vec<(At, &'static str)> = Vec::new();
        let mut work: Vec<usize> = Vec::new();
        {
            let module = &*module;
            let mut visit = |body: &Body,
                             work: &mut Vec<usize>,
                             biased: &mut Vec<(At, &'static str)>|
             -> Result<(), CompileError> {
                for site in body.calls() {
                    let Some(callee) = direct_callee(site.callee()) else { continue };
                    let inlined = inlined(module, &site, callee);
                    if inlined
                        && let Ok(at) = self
                            .sensitive
                            .binary_search_by_key(&callee.index(), |&(s, _)| s.index())
                    {
                        let sensitive = self.sensitive[at].1;
                        if !sensitive.biasable {
                            return Err(decline(module, callee, sensitive.why));
                        }
                        biased.push((At::Accepted(callee), sensitive.why));
                    }
                    if let Ok(at) =
                        self.held.binary_search_by_key(&callee.index(), |b| b.symbol().index())
                    {
                        // A copy stands for an inlined body where the call
                        // is inlined; a kept call (`noinline` site, a body
                        // LLVM cannot inline) means it out of line.
                        if inlined
                            && let Some(sensitive) = *judged[at]
                                .get_or_insert_with(|| inline_sensitivity(module, &self.held[at]))
                        {
                            if !sensitive.biasable {
                                return Err(decline(module, callee, sensitive.why));
                            }
                            biased.push((At::Held(at), sensitive.why));
                        }
                        if !reached[at] {
                            reached[at] = true;
                            work.push(at);
                        }
                    }
                }
                Ok(())
            };
            for body in bodies.iter() {
                visit(body, &mut work, &mut biased)?;
            }
            while let Some(at) = work.pop() {
                visit(&self.held[at], &mut work, &mut biased)?;
            }
        }
        if !biased.is_empty() {
            biased.sort_unstable_by_key(|&(at, _)| at);
            biased.dedup_by_key(|&mut (at, _)| at);
            let biases = frame_bias(module, bodies, &self.held, &reached, &biased)?;
            for (at, bias) in biases {
                let body = match at {
                    At::Accepted(sym) => {
                        let Some(body) = bodies.iter_mut().find(|body| body.symbol() == sym) else {
                            continue;
                        };
                        body
                    }
                    At::Held(at) => &mut self.held[at],
                };
                deepen(module, body, bias)?;
            }
        }
        let held = std::mem::take(&mut self.held);
        Ok(held
            .into_iter()
            .zip(reached)
            .filter_map(|(body, reached)| reached.then_some(body))
            .collect())
    }
}

/// A body of [`frame_bias`]: an accepted one by its symbol, or a held
/// `available_externally` copy by its position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum At {
    Accepted(SymId),
    Held(usize),
}

/// The deepest frame bias [`frame_bias`] follows; a chain of inlined
/// calls longer than this (or a cycle of them) is not given one.
const MAX_BIAS: u32 = 31;

/// The frames a body may run in, relative to the frame LLVM's inlined
/// code would run in: bit `b` of `bits` is "`b` frames below it" (0: its
/// own frame, the body runs out of line there too); `unsure` when hod
/// cannot tell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Biases {
    bits: u32,
    unsure: bool,
}

/// The symbols a reference other than the callee of a direct call names
/// (an operand of code, a constant, a structor, a personality, a
/// retained symbol): such a body may run out of line.
fn escaping(module: &Module, bodies: &[Body], held: &[Body], reached: &[bool]) -> Vec<bool> {
    let mut escapes = vec![false; module.symbol_count()];
    let mut mark = |sym: SymId| {
        if let Some(escape) = escapes.get_mut(sym.index()) {
            *escape = true;
        }
    };
    let live_held = held.iter().zip(reached).filter_map(|(body, &reached)| reached.then_some(body));
    for body in bodies.iter().chain(live_held) {
        for inst in body.insts() {
            let operands = inst.operands();
            let operands =
                if inst.call().is_some() { operands.get(1..).unwrap_or(&[]) } else { operands };
            for value in operands {
                if let ValueKind::Sym(sym) = value.kind() {
                    mark(sym);
                }
            }
        }
    }
    for id in 0..module.consts.len() {
        match module.consts.get(hod_ir::ConstId::new(id as u32)) {
            Const::Addr { sym, .. } | Const::DsoLocalEquivalent(sym) | Const::NoCfi(sym) => {
                mark(sym)
            }
            Const::SymDiff { a, b, .. } => {
                mark(a);
                mark(b);
            }
            _ => {}
        }
    }
    for structor in module.ctors().iter().chain(module.dtors()) {
        mark(structor.func);
    }
    for (index, symbol) in module.symbols().enumerate() {
        if symbol.is_retained() {
            mark(SymId::new(index as u32));
        }
        if let SymbolKind::Function { personality: Some(personality), .. } = symbol.kind() {
            mark(personality);
        }
    }
    escapes
}

/// WP-197 (`spec/cli.md` "Always-inline"): the frame bias of each body of
/// `biased`, or the decline of one that has none.
///
/// Inlined, `llvm.returnaddress(n)` and `llvm.frameaddress(n)` speak of
/// the frame the inlined code runs in. Out of line, a body that LLVM
/// would have inlined runs one frame below its caller; through a chain
/// of such calls, `b` frames below the frame LLVM's code runs in. Every
/// hod function has a frame record (`spec/codegen.md` §5.4, §5.6), so
/// depth `n + b` names that frame: the bias is `b`. A body has one when
/// every way it runs gives the same `b >= 1`: it is local (or a held
/// copy, which only direct calls reach), nothing but direct calls names
/// it, and each of those is one LLVM certainly inlines ([`inlined`],
/// [`inline_viable`], [`surely_inlined`]) from a body that itself has
/// one bias.
///
/// # Errors
/// The decline of a body with no bias, by its reason.
fn frame_bias(
    module: &Module,
    bodies: &[Body],
    held: &[Body],
    reached: &[bool],
    biased: &[(At, &'static str)],
) -> Result<Vec<(At, u32)>, CompileError> {
    // The graph: every accepted body and every reached copy.
    let mut nodes: Vec<(At, &Body)> =
        bodies.iter().map(|body| (At::Accepted(body.symbol()), body)).collect();
    nodes.extend(
        held.iter()
            .enumerate()
            .filter(|&(at, _)| reached[at])
            .map(|(at, body)| (At::Held(at), body)),
    );
    let mut node_of = vec![u32::MAX; module.symbol_count()];
    for (index, (_, body)) in nodes.iter().enumerate() {
        if let Some(slot) = node_of.get_mut(body.symbol().index()) {
            *slot = index as u32;
        }
    }
    let escapes = escaping(module, bodies, held, reached);
    let mut sets = vec![Biases::default(); nodes.len()];
    for (set, &(at, body)) in sets.iter_mut().zip(&nodes) {
        let sym = body.symbol();
        if let At::Accepted(_) = at
            && (!module.symbol(sym).linkage().is_local() || escapes[sym.index()])
        {
            set.bits |= 1;
        }
    }
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for (from, &(_, caller)) in nodes.iter().enumerate() {
        for site in caller.calls() {
            let Some(callee) = direct_callee(site.callee()) else { continue };
            let Some(&to) = node_of.get(callee.index()).filter(|&&to| to != u32::MAX) else {
                continue;
            };
            let target = nodes[to as usize].1;
            if !inlined(module, &site, callee) {
                sets[to as usize].bits |= 1;
                continue;
            }
            match inline_viable(module, target) {
                Viable::No => sets[to as usize].bits |= 1,
                Viable::Unsure => sets[to as usize].unsure = true,
                Viable::Yes if surely_inlined(module, caller, &site, target) => {
                    edges.push((from as u32, to));
                }
                Viable::Yes => sets[to as usize].unsure = true,
            }
        }
    }
    // A union of shifted sets, to the fixed point (each pass only adds
    // bits, of which there are 32 per body).
    let mut changed = true;
    while changed {
        changed = false;
        for &(from, to) in &edges {
            let from = sets[from as usize];
            let set = &mut sets[to as usize];
            let next = Biases {
                bits: set.bits | (from.bits << 1),
                unsure: set.unsure || from.unsure || from.bits >> MAX_BIAS != 0,
            };
            if next != *set {
                *set = next;
                changed = true;
            }
        }
    }
    let mut biases = Vec::with_capacity(biased.len());
    for &(at, why) in biased {
        let index = nodes.iter().position(|&(node, _)| node == at);
        let set = index.map(|index| sets[index]).unwrap_or(Biases { bits: 0, unsure: true });
        let sym = index.map_or_else(|| SymId::new(0), |index| nodes[index].1.symbol());
        if set.unsure || set.bits.count_ones() != 1 || set.bits == 1 {
            return Err(decline(module, sym, why));
        }
        biases.push((at, set.bits.trailing_zeros()));
    }
    Ok(biases)
}

/// Raises the depth of every `llvm.returnaddress` and `llvm.frameaddress`
/// of `body` by `bias` ([`frame_bias`]).
///
/// # Errors
/// A constant the module has no room for.
fn deepen(module: &mut Module, body: &mut Body, bias: u32) -> Result<(), CompileError> {
    let sites: Vec<(hod_ir::InstId, Value)> = body
        .calls()
        .filter(|site| {
            site.intrinsic()
                .is_some_and(|i| matches!(i.name(), "llvm.returnaddress" | "llvm.frameaddress"))
        })
        .filter_map(|site| site.args().first().map(|&depth| (site.inst(), depth)))
        .collect();
    let internal = |error: hod_ir::IrError| CompileError::Internal(format!("frame bias: {error}"));
    for (inst, depth) in sites {
        let ValueKind::Const(id) = depth.kind() else {
            return Err(CompileError::Internal(
                "frame bias: a depth that is not a constant".into(),
            ));
        };
        let ty = module.consts.ty(id);
        let deeper = module.consts.int_value(id).unwrap_or(0).saturating_add(u64::from(bias));
        let deeper = module
            .consts
            .int(&module.types, ty, deeper.min(u64::from(u32::MAX)))
            .map_err(internal)?;
        if !body.set_call_arg(inst, 0, Value::constant(deeper).map_err(internal)?) {
            return Err(CompileError::Internal("frame bias: no depth argument".into()));
        }
    }
    Ok(())
}
