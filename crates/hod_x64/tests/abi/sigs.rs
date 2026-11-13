//! The signatures the tests check: their LLVM text, their `hod_cg::abi`
//! form, and where each case keeps its bytes while it runs.

use std::fmt::Write as _;

use hod_cg::abi::{Assignment, Part, Place, Sig};
use hod_ir::{
    CallConv, DataLayout, ParamAttrs, ParamMem, TypeId, TypeTable, X86_64_LINUX_DATA_LAYOUT,
};
use hod_x64::abi::{Features, assign, assumed_extension, produced_extension};

use crate::ty::{Image, Rng, SLACK, Ty};

/// A fresh type table for the target.
pub fn table() -> TypeTable {
    TypeTable::new(&DataLayout::parse(X86_64_LINUX_DATA_LAYOUT).expect("the target's layout"))
}

/// What a parameter carries besides its type.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Attr {
    None,
    /// `zeroext`, or `signext` when true.
    Ext(bool),
    /// `alignstack(n)`.
    StackAlign(u64),
    /// `sret(i64)`.
    Sret,
    /// `byval(pointee)` with `align` and `alignstack`.
    ByVal {
        pointee: Ty,
        align: Option<u64>,
        stack_align: Option<u64>,
    },
    /// `byref(i64)`.
    ByRef,
    /// `nest`.
    Nest,
    /// `inreg`.
    InReg,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Param {
    pub ty: Ty,
    pub attr: Attr,
}

/// A parameter of type `ty` with no attribute.
pub fn p(ty: Ty) -> Param {
    Param { ty, attr: Attr::None }
}

/// A parameter of type `ty` with `attr`.
pub fn pa(ty: Ty, attr: Attr) -> Param {
    Param { ty, attr }
}

/// A `byval(pointee)` parameter.
pub fn byval(pointee: Ty, align: Option<u64>, stack_align: Option<u64>) -> Param {
    Param { ty: Ty::Ptr, attr: Attr::ByVal { pointee, align, stack_align } }
}

impl Param {
    /// The parameter's attributes in LLVM's syntax, with a leading space
    /// when there are any.
    pub fn attr_text(&self) -> String {
        match &self.attr {
            Attr::None => String::new(),
            Attr::Ext(false) => " zeroext".into(),
            Attr::Ext(true) => " signext".into(),
            Attr::StackAlign(n) => format!(" alignstack({n})"),
            Attr::Sret => " sret(i64)".into(),
            Attr::ByRef => " byref(i64)".into(),
            Attr::Nest => " nest".into(),
            Attr::InReg => " inreg".into(),
            Attr::ByVal { pointee, align, stack_align } => {
                let mut text = format!(" byval({})", pointee.text());
                if let Some(n) = align {
                    let _ = write!(text, " align {n}");
                }
                if let Some(n) = stack_align {
                    let _ = write!(text, " alignstack({n})");
                }
                text
            }
        }
    }

    fn attrs(&self, types: &mut TypeTable) -> ParamAttrs {
        let none = ParamAttrs::NONE;
        let made = match &self.attr {
            Attr::None => Ok(none),
            Attr::Ext(false) => none.with(ParamAttrs::ZEROEXT),
            Attr::Ext(true) => none.with(ParamAttrs::SIGNEXT),
            Attr::StackAlign(n) => none.with_stack_align(*n),
            Attr::Sret => none.with_mem(ParamMem::Sret, TypeId::I64),
            Attr::ByRef => none.with_mem(ParamMem::ByRef, TypeId::I64),
            Attr::Nest => none.with(ParamAttrs::NEST),
            Attr::InReg => none.with(ParamAttrs::INREG),
            Attr::ByVal { pointee, align, stack_align } => {
                let pointee = pointee.intern(types);
                let mut attrs = none.with_mem(ParamMem::ByVal, pointee);
                if let Some(n) = align {
                    attrs = attrs.and_then(|a| a.with_align(*n));
                }
                if let Some(n) = stack_align {
                    attrs = attrs.and_then(|a| a.with_stack_align(*n));
                }
                attrs
            }
        };
        made.unwrap_or_else(|error| panic!("{:?}: {error:?}", self.attr))
    }
}

/// One signature.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Case {
    pub cc: CallConv,
    pub ret: Option<Ty>,
    /// `zeroext` (false) or `signext` (true) on the return value.
    pub ret_ext: Option<bool>,
    pub params: Vec<Param>,
    /// The number of named parameters of a variadic signature.
    pub fixed: Option<usize>,
}

/// The `hod_cg::abi` form of a [`Case`].
#[derive(Clone, Debug)]
pub struct Lowered {
    pub cc: CallConv,
    pub ret: TypeId,
    pub ret_attrs: ParamAttrs,
    pub params: Vec<TypeId>,
    pub attrs: Vec<ParamAttrs>,
    pub fixed: u32,
    pub variadic: bool,
}

impl Lowered {
    pub fn sig(&self) -> Sig<'_> {
        Sig {
            cc: self.cc,
            ret: self.ret,
            ret_attrs: self.ret_attrs,
            params: &self.params,
            param_attrs: &self.attrs,
            fixed: self.fixed,
            variadic: self.variadic,
        }
    }
}

impl Case {
    /// A `ccc` signature returning `void`.
    pub fn new(params: Vec<Param>) -> Case {
        Case { cc: CallConv::C, ret: None, ret_ext: None, params, fixed: None }
    }

    /// A signature of plain parameters of these types.
    pub fn of(types: &[Ty]) -> Case {
        Case::new(types.iter().cloned().map(p).collect())
    }

    pub fn ret(mut self, ty: Ty) -> Case {
        self.ret = Some(ty);
        self
    }

    pub fn ret_ext(mut self, ty: Ty, signed: bool) -> Case {
        self.ret = Some(ty);
        self.ret_ext = Some(signed);
        self
    }

    pub fn variadic(mut self, fixed: usize) -> Case {
        self.fixed = Some(fixed);
        self
    }

    pub fn cc(mut self, cc: CallConv) -> Case {
        self.cc = cc;
        self
    }

    /// `ret (params)` in LLVM's syntax.
    pub fn text(&self) -> String {
        let mut text = String::new();
        if self.cc != CallConv::C {
            let _ = write!(text, "{} ", self.cc.keyword().unwrap_or("cc ?"));
        }
        text.push_str(&self.ret_text());
        text.push_str(" (");
        for (index, param) in self.params.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            if self.fixed == Some(index) {
                text.push_str("... ");
            }
            let _ = write!(text, "{}{}", param.ty.text(), param.attr_text());
        }
        if self.fixed == Some(self.params.len()) {
            text.push_str(if self.params.is_empty() { "..." } else { ", ..." });
        }
        text.push(')');
        text
    }

    /// The return type, with its attribute before it.
    pub fn ret_text(&self) -> String {
        let ty = self.ret.as_ref().map_or_else(|| "void".to_string(), Ty::text);
        match self.ret_ext {
            None => ty,
            Some(false) => format!("zeroext {ty}"),
            Some(true) => format!("signext {ty}"),
        }
    }

    /// The calling convention keyword with a trailing space, or nothing
    /// for `ccc`.
    pub fn cc_text(&self) -> String {
        if self.cc == CallConv::C {
            String::new()
        } else {
            format!("{} ", self.cc.keyword().expect("a named convention"))
        }
    }

    pub fn lower(&self, types: &mut TypeTable) -> Lowered {
        let ret = self.ret.as_ref().map_or(TypeId::VOID, |ty| ty.intern(types));
        let ret_attrs = match self.ret_ext {
            None => Ok(ParamAttrs::NONE),
            Some(false) => ParamAttrs::NONE.with(ParamAttrs::ZEROEXT),
            Some(true) => ParamAttrs::NONE.with(ParamAttrs::SIGNEXT),
        }
        .expect("a return attribute");
        Lowered {
            cc: self.cc,
            ret,
            ret_attrs,
            params: self.params.iter().map(|param| param.ty.intern(types)).collect(),
            attrs: self.params.iter().map(|param| param.attrs(types)).collect(),
            fixed: self.fixed.unwrap_or(self.params.len()) as u32,
            variadic: self.fixed.is_some(),
        }
    }

    /// The assignment of the signature, or the name it is refused with.
    pub fn assign(&self, types: &mut TypeTable) -> Result<Assignment, &'static str> {
        self.assign_with(types, Features::default())
    }

    /// The assignment under `features`, or the name it is refused with.
    pub fn assign_with(
        &self,
        types: &mut TypeTable,
        features: Features,
    ) -> Result<Assignment, &'static str> {
        let lowered = self.lower(types);
        let mut out = Assignment::default();
        assign(types, &lowered.sig(), features, &mut out).map_err(|unsupported| unsupported.0)?;
        Ok(out)
    }

    /// Whether an `i128` is among the parameters, in an aggregate or not.
    pub fn has_i128(&self) -> bool {
        fn has(ty: &Ty) -> bool {
            match ty {
                Ty::Int(128) => true,
                Ty::Arr(elem, _) => has(elem),
                Ty::Struct(fields) | Ty::Packed(fields) => fields.iter().any(has),
                _ => false,
            }
        }
        self.params.iter().any(|param| has(&param.ty))
    }

    pub fn has_sret(&self) -> bool {
        self.params.iter().any(|param| param.attr == Attr::Sret)
    }
}

/// The bytes of one argument, or of the return value, of a running case.
#[derive(Clone, Debug)]
pub struct Slot {
    /// The value; for a `byval` argument, the pointee.
    pub image: Image,
    /// Where `hodabi_in` holds `image.clean`, `image.dirty` and
    /// `image.memory`.
    pub clean: u64,
    pub dirty: u64,
    pub memory: u64,
    /// Where the consumer writes what arrived, in its output blob.
    pub out: u64,
    /// The width of a value that is one integer.
    pub int: Option<u32>,
    /// `zeroext` (false) or `signext` (true) on it.
    pub ext: Option<bool>,
}

impl Slot {
    /// The size in bytes of the value proper: for an integer, the bytes
    /// its bits need.
    pub fn value_bytes(&self) -> u32 {
        match self.int {
            Some(bits) => bits.div_ceil(8),
            None => self.image.size as u32,
        }
    }
}

/// The input blob and the size of each output blob, as cases are added.
#[derive(Clone, Default, Debug)]
pub struct Blobs {
    pub input: Vec<u8>,
    pub out_size: u64,
}

impl Blobs {
    fn put(&mut self, bytes: &[u8]) -> u64 {
        let at = self.input.len() as u64;
        self.input.extend_from_slice(bytes);
        self.input.resize(self.input.len().next_multiple_of(16), 0);
        at
    }

    fn slot(&mut self, image: Image, int: Option<u32>, ext: Option<bool>) -> Slot {
        let clean = self.put(&image.clean);
        let dirty = self.put(&image.dirty);
        let memory = self.put(&image.memory);
        let out = self.out_size;
        self.out_size += (image.size + SLACK).next_multiple_of(16) as u64;
        Slot { image, clean, dirty, memory, out, int, ext }
    }
}

/// A case ready to run: its assignment and its bytes.
#[derive(Clone, Debug)]
pub struct Plan {
    pub case: Case,
    pub asg: Assignment,
    pub args: Vec<Slot>,
    pub ret: Option<Slot>,
}

impl Plan {
    /// Plans `case` with random values.
    ///
    /// # Panics
    /// Panics if the signature is unsupported.
    pub fn new(case: &Case, types: &mut TypeTable, blobs: &mut Blobs, rng: &mut Rng) -> Plan {
        let asg = case
            .assign(types)
            .unwrap_or_else(|name| panic!("`{}` is unsupported: {name}", case.text()));
        let args = case
            .params
            .iter()
            .map(|param| match &param.attr {
                Attr::ByVal { pointee, .. } => {
                    let id = pointee.intern(types);
                    let size = types.alloc_size_of(id) as usize;
                    blobs.slot(Image::bytes(size, rng), None, None)
                }
                attr => {
                    let ext = if let Attr::Ext(signed) = attr { Some(*signed) } else { None };
                    blobs.slot(Image::random(types, &param.ty, rng), param.ty.int_bits(), ext)
                }
            })
            .collect();
        let ret = case
            .ret
            .as_ref()
            .map(|ty| blobs.slot(Image::random(types, ty, rng), ty.int_bits(), case.ret_ext));
        Plan { case: case.clone(), asg, args, ret }
    }

    /// The parts of argument `index`.
    pub fn parts_of(&self, index: usize) -> impl Iterator<Item = &Part> {
        self.asg.args.iter().filter(move |part| part.arg as usize == index)
    }
}

/// The attributes of an integer with `zeroext` (false), `signext` (true)
/// or neither.
fn ext_attrs(ext: Option<bool>) -> ParamAttrs {
    let flag = match ext {
        None => return ParamAttrs::NONE,
        Some(false) => ParamAttrs::ZEROEXT,
        Some(true) => ParamAttrs::SIGNEXT,
    };
    ParamAttrs::NONE.with(flag).expect("an extension attribute")
}

/// `spec/abi-x64.md` §7, the producer's duty: an integer of `bits` bits
/// with the extension attribute `ext`, an argument or (`ret`) the return
/// value, is extended, by sign or by zero, to this many bits; `None` when
/// nothing is owed.
pub fn produced_bits(bits: u32, ext: Option<bool>, ret: bool) -> Option<(bool, u32)> {
    produced_extension(bits, ext_attrs(ext), ret).map(|extension| (extension.signed, extension.to))
}

/// `spec/abi-x64.md` §7, what the consumer may assume: the integer
/// arrives extended, by sign or by zero, to this many bits (its own
/// width when nothing may be assumed).
pub fn assumed_bits(bits: u32, ext: Option<bool>, ret: bool, place: Place) -> (bool, u32) {
    assumed_extension(bits, ext_attrs(ext), ret, place)
        .map_or((false, bits), |extension| (extension.signed, extension.to))
}
