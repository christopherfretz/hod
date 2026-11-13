//! Attributes (`spec/ll.md` §5.6): each one goes through `hod_ir`'s
//! table and is kept, dropped, or noted as `attr:<name>`.

use hod_ir::Layering;
use hod_ir::kit::{
    AttrUse, TypedAttrError, ValueKeyword, add_typed, attr_construct, fn_keyword, merge_fn_attrs,
    string_attr, value_keyword,
};
use hod_ir::{
    AttrInfo, AttrSyntax, CallConv, FnAttrs, FnAttrsId, FnStr, ParamAttrs, UwTable, attr_info,
};

use crate::AttrGroup;
use crate::error::{LlErrorKind, PResult, fail};
use crate::index::parse_u32;
use crate::lexer::Tok;
use crate::meta::Upgrade;
use crate::parser::{Parser, ir};

/// What `hod_ir::attr_info` answered for the keywords met so far
/// (WP-096): a call site asks it for every word after a type, most of
/// them the same few (`noundef`, `nonnull`, `align`, `ptr`), and each
/// answer is a binary search of byte strings. Direct-mapped, keyed by
/// the keyword itself, so an answer from here is the table's answer.
#[derive(Debug, Default)]
pub(crate) struct AttrMemo {
    slots: Vec<(u128, Option<&'static AttrInfo>)>,
    /// How many times the table itself was searched.
    pub lookups: u64,
}

/// A word of 1 to 16 bytes without a zero byte, padded with zeros: a key
/// that no other such word has, and never 0. `None` for any other word.
pub(crate) fn packed(word: &[u8]) -> Option<u128> {
    if word.is_empty() || word.len() > 16 || word.contains(&0) {
        return None;
    }
    let mut bytes = [0u8; 16];
    bytes[..word.len()].copy_from_slice(word);
    Some(u128::from_le_bytes(bytes))
}

/// Slots of [`AttrMemo`]: a power of two.
const MEMO_SLOTS: usize = 64;

impl AttrMemo {
    /// `attr_info(AttrSyntax::Keyword, word)` for a word token.
    pub(crate) fn keyword(&mut self, word: &[u8]) -> Option<&'static AttrInfo> {
        // A word has no zero byte, so a word of at most 16 bytes padded
        // with zeros is a key that no other word has, and no key is 0.
        let Some(key) = packed(word) else {
            self.lookups += 1;
            return attr_info(AttrSyntax::Keyword, word);
        };
        if self.slots.is_empty() {
            self.slots.resize(MEMO_SLOTS, (0, None));
        }
        let mixed = (key as u64) ^ ((key >> 64) as u64).rotate_left(23);
        let at = (mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize % MEMO_SLOTS;
        let slot = &mut self.slots[at];
        if slot.0 != key {
            self.lookups += 1;
            *slot = (key, attr_info(AttrSyntax::Keyword, word));
        }
        slot.1
    }
}

/// LLVM's number for the calling convention written `keyword`
/// (`b"fastcc"`), for the ones that have a keyword in LLVM 19 to 22
/// ([`CallConv::from_keyword`]); `cc N` is written with its number, and
/// `riscv_vls_cc(N)` is read by the parser as one keyword.
pub fn call_conv_keyword(keyword: &[u8]) -> Option<u32> {
    CallConv::from_keyword(keyword).map(|cc| u32::from(cc.llvm_id()))
}

impl<L: Layering> Parser<'_, '_, L> {
    /// `[ccc | fastcc | ... | cc N | riscv_vls_cc(N)]`: a calling
    /// convention, `ccc` if there is none.
    pub(crate) fn call_conv(&mut self) -> PResult<CallConv> {
        let at = self.tok.start;
        let mut cc = 0;
        if self.eat_word(b"cc") {
            cc = self.expect_u32("expected a calling convention number")?;
        } else if self.at_word(b"riscv_vls_cc") {
            self.bump();
            self.expect(Tok::LParen, "expected '(' after riscv_vls_cc")?;
            let width = self.expect_u32("expected a vector length")?;
            self.expect(Tok::RParen, "expected ')'")?;
            let keyword = format!("riscv_vls_cc({width})");
            match call_conv_keyword(keyword.as_bytes()) {
                Some(number) => cc = number,
                None => return self.syntax("expected a power of two from 32 to 65536"),
            }
        } else if self.tok.kind == Tok::Word
            && let Some(number) = call_conv_keyword(self.text())
        {
            cc = number;
            self.bump();
        }
        ir(at, CallConv::from_llvm(cc))
    }

    /// Parses every attribute group. Of two definitions of one number
    /// the later one is the group, as in LLVM; the earlier one is still
    /// read, and an error in it is an error.
    pub(crate) fn define_groups(&mut self) -> PResult<()> {
        let mut order: Vec<(u32, usize)> = Vec::with_capacity(self.index.groups.len());
        for (at, ent) in self.index.groups.iter().enumerate() {
            match parse_u32(&self.tok_text(ent.id)[1..]) {
                Some(id) => order.push((id, at)),
                None => {
                    return fail(LlErrorKind::Syntax, ent.id.start, "attribute group number");
                }
            }
        }
        order.sort_unstable();
        let mut next = 0;
        while next < order.len() {
            let id = order[next].0;
            let mut at = 0;
            while next < order.len() && order[next].0 == id {
                let ent = self.index.groups[order[next].1];
                at = ent.id.start;
                self.fa.clear();
                self.fa_align = None;
                self.pending.clear();
                self.goto(ent.span.start);
                if self.tok.start >= ent.span.end {
                    return self.syntax("attribute group has no attributes");
                }
                while self.tok.start < ent.span.end {
                    if !self.fn_attr(true)? {
                        return self.syntax("expected a function attribute");
                    }
                }
                next += 1;
            }
            let attrs = ir(at, self.module.intern_fn_attrs(&self.fa))?;
            let rejected = self.pending.drain(..).map(|name| name[5..].into()).collect();
            self.groups.push(AttrGroup { id, attrs, align: self.fa_align, rejected });
        }
        Ok(())
    }

    /// Steps over the parenthesized arguments of an attribute, if it has
    /// any.
    fn skip_attr_args(&mut self) -> PResult<()> {
        if self.tok.kind == Tok::LParen {
            self.skip_brackets()?;
        }
        Ok(())
    }

    /// `(N)`.
    fn paren_u64(&mut self) -> PResult<u64> {
        self.expect(Tok::LParen, "expected '(' after the attribute")?;
        let value = self.expect_u64("expected a number")?;
        self.expect(Tok::RParen, "expected ')' after the attribute's value")?;
        Ok(value)
    }

    /// `"name"` or `"name"="value"`. `function` says whether the place
    /// is a function's: the kept strings are function attributes.
    fn string_attr(&mut self, function: bool) -> PResult<()> {
        let at = self.tok.start;
        let name = self.string(self.tok);
        self.bump();
        let value = if self.eat(Tok::Equal) {
            let token = self.expect(Tok::Str, "expected a string after '='")?;
            self.string(token)
        } else {
            (&[][..]).into()
        };
        let fa = if function { Some(&mut self.fa) } else { None };
        if ir(at, string_attr(fa, &name, &value))? == AttrUse::Rejected {
            self.note(attr_construct(&name));
        }
        Ok(())
    }

    /// Parses one function attribute into `self.fa`, if the current
    /// token starts one. In a group (`in_group`) `align` and `alignstack`
    /// are written with `=`; outside, the words that follow the
    /// attributes in a function header are not attributes.
    pub(crate) fn fn_attr(&mut self, in_group: bool) -> PResult<bool> {
        if self.tok.kind == Tok::Str {
            self.string_attr(true)?;
            return Ok(true);
        }
        if self.tok.kind != Tok::Word {
            return Ok(false);
        }
        let name = self.text();
        let at = self.tok.start;
        let Some(row) = self.attr_memo.keyword(name) else {
            // Not an attribute of LLVM 19 to 22: the list ends here.
            return Ok(false);
        };
        self.bump();
        match name {
            b"uwtable" => {
                let mut kind = UwTable::Async;
                if self.eat(Tok::LParen) {
                    kind = match self.text() {
                        b"sync" => UwTable::Sync,
                        b"async" => UwTable::Async,
                        _ => return self.syntax("expected 'sync' or 'async'"),
                    };
                    self.bump();
                    self.expect(Tok::RParen, "expected ')' after the unwind table kind")?;
                }
                self.fa.set_uwtable(kind);
            }
            b"alignstack" => {
                let bytes = if in_group && self.eat(Tok::Equal) {
                    self.expect_u64("expected a number")?
                } else {
                    self.paren_u64()?
                };
                ir(at, self.fa.set_stack_align(bytes))?;
            }
            b"align" => {
                if in_group {
                    self.expect(Tok::Equal, "expected '=' after align in a group")?;
                }
                self.fa_align = Some(self.expect_u64("expected an alignment")?);
            }
            _ => {
                match ir(at, fn_keyword(&mut self.fa, name, row.class))? {
                    AttrUse::Kept => return Ok(true),
                    AttrUse::Dropped => {}
                    AttrUse::Misplaced => {
                        let message = "this attribute does not apply to functions";
                        return fail(LlErrorKind::Syntax, at, message);
                    }
                    AttrUse::Rejected => self.note(attr_construct(name)),
                }
                self.skip_attr_args()?;
            }
        }
        Ok(true)
    }

    /// The attributes of a parameter or of a return value: the run ends
    /// at the first token that is no attribute.
    pub(crate) fn param_attrs(&mut self) -> PResult<ParamAttrs> {
        let mut attrs = ParamAttrs::NONE;
        loop {
            let at = self.tok.start;
            if self.tok.kind == Tok::Str {
                self.string_attr(false)?;
                continue;
            }
            if self.tok.kind != Tok::Word {
                return Ok(attrs);
            }
            let name = self.text();
            let Some(row) = self.attr_memo.keyword(name) else {
                return Ok(attrs);
            };
            self.bump();
            match value_keyword(name) {
                ValueKeyword::Flag(flag) => attrs = ir(at, attrs.with(flag))?,
                ValueKeyword::Typed(mem) => {
                    self.expect(Tok::LParen, "expected '(' and a type after the attribute")?;
                    let ty = self.parse_type()?;
                    self.expect(Tok::RParen, "expected ')' after the attribute's type")?;
                    // `elementtype` beside another typed attribute has
                    // no representation (`spec/ir.md` §5.7).
                    match add_typed(attrs, mem, ty) {
                        Ok((added, construct)) => {
                            if let Some(construct) = construct {
                                self.note(construct);
                            }
                            attrs = added;
                        }
                        Err(TypedAttrError::TwoTyped) => {
                            return fail(
                                LlErrorKind::Syntax,
                                at,
                                "two typed attributes on one value",
                            );
                        }
                        Err(TypedAttrError::Ir(error)) => return ir(at, Err(error)),
                    }
                }
                ValueKeyword::Align => {
                    let bytes = if self.tok.kind == Tok::LParen {
                        self.paren_u64()?
                    } else {
                        self.expect_u64("expected an alignment")?
                    };
                    attrs = ir(at, attrs.with_align(bytes))?;
                }
                ValueKeyword::StackAlign => {
                    let bytes = self.paren_u64()?;
                    attrs = ir(at, attrs.with_stack_align(bytes))?;
                }
                ValueKeyword::Other => {
                    match AttrUse::of_class(row.class) {
                        AttrUse::Misplaced => {
                            let message = "this attribute does not apply to values";
                            return fail(LlErrorKind::Syntax, at, message);
                        }
                        AttrUse::Rejected => self.note(attr_construct(name)),
                        AttrUse::Dropped | AttrUse::Kept => {}
                    }
                    self.skip_attr_args()?;
                }
            }
        }
    }

    /// The attribute set of a function whose own attributes are in
    /// `self.fa` and that names the groups `groups`; and the alignment
    /// the groups give. `defined` says whether it has a body: LLVM's
    /// reader rewrites the branch-protection attributes of definitions
    /// (§7.3).
    pub(crate) fn function_attrs(
        &mut self,
        groups: &[(u32, u32)],
        defined: bool,
    ) -> PResult<(FnAttrsId, Option<u64>)> {
        let mut align = None;
        let mut single = None;
        for &(id, at) in groups {
            // A group that is used but not defined is empty.
            let Ok(found) = self.groups.binary_search_by_key(&id, |group| group.id) else {
                continue;
            };
            let group = &self.groups[found];
            align = group.align.or(align);
            for name in &group.rejected {
                let construct = format!("attr:{name}");
                if !self.pending.iter().any(|noted| **noted == construct) {
                    self.pending.push(construct.into());
                }
            }
            if groups.len() == 1 && self.fa.is_empty() {
                single = Some(group.attrs);
            } else {
                let view = self.module.fn_attrs(group.attrs);
                ir(at, merge_fn_attrs(&mut self.fa, &view))?;
            }
        }
        let at = groups.first().map_or(self.tok.start, |&(_, at)| at);
        let mut id = match single {
            Some(id) => id,
            None => ir(at, self.module.intern_fn_attrs(&self.fa))?,
        };
        if defined && let Some(upgrade) = self.upgrade {
            id = self.upgrade_attrs(id, upgrade, at)?;
        }
        Ok((id, align))
    }

    /// The attribute set LLVM's reader turns `id` into on a definition
    /// in an AArch64 module whose branch-protection module flags are not
    /// in the current form (§7.3).
    fn upgrade_attrs(&mut self, id: FnAttrsId, upgrade: Upgrade, at: u32) -> PResult<FnAttrsId> {
        if let Some(&(_, done)) = self.upgraded.iter().find(|(from, _)| *from == id) {
            return Ok(done);
        }
        let view = self.module.fn_attrs(id);
        let mut out = FnAttrs::new();
        ir(at, out.set(view.flags()))?;
        out.set_uwtable(view.uwtable());
        if let Some(bytes) = view.stack_align() {
            ir(at, out.set_stack_align(bytes))?;
        }
        let drop_sign = !upgrade.sign && view.string(FnStr::SignReturnAddress) == Some(b"none");
        for (key, value) in view.strings() {
            let value = match key {
                FnStr::SignReturnAddress | FnStr::SignReturnAddressKey if drop_sign => continue,
                FnStr::BranchTargetEnforcement
                | FnStr::BranchProtectionPauthLr
                | FnStr::GuardedControlStack => {
                    if value.eq_ignore_ascii_case(b"false") {
                        continue;
                    }
                    if value.eq_ignore_ascii_case(b"true") { b"" } else { value }
                }
                _ => value,
            };
            ir(at, out.set_string(key, value))?;
        }
        let mut default = |key: FnStr, wanted: bool, value: &[u8]| {
            if wanted && view.string(key).is_none() {
                return out.set_string(key, value);
            }
            Ok(())
        };
        let kind: &[u8] = if upgrade.sign_all { b"all" } else { b"non-leaf" };
        let sign_key: &[u8] = if upgrade.sign_b_key { b"b_key" } else { b"a_key" };
        ir(at, default(FnStr::SignReturnAddress, upgrade.sign, kind))?;
        ir(at, default(FnStr::SignReturnAddressKey, upgrade.sign, sign_key))?;
        ir(at, default(FnStr::BranchTargetEnforcement, upgrade.bte, b""))?;
        ir(at, default(FnStr::BranchProtectionPauthLr, upgrade.pauth_lr, b""))?;
        ir(at, default(FnStr::GuardedControlStack, upgrade.gcs, b""))?;
        let done = ir(at, self.module.intern_fn_attrs(&out))?;
        self.upgraded.push((id, done));
        Ok(done)
    }
}
