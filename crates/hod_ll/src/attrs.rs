//! Attributes (`spec/ll.md` §5.6): each one goes through `hod_ir`'s
//! table and is kept, dropped, or noted as `attr:<name>`.

use hod_ir::{
    AttrClass, AttrSyntax, FnAttrs, FnAttrsId, FnStr, ParamAttrs, ParamMem, UwTable, attr_info,
};

use crate::AttrGroup;
use crate::error::{LlErrorKind, PResult, fail};
use crate::index::parse_u32;
use crate::lexer::Tok;
use crate::meta::Upgrade;
use crate::parser::{Parser, ir};

/// LLVM's number for the calling convention written `keyword`
/// (`b"fastcc"`), for the ones that have a keyword; `cc N` is written
/// with its number.
pub fn call_conv_keyword(keyword: &[u8]) -> Option<u32> {
    Some(match keyword {
        b"ccc" => 0,
        b"fastcc" => 8,
        b"coldcc" => 9,
        b"ghccc" => 10,
        b"anyregcc" => 13,
        b"preserve_mostcc" => 14,
        b"preserve_allcc" => 15,
        b"swiftcc" => 16,
        b"cxx_fast_tlscc" => 17,
        b"tailcc" => 18,
        b"cfguard_checkcc" => 19,
        b"swifttailcc" => 20,
        b"preserve_nonecc" => 21,
        b"x86_stdcallcc" => 64,
        b"x86_fastcallcc" => 65,
        b"arm_apcscc" => 66,
        b"arm_aapcscc" => 67,
        b"arm_aapcs_vfpcc" => 68,
        b"msp430_intrcc" => 69,
        b"x86_thiscallcc" => 70,
        b"ptx_kernel" => 71,
        b"ptx_device" => 72,
        b"spir_func" => 75,
        b"spir_kernel" => 76,
        b"intel_ocl_bicc" => 77,
        b"x86_64_sysvcc" => 78,
        b"win64cc" => 79,
        b"x86_vectorcallcc" => 80,
        b"hhvmcc" => 81,
        b"hhvm_ccc" => 82,
        b"x86_intrcc" => 83,
        b"avr_intrcc" => 84,
        b"avr_signalcc" => 85,
        b"amdgpu_vs" => 87,
        b"amdgpu_gs" => 88,
        b"amdgpu_ps" => 89,
        b"amdgpu_cs" => 90,
        b"amdgpu_kernel" => 91,
        b"x86_regcallcc" => 92,
        b"amdgpu_hs" => 93,
        b"amdgpu_ls" => 95,
        b"amdgpu_es" => 96,
        b"aarch64_vector_pcs" => 97,
        b"aarch64_sve_vector_pcs" => 98,
        b"amdgpu_gfx" => 100,
        b"m68k_intrcc" => 101,
        b"aarch64_sme_preservemost_from_x0" => 102,
        b"aarch64_sme_preservemost_from_x2" => 103,
        b"amdgpu_cs_chain" => 104,
        b"amdgpu_cs_chain_preserve" => 105,
        b"m68k_rtdcc" => 106,
        b"graalcc" => 107,
        b"riscv_vector_cc" => 108,
        b"aarch64_sme_preservemost_from_x1" => 109,
        _ => return None,
    })
}

fn lossy(name: &[u8]) -> String {
    format!("attr:{}", String::from_utf8_lossy(name))
}

impl Parser<'_> {
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
        match attr_info(AttrSyntax::String, &name).map(|row| row.class) {
            Some(AttrClass::Dropped) => {}
            Some(AttrClass::Kept) if function => {
                if let Some((key, rest)) = FnStr::from_name(&name) {
                    let value = if key == FnStr::NoBuiltin { rest } else { &value };
                    ir(at, self.fa.set_string(key, value))?;
                }
            }
            _ => self.note(lossy(&name)),
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
        let Some(row) = attr_info(AttrSyntax::Keyword, name) else {
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
                if let Some(flag) = FnAttrs::flag_named(name) {
                    ir(at, self.fa.set(flag))?;
                    return Ok(true);
                }
                match row.class {
                    AttrClass::Dropped => {}
                    AttrClass::Kept => {
                        let message = "this attribute does not apply to functions";
                        return fail(LlErrorKind::Syntax, at, message);
                    }
                    AttrClass::Rejected => self.note(lossy(name)),
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
            let Some(row) = attr_info(AttrSyntax::Keyword, name) else {
                return Ok(attrs);
            };
            self.bump();
            let flag = match name {
                b"zeroext" => Some(ParamAttrs::ZEROEXT),
                b"signext" => Some(ParamAttrs::SIGNEXT),
                b"inreg" => Some(ParamAttrs::INREG),
                b"nest" => Some(ParamAttrs::NEST),
                b"swiftself" => Some(ParamAttrs::SWIFT_SELF),
                b"swifterror" => Some(ParamAttrs::SWIFT_ERROR),
                b"swiftasync" => Some(ParamAttrs::SWIFT_ASYNC),
                _ => None,
            };
            let mem = match name {
                b"sret" => Some(ParamMem::Sret),
                b"byval" => Some(ParamMem::ByVal),
                b"byref" => Some(ParamMem::ByRef),
                b"inalloca" => Some(ParamMem::InAlloca),
                b"preallocated" => Some(ParamMem::Preallocated),
                b"elementtype" => Some(ParamMem::ElementType),
                _ => None,
            };
            if let Some(flag) = flag {
                attrs = ir(at, attrs.with(flag))?;
            } else if let Some(mem) = mem {
                self.expect(Tok::LParen, "expected '(' and a type after the attribute")?;
                let ty = self.parse_type()?;
                self.expect(Tok::RParen, "expected ')' after the attribute's type")?;
                if attrs.mem() == ParamMem::None {
                    attrs = ir(at, attrs.with_mem(mem, ty))?;
                } else if mem == ParamMem::ElementType || attrs.mem() == ParamMem::ElementType {
                    // `elementtype` beside another typed attribute has
                    // no representation (`spec/ir.md` §5.7).
                    let other = if mem == ParamMem::ElementType { attrs.mem() } else { mem };
                    self.note(format!("attr:elementtype-with-{}", mem_keyword(other)));
                    if attrs.mem() == ParamMem::ElementType {
                        attrs = ir(at, clear_mem(attrs).with_mem(mem, ty))?;
                    }
                } else {
                    return fail(LlErrorKind::Syntax, at, "two typed attributes on one value");
                }
            } else if name == b"align" {
                let bytes = if self.tok.kind == Tok::LParen {
                    self.paren_u64()?
                } else {
                    self.expect_u64("expected an alignment")?
                };
                attrs = ir(at, attrs.with_align(bytes))?;
            } else if name == b"alignstack" {
                let bytes = self.paren_u64()?;
                attrs = ir(at, attrs.with_stack_align(bytes))?;
            } else {
                match row.class {
                    AttrClass::Dropped => {}
                    AttrClass::Kept => {
                        let message = "this attribute does not apply to values";
                        return fail(LlErrorKind::Syntax, at, message);
                    }
                    AttrClass::Rejected => self.note(lossy(name)),
                }
                self.skip_attr_args()?;
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
                ir(at, self.fa.set(view.flags()))?;
                if view.uwtable() != UwTable::None {
                    self.fa.set_uwtable(view.uwtable());
                }
                if let Some(bytes) = view.stack_align() {
                    ir(at, self.fa.set_stack_align(bytes))?;
                }
                for (key, value) in view.strings() {
                    ir(at, self.fa.set_string(key, value))?;
                }
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

fn mem_keyword(mem: ParamMem) -> &'static str {
    match mem {
        ParamMem::Sret => "sret",
        ParamMem::ByVal => "byval",
        ParamMem::ByRef => "byref",
        ParamMem::InAlloca => "inalloca",
        ParamMem::Preallocated => "preallocated",
        _ => "elementtype",
    }
}

/// `attrs` without its typed attribute.
fn clear_mem(attrs: ParamAttrs) -> ParamAttrs {
    let mut out = ParamAttrs::NONE;
    for flag in [
        ParamAttrs::ZEROEXT,
        ParamAttrs::SIGNEXT,
        ParamAttrs::INREG,
        ParamAttrs::NEST,
        ParamAttrs::SWIFT_SELF,
        ParamAttrs::SWIFT_ERROR,
        ParamAttrs::SWIFT_ASYNC,
    ] {
        if attrs.has(flag) {
            out = out.with(flag).unwrap_or(out);
        }
    }
    if let Some(bytes) = attrs.align() {
        out = out.with_align(bytes).unwrap_or(out);
    }
    if let Some(bytes) = attrs.stack_align() {
        out = out.with_stack_align(bytes).unwrap_or(out);
    }
    out
}
