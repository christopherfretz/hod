//! Calls (`spec/ll.md` §11.6, §11.7): `call`, `invoke` and `callbr`,
//! their callees (symbols, intrinsics, inline assembly, pointers), the
//! call-site attributes `hod_ir` keeps, operand bundles, and the calls of
//! intrinsics that generate no code.

use hod_ir::Layering;
use hod_ir::kit::{CALL_SITE_FLAGS, set_call_site_flags};
use hod_ir::{
    BlockId, Bundle, CallConv, CallDesc, Const, InlineAsm, Intrinsic, NoCode, Opcode, ParamAttrs,
    SymId, TailKind, TypeId, TypeKind, Value, no_code_intrinsic, resolve_intrinsic,
};

use crate::body::{CalleeInfo, Fx};
use crate::error::{LlErrorKind, PResult, fail};
use crate::index::parse_u32;
use crate::inst::Done;
use crate::lexer::{Tok, Token};
use crate::parser::ir;
use crate::{MdRef, Span};

/// What is called, as the text writes it.
#[derive(Clone, Copy)]
enum Target {
    /// A symbol: a direct call.
    Sym(SymId),
    /// Any other pointer value.
    Value(Value),
    /// `asm … "template", "constraints"`: the two string tokens and the
    /// flags.
    Asm(Token, Token, u8),
}

/// Whether the word `word` cannot start an attribute: what follows an
/// attribute list in a call. `hod_ir::attr_info` scans its whole table on
/// a miss, so the common followers are answered here first.
fn not_attr(word: &[u8]) -> bool {
    matches!(
        word,
        b"void"
            | b"ptr"
            | b"null"
            | b"true"
            | b"false"
            | b"undef"
            | b"poison"
            | b"zeroinitializer"
            | b"none"
            | b"float"
            | b"double"
            | b"half"
            | b"bfloat"
            | b"x86_fp80"
            | b"fp128"
            | b"to"
            | b"label"
            | b"ret"
            | b"br"
            | b"call"
            | b"tail"
            | b"musttail"
            | b"notail"
            | b"invoke"
            | b"store"
            | b"load"
            | b"unreachable"
            | b"getelementptr"
            | b"alloca"
            | b"icmp"
            | b"switch"
            | b"resume"
            | b"bitcast"
            | b"asm"
            | b"metadata"
            | b"token"
    )
}

impl<L: Layering> Fx<'_, '_, '_, L> {
    /// Parameter or return attributes, if the current token can start
    /// one.
    fn value_attrs(&mut self) -> PResult<ParamAttrs> {
        match self.p.tok.kind {
            Tok::Str => self.p.param_attrs(),
            Tok::Word if !not_attr(self.p.text()) => self.p.param_attrs(),
            _ => Ok(ParamAttrs::NONE),
        }
    }

    /// What is known of `sym` as a callee, found once per symbol.
    fn callee_info(&mut self, sym: SymId) -> CalleeInfo {
        let at = sym.index();
        if self.s.callees.len() <= at {
            self.s.callees.resize(at + 1, CalleeInfo::Unknown);
        }
        if let CalleeInfo::Unknown = self.s.callees[at] {
            let module = &*self.p.module;
            let symbol = module.symbol(sym);
            let info = match std::str::from_utf8(symbol.name()) {
                Ok(name) if name.starts_with("llvm.") && symbol.is_function() => {
                    if let Some(rule) = no_code_intrinsic(name) {
                        CalleeInfo::NoCode(rule)
                    } else {
                        let found = resolve_intrinsic(&module.types, name, symbol.ty());
                        let known = match found.intrinsic {
                            Intrinsic::Other(_) => None,
                            intrinsic => Intrinsic::ALL
                                .iter()
                                .copied()
                                .find(|member| member.name() == intrinsic.name()),
                        };
                        if let Some(member) = known {
                            let mut overloads = [TypeId::VOID; 3];
                            let given = found.overloads();
                            overloads[..given.len()].copy_from_slice(given);
                            CalleeInfo::Known(member, overloads, given.len() as u8)
                        } else {
                            CalleeInfo::Other
                        }
                    }
                }
                _ => CalleeInfo::Plain,
            };
            self.s.callees[at] = info;
        }
        self.s.callees[at]
    }

    /// `[cc | cc N]`.
    fn call_conv(&mut self) -> PResult<CallConv> {
        self.p.call_conv()
    }

    /// `asm [sideeffect] [alignstack] [inteldialect] [unwind] "t", "c"`,
    /// after `asm`.
    fn asm_callee(&mut self) -> PResult<Target> {
        let mut flags = 0;
        loop {
            let flag = match self.p.text() {
                _ if self.p.tok.kind != Tok::Word => break,
                b"sideeffect" => InlineAsm::SIDE_EFFECT,
                b"alignstack" => InlineAsm::ALIGN_STACK,
                b"inteldialect" => InlineAsm::INTEL_DIALECT,
                b"unwind" => InlineAsm::UNWIND,
                _ => break,
            };
            flags |= flag;
            self.p.bump();
        }
        let template = self.p.expect(Tok::Str, "expected the assembly text")?;
        self.p.expect(Tok::Comma, "expected comma in inline asm expression")?;
        let constraints = self.p.expect(Tok::Str, "expected the constraint string")?;
        Ok(Target::Asm(template, constraints, flags))
    }

    /// The callee, of pointer type `ptr_ty`.
    fn callee(&mut self, ptr_ty: TypeId) -> PResult<Target> {
        let token = self.p.tok;
        match token.kind {
            Tok::Global | Tok::GlobalQuoted | Tok::GlobalId => {
                let sym = self.p.global_ref(token)?;
                self.p.bump();
                if ptr_ty != TypeId::PTR {
                    // A symbol of another address space than the call's:
                    // left to the verifier, as a pointer value.
                    return Ok(Target::Value(ir(token.start, Value::sym(sym))?));
                }
                Ok(Target::Sym(sym))
            }
            Tok::Local | Tok::LocalQuoted | Tok::LocalId => {
                self.p.bump();
                Ok(Target::Value(self.local_value(token, ptr_ty)?))
            }
            Tok::Word if self.p.text() == b"asm" => {
                self.p.bump();
                self.asm_callee()
            }
            _ => {
                let id = self.p.parse_const(ptr_ty)?;
                if let Const::Addr { sym, addend: 0 } = self.p.module.consts.get(id) {
                    return Ok(Target::Sym(sym));
                }
                Ok(Target::Value(ir(token.start, Value::constant(id))?))
            }
        }
    }

    /// A metadata operand, after `metadata`: `!"string"`, `!N`, `!{…}`,
    /// `!DIKind(…)`, or `type value` (a value as metadata).
    pub(crate) fn metadata_operand(&mut self) -> PResult<()> {
        let start = self.p.tok.start;
        match self.p.tok.kind {
            Tok::Exclaim => {
                self.p.bump();
                if self.p.tok.kind == Tok::Str {
                    self.p.bump();
                    return Ok(());
                }
                self.p.goto(start);
            }
            Tok::Meta => {}
            _ => {
                let ty = self.p.parse_type()?;
                if ty == TypeId::METADATA {
                    return self.p.syntax("invalid metadata-value-metadata roundtrip");
                }
                self.value(ty)?;
                return Ok(());
            }
        }
        let node = self.p.md_ref()?;
        if let MdRef::Inline(_, span) = node
            && span.end >= self.end
        {
            return fail(LlErrorKind::Syntax, start, "this bracket is never closed");
        }
        Ok(())
    }

    /// A `metadata` argument of a call (WP-064, `spec/ll.md` §11.6): a
    /// metadata operand of the body (`spec/ir.md` §5.3) when it is a
    /// string `!"…"` or a node of strings (`!{…}` in place, or `!N`
    /// naming one), `None` for any other metadata, which is only read.
    fn metadata_arg(&mut self) -> PResult<Option<Value>> {
        let start = self.p.tok.start;
        let mut strings = Vec::new();
        let Some(node) = self.metadata_strings(&mut strings)? else {
            self.p.goto(start);
            self.metadata_operand()?;
            return Ok(None);
        };
        let strings: Vec<&str> = strings.iter().map(String::as_str).collect();
        Ok(Some(ir(start, self.b.metadata(node, &strings))?))
    }

    /// The strings of a metadata operand that is a string or a node of
    /// strings, and whether it is a node; `None` for any other operand,
    /// or a string that is not UTF-8. Leaves the parser after the operand
    /// only when it answers `Some`.
    fn metadata_strings(&mut self, out: &mut Vec<String>) -> PResult<Option<bool>> {
        if self.p.tok.kind != Tok::Exclaim {
            return Ok(None);
        }
        self.p.bump();
        let span = match self.p.tok.kind {
            Tok::Str => {
                let token = self.p.tok;
                self.p.bump();
                return Ok(self.utf8(token, out).then_some(false));
            }
            Tok::LBrace => {
                let span = self.p.skip_brackets()?;
                if span.end >= self.end {
                    return Ok(None);
                }
                span
            }
            Tok::Int => {
                let Some(id) = parse_u32(self.p.text()) else { return Ok(None) };
                let node = match self.p.text {
                    Some(text) => text.metadata().node(id).copied(),
                    None => self.p.md.node(id).copied(),
                };
                match node {
                    Some(node) if node.kind == crate::MdKind::Tuple && !node.distinct => {
                        self.p.bump();
                        node.span
                    }
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
        let resume = self.p.tok.start;
        self.p.goto(span.start);
        while self.p.tok.start < span.end {
            if !out.is_empty() && !self.p.eat(Tok::Comma) {
                return Ok(None);
            }
            if !self.p.eat(Tok::Exclaim) || self.p.tok.kind != Tok::Str {
                return Ok(None);
            }
            let token = self.p.tok;
            if !self.utf8(token, out) {
                return Ok(None);
            }
            self.p.bump();
        }
        self.p.goto(resume);
        Ok(Some(true))
    }

    /// Appends the contents of the string token `token` to `out` if they
    /// are UTF-8.
    fn utf8(&self, token: Token, out: &mut Vec<String>) -> bool {
        match String::from_utf8(self.p.string(token).into_owned()) {
            Ok(text) => {
                out.push(text);
                true
            }
            Err(_) => false,
        }
    }

    /// The function attributes after the arguments: `#N` and attributes
    /// in place. Only what `spec/ir.md` §5.7 keeps on a call is returned.
    fn call_fn_attrs(&mut self) -> PResult<u32> {
        let mut flags = 0;
        let mut cleared = false;
        loop {
            match self.p.tok.kind {
                Tok::AttrGroup => {
                    let id = parse_u32(&self.p.text()[1..]).unwrap_or(u32::MAX);
                    self.p.bump();
                    let Some(text) = self.p.text else { continue };
                    if let Some(group) = text.attr_group(id) {
                        flags |= self.p.module.fn_attrs(group.attrs).flags();
                        for name in &group.rejected {
                            let known = self
                                .p
                                .pending
                                .iter()
                                .any(|noted| noted.strip_prefix("attr:") == Some(&**name));
                            if !known {
                                self.p.note(format!("attr:{name}"));
                            }
                        }
                    }
                    continue;
                }
                Tok::Str => {}
                Tok::Word if !not_attr(self.p.text()) => {}
                _ => break,
            }
            if !cleared {
                self.p.fa.clear();
                cleared = true;
            }
            if !self.p.fn_attr(false)? {
                break;
            }
        }
        if cleared {
            flags |= self.p.fa.flags();
        }
        // The call-site function attributes `hod_ir` keeps (`spec/ir.md`
        // §5.7): `CallDesc`'s flags.
        Ok(flags & CALL_SITE_FLAGS)
    }

    /// `[ "tag"(type value, …), … ]`, into the bundle tables.
    fn bundles(&mut self) -> PResult<()> {
        self.s.bundles.clear();
        self.s.bundle_vals.clear();
        self.s.bundle_tags.clear();
        if !self.p.eat(Tok::LBracket) {
            return Ok(());
        }
        loop {
            let tag = self.p.expect(Tok::Str, "expected operand bundle tag")?;
            let name = self.p.string(tag);
            let tag_start = self.s.bundle_tags.len() as u32;
            self.s.bundle_tags.push_str(&String::from_utf8_lossy(&name));
            let tag_end = self.s.bundle_tags.len() as u32;
            self.p.expect(Tok::LParen, "expected '(' in operand bundle")?;
            let vals_start = self.s.bundle_vals.len() as u32;
            let mut first = true;
            while self.p.tok.kind != Tok::RParen {
                if !first {
                    self.p.expect(Tok::Comma, "expected ',' in input list")?;
                }
                first = false;
                if self.p.tok.start >= self.end {
                    return self.p.syntax("expected ')' at the end of an operand bundle");
                }
                let ty = self.p.parse_type()?;
                if ty == TypeId::METADATA {
                    self.metadata_operand()?;
                    self.p.note("bundle:metadata-input");
                    continue;
                }
                let value = self.value(ty)?;
                self.s.bundle_vals.push(value);
            }
            self.p.bump();
            let vals_end = self.s.bundle_vals.len() as u32;
            self.s.bundles.push((tag_start, tag_end, vals_start, vals_end));
            if !self.p.eat(Tok::Comma) {
                break;
            }
        }
        self.p.expect(Tok::RBracket, "expected ']' at the end of operand bundles")?;
        Ok(())
    }

    /// `[addrspace(N)] type`: the pointer type of the callee, where the
    /// type was, the type as written, and the return type.
    fn call_type(&mut self) -> PResult<(TypeId, u32, TypeId, TypeId)> {
        let mut ptr_ty = TypeId::PTR;
        if self.p.eat_word(b"addrspace") {
            self.p.expect(Tok::LParen, "expected '(' in an address space")?;
            let space_at = self.p.tok.start;
            let space = self.p.expect_u32("expected an address space number")?;
            self.p.expect(Tok::RParen, "expected ')' in an address space")?;
            ptr_ty = ir(space_at, self.p.module.types.ptr(space))?;
        }
        let ty_at = self.p.tok.start;
        let written = self.p.parse_type()?;
        let ret = match self.p.module.types.kind(written) {
            TypeKind::Function { ret, .. } => ret,
            _ => written,
        };
        if matches!(
            self.p.module.types.kind(ret),
            TypeKind::Function { .. } | TypeKind::Label | TypeKind::Metadata
        ) {
            return fail(LlErrorKind::Type, ty_at, "Invalid result type for LLVM function");
        }
        Ok((ptr_ty, ty_at, written, ret))
    }

    /// `( type [attrs] value, … )` into `vals`, `arg_tys`, `arg_attrs`
    /// and `arg_at`. Whether any argument has attributes, whether one is
    /// metadata, and whether the list ends in `...`.
    fn call_args(&mut self, tail: TailKind) -> PResult<(bool, bool)> {
        self.p.expect(Tok::LParen, "expected '(' in call")?;
        self.s.vals.clear();
        self.s.arg_tys.clear();
        self.s.arg_attrs.clear();
        self.s.arg_at.clear();
        let (mut any_attrs, mut metadata) = (false, false);
        while self.p.tok.kind != Tok::RParen {
            if !self.s.vals.is_empty() {
                self.p.expect(Tok::Comma, "expected ',' in argument list")?;
            }
            if self.p.tok.start >= self.end {
                return self.p.syntax("expected ')' at end of argument list");
            }
            if self.p.tok.kind == Tok::DotDotDot {
                if tail != TailKind::MustTail {
                    return self
                        .p
                        .syntax("unexpected ellipsis in argument list for non-musttail call");
                }
                if !self.s.variadic {
                    return self.p.syntax(
                        "unexpected ellipsis in argument list for musttail call in non-varargs \
                         function",
                    );
                }
                // It forwards the function's variable arguments; `hod_ir`
                // records nothing for it, since a `musttail` call in a
                // variadic function always does (`spec/ir.md` §5.8, §9.4).
                self.p.bump();
                if self.p.tok.kind != Tok::RParen {
                    return self.p.syntax("expected ')' at end of argument list");
                }
                self.p.bump();
                return Ok((any_attrs, metadata));
            }
            let arg_at = self.p.tok.start;
            let ty = self.p.parse_type()?;
            let attrs = self.value_attrs()?;
            any_attrs |= attrs != ParamAttrs::NONE;
            let value = if ty == TypeId::METADATA {
                // A string or a node of strings is a metadata operand
                // (WP-064); anything else cannot be represented.
                let meta = self.metadata_arg()?;
                metadata |= meta.is_none();
                meta.unwrap_or(Value::NONE)
            } else {
                self.value(ty)?
            };
            self.s.vals.push(value);
            self.s.arg_tys.push(ty);
            self.s.arg_attrs.push(attrs);
            self.s.arg_at.push(arg_at);
        }
        if tail == TailKind::MustTail && self.s.variadic {
            return self.p.syntax(
                "expected '...' at end of argument list for musttail call in varargs function",
            );
        }
        self.p.bump();
        Ok((any_attrs, metadata))
    }

    /// The function type of the call: the one written, checked against
    /// the arguments, or one made of the return type and the arguments'
    /// types.
    fn fn_type(
        &mut self,
        written: TypeId,
        ret: TypeId,
        ty_at: u32,
        target: Target,
    ) -> PResult<TypeId> {
        if let TypeKind::Function { params, variadic, .. } = self.p.module.types.kind(written) {
            for (index, &ty) in self.s.arg_tys.iter().enumerate() {
                let pos = self.s.arg_at[index];
                match params.get(index) {
                    Some(&param) if param != ty => {
                        return fail(
                            LlErrorKind::Type,
                            pos,
                            "argument is not of the expected type",
                        );
                    }
                    None if !variadic => {
                        return fail(LlErrorKind::Syntax, pos, "too many arguments specified");
                    }
                    _ => {}
                }
            }
            if self.s.arg_tys.len() < params.len() {
                let message = "not enough parameters specified for call";
                return fail(LlErrorKind::Syntax, ty_at, message);
            }
            return Ok(written);
        }
        // The callee's own type, when it is the one meant: no interning.
        if let Target::Sym(sym) = target {
            let sym_ty = self.p.module.symbol(sym).ty();
            if let TypeKind::Function { ret: r, params, variadic: false } =
                self.p.module.types.kind(sym_ty)
                && r == ret
                && params == self.s.arg_tys.as_slice()
            {
                return Ok(sym_ty);
            }
        }
        ir(ty_at, self.p.module.types.function(ret, &self.s.arg_tys, false))
    }

    /// `to label %n unwind label %u` of an invoke, or `to label %f [label
    /// %a, …]` of a callbr (the indirect ones into `targets`).
    fn successors(&mut self, op: Opcode) -> PResult<(BlockId, BlockId)> {
        match op {
            Opcode::Invoke => {
                self.p.expect_word(b"to", "expected 'to' in invoke")?;
                let normal = self.label()?;
                self.p.expect_word(b"unwind", "expected 'unwind' in invoke")?;
                Ok((normal, self.label()?))
            }
            Opcode::CallBr => {
                self.p.expect_word(b"to", "expected 'to' in callbr")?;
                let fallthrough = self.label()?;
                self.p.expect(Tok::LBracket, "expected '[' in callbr")?;
                self.s.targets.clear();
                if self.p.tok.kind != Tok::RBracket {
                    loop {
                        let target = self.label()?;
                        self.s.targets.push(target);
                        if !self.p.eat(Tok::Comma) {
                            break;
                        }
                    }
                }
                self.p.expect(Tok::RBracket, "expected ']' at end of block list")?;
                Ok((fallthrough, fallthrough))
            }
            _ => Ok((BlockId::new(0), BlockId::new(0))),
        }
    }

    /// The end of a call that is not emitted: an `invoke` or `callbr`
    /// still reads its successors and ends the block with a stand-in
    /// branch.
    fn stand_in_call(&mut self, at: u32, op: Opcode, ret: TypeId) -> PResult<Done> {
        let value = self.stand_in(at, ret)?;
        if op == Opcode::Call {
            return Ok((ret, value, false));
        }
        let (normal, _) = self.successors(op)?;
        ir(at, self.b.br(normal))?;
        Ok((ret, value, true))
    }

    /// `call`, `invoke` or `callbr`, after the keyword (and after `tail`
    /// and its kin for a call).
    pub(crate) fn call(&mut self, at: u32, op: Opcode, tail: TailKind) -> PResult<Done> {
        if op == Opcode::Call {
            self.fast_math();
        }
        let cc = self.call_conv()?;
        let ret_attrs = self.value_attrs()?;
        let (ptr_ty, ty_at, written, ret) = self.call_type()?;
        let callee_at = self.p.tok.start;
        let callee_token = self.p.tok;
        let target = self.callee(ptr_ty)?;
        let info = match target {
            Target::Sym(sym) => self.callee_info(sym),
            _ => CalleeInfo::Plain,
        };

        // A debug intrinsic is a debug record in the old form: its
        // arguments are not values (`spec/ll.md` §11.7).
        if let CalleeInfo::NoCode(NoCode::DebugRecord) = info {
            if self.p.tok.kind != Tok::LParen {
                return self.p.syntax("expected '(' in call");
            }
            let name = match callee_token.kind {
                Tok::GlobalQuoted => {
                    Span { start: callee_token.start + 2, end: callee_token.end - 1 }
                }
                _ => Span { start: callee_token.start + 1, end: callee_token.end },
            };
            let name = &self.p.src[name.start as usize..name.end as usize];
            self.debug_call(name)?;
            self.call_fn_attrs()?;
            self.bundles()?;
            return self.stand_in_call(at, op, ret);
        }

        let (any_attrs, metadata) = self.call_args(tail)?;
        let fn_ty = self.fn_type(written, ret, ty_at, target)?;
        let flags = self.call_fn_attrs()?;
        self.bundles()?;

        // An intrinsic must be called with the type it is declared with.
        let llvm =
            matches!(info, CalleeInfo::NoCode(_) | CalleeInfo::Known(..) | CalleeInfo::Other);
        if llvm
            && let Target::Sym(sym) = target
            && self.p.module.symbol(sym).ty() != fn_ty
        {
            let message = "intrinsic called with incompatible signature";
            return fail(LlErrorKind::Type, callee_at, message);
        }

        // The calls that generate no code (`spec/ir.md` §5.5).
        if let CalleeInfo::NoCode(rule) = info {
            if op != Opcode::Call {
                self.p.note("invoke:no-code-intrinsic");
                return self.stand_in_call(at, op, ret);
            }
            if rule == NoCode::FirstArg {
                if self.s.arg_tys.first() != Some(&ret) {
                    let message =
                        "the first argument of this intrinsic has another type than its result";
                    return fail(LlErrorKind::Type, callee_at, message);
                }
                return Ok((ret, self.s.vals[0], false));
            }
            if let Target::Sym(sym) = target
                && let Ok(name) = std::str::from_utf8(self.p.module.symbol(sym).name())
                && let Some(start) = hod_ir::lifetime_intrinsic(name)
                && let Some(&ptr) = self.s.vals.last()
            {
                ir(callee_at, self.b.lifetime_marker(start, ptr))?;
            }
            return self.stand_in_call(at, op, ret);
        }

        // Metadata operands are arguments of the intrinsics on hod's list
        // that take them (`spec/ir.md` §5.5); for any other callee the
        // call is not emitted.
        let metadata = metadata
            || (!matches!(info, CalleeInfo::Known(..))
                && self.s.arg_tys.contains(&TypeId::METADATA));
        if metadata {
            let construct = match target {
                Target::Sym(sym) => format!(
                    "metadata-arg:{}",
                    String::from_utf8_lossy(self.p.module.symbol(sym).name())
                ),
                _ => "metadata-arg:indirect".to_string(),
            };
            self.p.note(construct);
            return self.stand_in_call(at, op, ret);
        }

        let callee = self.lowered_callee(callee_at, target, info, fn_ty)?;
        let (normal, unwind) = self.successors(op)?;
        let site = Site { cc, tail, flags, ret_attrs, any_attrs, fn_ty, ret };
        let value = self.emit_call(at, op, site, callee, info, target, (normal, unwind))?;
        Ok((ret, value, op != Opcode::Call))
    }

    /// The callee as `hod_ir` has it; `None` for an intrinsic.
    fn lowered_callee(
        &mut self,
        at: u32,
        target: Target,
        info: CalleeInfo,
        fn_ty: TypeId,
    ) -> PResult<Option<Value>> {
        Ok(match target {
            Target::Sym(sym) => match info {
                CalleeInfo::Known(..) | CalleeInfo::Other => None,
                _ => Some(ir(at, Value::sym(sym))?),
            },
            Target::Value(value) => Some(value),
            Target::Asm(template, constraints, flags) => {
                let template = self.p.string(template);
                let constraints = self.p.string(constraints);
                let asm =
                    InlineAsm { template: &template, constraints: &constraints, flags, ty: fn_ty };
                let id = ir(at, self.p.module.inline_asm(&asm))?;
                Some(ir(at, Value::asm(id))?)
            }
        })
    }

    /// Builds the call from what the tables hold.
    #[allow(clippy::too_many_arguments)]
    fn emit_call(
        &mut self,
        at: u32,
        op: Opcode,
        site: Site,
        callee: Option<Value>,
        info: CalleeInfo,
        target: Target,
        (normal, unwind): (BlockId, BlockId),
    ) -> PResult<Value> {
        let bundles: Vec<Bundle<'_>>;
        let bundles_slice: &[Bundle<'_>] = if self.s.bundles.is_empty() {
            &[]
        } else {
            let (tags, vals) = (&self.s.bundle_tags, &self.s.bundle_vals);
            bundles = self
                .s
                .bundles
                .iter()
                .map(|&(t0, t1, v0, v1)| Bundle {
                    tag: &tags[t0 as usize..t1 as usize],
                    inputs: &vals[v0 as usize..v1 as usize],
                })
                .collect();
            &bundles
        };
        let overloads;
        let args = &self.s.vals;
        let mut desc = match (callee, info, target) {
            (Some(value), _, _) => CallDesc::new(value, site.fn_ty, args),
            (None, CalleeInfo::Known(intrinsic, types, len), _) => {
                overloads = types;
                CallDesc::intrinsic(intrinsic, &overloads[..usize::from(len)], site.fn_ty, args)
            }
            (None, _, Target::Sym(sym)) => {
                let Ok(name) = std::str::from_utf8(self.p.module.symbol(sym).name()) else {
                    unreachable!("an intrinsic's name was read as UTF-8")
                };
                CallDesc::intrinsic(Intrinsic::Other(name), &[], site.fn_ty, args)
            }
            (None, _, _) => unreachable!("only a symbol callee is an intrinsic"),
        };
        desc.cc = site.cc;
        desc.tail = site.tail;
        set_call_site_flags(&mut desc, site.flags);
        desc.ret_attrs = site.ret_attrs;
        desc.arg_attrs = if site.any_attrs { &self.s.arg_attrs } else { &[] };
        desc.bundles = bundles_slice;
        let ret = site.ret;
        match op {
            Opcode::Invoke => ir(at, self.b.invoke(ret, &desc, normal, unwind)),
            Opcode::CallBr => ir(at, self.b.callbr(ret, &desc, normal, &self.s.targets)),
            _ => ir(at, self.b.call(ret, &desc)),
        }
    }
}

/// What a call site says besides its callee and operands.
#[derive(Clone, Copy)]
struct Site {
    cc: CallConv,
    tail: TailKind,
    /// The kept function attributes, `FnAttrs` flags.
    flags: u32,
    ret_attrs: ParamAttrs,
    any_attrs: bool,
    fn_ty: TypeId,
    ret: TypeId,
}
