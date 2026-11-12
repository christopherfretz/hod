//! A reader of the unwind tables `emit_module` writes (`spec/eh.md` §2,
//! §3, §9), for tests: `.eh_frame` back to CIEs and FDEs with their rows,
//! and an LSDA back to its call sites, action chains, type table and
//! filters. Relocated fields are read through a lookup the caller
//! supplies, so the same reader serves a `hod_obj::Object` and `llc`'s
//! ELF file. It decodes what `hod_eh` and `llc` write for AArch64 and
//! x86-64; anything else is an error.

use hod_cg::{CfiOp, CfiRow};
use hod_obj::{Object, SectionId, SymbolId, SymbolKind};

/// What a relocated field refers to: a symbol (by id, or by name for an
/// ELF file) and the addend.
pub type Target<R> = (R, i64);

/// A CIE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cie<R> {
    pub offset: usize,
    pub augmentation: String,
    pub code_align: u64,
    pub data_align: i64,
    pub return_reg: u64,
    pub personality: Option<Target<R>>,
    pub personality_encoding: u8,
    pub lsda_encoding: u8,
    pub fde_encoding: u8,
    pub initial: Vec<CfiRow>,
}

/// A decoded `.eh_frame`: its CIEs and FDEs.
pub type Frames<R> = (Vec<Cie<R>>, Vec<FdeView<R>>);

/// An FDE, its rows decoded into `hod_cg`'s row type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FdeView<R> {
    pub offset: usize,
    /// The index of its CIE.
    pub cie: usize,
    pub start: Target<R>,
    pub size: u32,
    pub lsda: Option<Target<R>>,
    pub rows: Vec<CfiRow>,
    /// The instructions used `DW_CFA_remember_state`, which the rows
    /// restate (`spec/eh.md` §4.3).
    pub remembered: bool,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let byte = *self.data.get(self.at).ok_or("past the end")?;
        self.at += 1;
        Ok(byte)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.data.get(self.at..self.at + 4).ok_or("past the end")?;
        self.at += 4;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn uleb(&mut self) -> Result<u64, String> {
        let (mut value, mut shift) = (0u64, 0);
        loop {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
    }
    fn sleb(&mut self) -> Result<i64, String> {
        let (mut value, mut shift) = (0i64, 0);
        loop {
            let byte = self.u8()?;
            value |= i64::from(byte & 0x7f) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if shift < 64 && byte & 0x40 != 0 {
                    value |= -1i64 << shift;
                }
                return Ok(value);
            }
        }
    }
}

/// The size of a pointer of encoding `encoding`.
fn encoded_size(encoding: u8) -> Result<usize, String> {
    match encoding & 0x0f {
        0x0b | 0x03 => Ok(4),
        0x0c | 0x04 | 0x00 => Ok(8),
        other => Err(format!("pointer format {other:#x}")),
    }
}

/// The rule state the instructions build, to restate a remembered one.
#[derive(Clone, Default)]
struct State {
    cfa: (u16, i32),
    saved: Vec<(u16, i32)>,
}

/// Decodes call-frame instructions into rows (`spec/eh.md` §4).
fn instructions(
    bytes: &[u8],
    code_align: u64,
    data_align: i64,
    initial: &State,
    rows: &mut Vec<CfiRow>,
) -> Result<bool, String> {
    let mut r = Reader { data: bytes, at: 0 };
    let mut offset = 0u32;
    let mut state = initial.clone();
    let mut stack: Vec<State> = Vec::new();
    let mut remembered = false;
    let daf = |f: i64| (f * data_align) as i32;
    let push = |rows: &mut Vec<CfiRow>, state: &mut State, offset: u32, op: CfiOp| {
        match op {
            CfiOp::DefCfa { reg, offset } => state.cfa = (reg, offset),
            CfiOp::DefCfaOffset(o) => state.cfa.1 = o,
            CfiOp::DefCfaRegister(reg) => state.cfa.0 = reg,
            CfiOp::Offset { reg, cfa_offset } => {
                state.saved.retain(|&(r, _)| r != reg);
                state.saved.push((reg, cfa_offset));
            }
            CfiOp::Restore { reg } => state.saved.retain(|&(r, _)| r != reg),
        }
        rows.push(CfiRow { offset, op });
    };
    while r.at < bytes.len() {
        let byte = r.u8()?;
        let (high, low) = (byte & 0xc0, u16::from(byte & 0x3f));
        let op = match (high, byte) {
            (0x40, _) => {
                offset += (u64::from(low) * code_align) as u32;
                continue;
            }
            (0x80, _) => CfiOp::Offset { reg: low, cfa_offset: daf(r.uleb()? as i64) },
            (0xc0, _) => CfiOp::Restore { reg: low },
            (_, 0x00) => continue,
            (_, 0x02) => {
                offset += u32::from(r.u8()?) * code_align as u32;
                continue;
            }
            (_, 0x03) => {
                let lo = r.u8()?;
                let hi = r.u8()?;
                offset += u32::from(u16::from_le_bytes([lo, hi])) * code_align as u32;
                continue;
            }
            (_, 0x04) => {
                offset += r.u32()? * code_align as u32;
                continue;
            }
            (_, 0x05) => {
                let reg = r.uleb()? as u16;
                CfiOp::Offset { reg, cfa_offset: daf(r.uleb()? as i64) }
            }
            (_, 0x06) => CfiOp::Restore { reg: r.uleb()? as u16 },
            (_, 0x0a) => {
                remembered = true;
                stack.push(state.clone());
                continue;
            }
            (_, 0x0b) => {
                let to = stack.pop().ok_or("restore_state without remember_state")?;
                // Restated as `spec/eh.md` §4.3 says hod states it.
                let (reg, off) = to.cfa;
                push(rows, &mut state, offset, CfiOp::DefCfa { reg, offset: off });
                for &(reg, _) in &state.saved.clone() {
                    if !to.saved.iter().any(|&(r, _)| r == reg) {
                        push(rows, &mut state, offset, CfiOp::Restore { reg });
                    }
                }
                for &(reg, cfa_offset) in &to.saved {
                    push(rows, &mut state, offset, CfiOp::Offset { reg, cfa_offset });
                }
                continue;
            }
            (_, 0x0c) => {
                let reg = r.uleb()? as u16;
                CfiOp::DefCfa { reg, offset: r.uleb()? as i32 }
            }
            (_, 0x0d) => CfiOp::DefCfaRegister(r.uleb()? as u16),
            (_, 0x0e) => CfiOp::DefCfaOffset(r.uleb()? as i32),
            (_, 0x11) => {
                let reg = r.uleb()? as u16;
                CfiOp::Offset { reg, cfa_offset: daf(r.sleb()?) }
            }
            (_, 0x12) => {
                let reg = r.uleb()? as u16;
                CfiOp::DefCfa { reg, offset: daf(r.sleb()?) }
            }
            (_, 0x13) => CfiOp::DefCfaOffset(daf(r.sleb()?)),
            (_, other) => return Err(format!("call-frame instruction {other:#x}")),
        };
        push(rows, &mut state, offset, op);
    }
    Ok(remembered)
}

/// Decodes a whole `.eh_frame`. `reloc_at(offset)` is the relocation at
/// that offset of the section, if any.
///
/// # Errors
/// What could not be read.
pub fn eh_frame<R: Clone>(
    data: &[u8],
    reloc_at: &dyn Fn(u64) -> Option<Target<R>>,
) -> Result<Frames<R>, String> {
    let mut cies: Vec<Cie<R>> = Vec::new();
    let mut fdes = Vec::new();
    let mut at = 0usize;
    while at < data.len() {
        let mut r = Reader { data, at };
        let length = r.u32()? as usize;
        if length == 0 {
            break;
        }
        let end = r.at + length;
        let id_at = r.at;
        let id = r.u32()?;
        if id == 0 {
            if r.u8()? != 1 {
                return Err(format!("CIE at {at}: version"));
            }
            let mut augmentation = String::new();
            loop {
                let c = r.u8()?;
                if c == 0 {
                    break;
                }
                augmentation.push(c as char);
            }
            let code_align = r.uleb()?;
            let data_align = r.sleb()?;
            let return_reg = u64::from(r.u8()?);
            let mut cie = Cie {
                offset: at,
                augmentation: augmentation.clone(),
                code_align,
                data_align,
                return_reg,
                personality: None,
                personality_encoding: 0xff,
                lsda_encoding: 0xff,
                fde_encoding: 0,
                initial: Vec::new(),
            };
            if augmentation.starts_with('z') {
                let len = r.uleb()? as usize;
                let aug_end = r.at + len;
                for c in augmentation.chars().skip(1) {
                    match c {
                        'P' => {
                            cie.personality_encoding = r.u8()?;
                            cie.personality = reloc_at(r.at as u64);
                            r.at += encoded_size(cie.personality_encoding)?;
                        }
                        'L' => cie.lsda_encoding = r.u8()?,
                        'R' => cie.fde_encoding = r.u8()?,
                        other => return Err(format!("augmentation {other}")),
                    }
                }
                r.at = aug_end;
            }
            instructions(
                &data[r.at..end],
                code_align,
                data_align,
                &State::default(),
                &mut cie.initial,
            )?;
            cies.push(cie);
        } else {
            let cie_at = id_at - id as usize;
            let cie = cies.iter().position(|c| c.offset == cie_at).ok_or("an FDE of no CIE")?;
            let start = reloc_at(r.at as u64).ok_or(format!("FDE at {at}: no relocation"))?;
            r.at += encoded_size(cies[cie].fde_encoding)?;
            let size = r.u32()?;
            let mut lsda = None;
            if cies[cie].augmentation.starts_with('z') {
                let len = r.uleb()? as usize;
                if cies[cie].augmentation.contains('L') && len > 0 {
                    lsda = reloc_at(r.at as u64);
                }
                r.at += len;
            }
            // The CIE's state is where the FDE's rows start.
            let mut initial = State::default();
            for row in &cies[cie].initial {
                match row.op {
                    CfiOp::DefCfa { reg, offset } => initial.cfa = (reg, offset),
                    CfiOp::Offset { reg, cfa_offset } => initial.saved.push((reg, cfa_offset)),
                    _ => {}
                }
            }
            let mut rows = Vec::new();
            let c = &cies[cie];
            let remembered =
                instructions(&data[r.at..end], c.code_align, c.data_align, &initial, &mut rows)?;
            fdes.push(FdeView { offset: at, cie, start, size, lsda, rows, remembered });
        }
        at = end;
    }
    Ok((cies, fdes))
}

/// One decoded LSDA (`spec/eh.md` §9.2 to §9.5).
#[derive(Clone, Debug)]
pub struct LsdaView<'a, R> {
    data: &'a [u8],
    /// Start, length, landing pad, action (0 for none; else the offset
    /// of the chain's first record plus one).
    pub sites: Vec<(u64, u64, u64, u64)>,
    /// Where the action table starts.
    actions: usize,
    /// The `TType` base, with a type table.
    pub ttbase: Option<usize>,
    pub ttype_encoding: u8,
    /// The type table's entries, by type id from 1: the target of each.
    pub types: Vec<Option<Target<R>>>,
}

impl<R: Clone> LsdaView<'_, R> {
    /// The type filters of the chain of `action`, in the order the
    /// personality visits them.
    ///
    /// # Errors
    /// A chain that leaves the table.
    pub fn chain(&self, action: u64) -> Result<Vec<i64>, String> {
        let mut out = Vec::new();
        if action == 0 {
            return Ok(out);
        }
        let mut at = self.actions + action as usize - 1;
        loop {
            let mut r = Reader { data: self.data, at };
            let filter = r.sleb()?;
            let disp_at = r.at;
            let disp = r.sleb()?;
            out.push(filter);
            if disp == 0 || out.len() > 1000 {
                return Ok(out);
            }
            at = (disp_at as i64 + disp) as usize;
        }
    }

    /// The type ids of the exception specification `filter` (< 0).
    ///
    /// # Errors
    /// A filter outside the table.
    pub fn filter(&self, filter: i64) -> Result<Vec<u64>, String> {
        let base = self.ttbase.ok_or("a filter without a type table")?;
        let mut r = Reader { data: self.data, at: base + (-filter - 1) as usize };
        let mut out = Vec::new();
        loop {
            let id = r.uleb()?;
            if id == 0 {
                return Ok(out);
            }
            out.push(id);
        }
    }
}

/// Decodes the LSDA at `at` of a section whose bytes are `data`.
/// `pointer` is the type table's entry size.
///
/// # Errors
/// What could not be read.
pub fn lsda<'a, R: Clone>(
    data: &'a [u8],
    at: usize,
    pointer: usize,
    reloc_at: &dyn Fn(u64) -> Option<Target<R>>,
) -> Result<LsdaView<'a, R>, String> {
    let mut r = Reader { data, at };
    if r.u8()? != 0xff {
        return Err("an @LPStart".to_owned());
    }
    let ttype_encoding = r.u8()?;
    let mut ttbase = None;
    if ttype_encoding != 0xff {
        let offset = r.uleb()? as usize;
        ttbase = Some(r.at + offset);
    }
    if r.u8()? != 0x01 {
        return Err("a call-site encoding other than uleb128".to_owned());
    }
    let length = r.uleb()? as usize;
    let end = r.at + length;
    let mut sites = Vec::new();
    while r.at < end {
        sites.push((r.uleb()?, r.uleb()?, r.uleb()?, r.uleb()?));
    }
    let actions = end;
    let mut types = Vec::new();
    if let Some(base) = ttbase {
        // The highest type id any chain names bounds the table.
        let mut view = LsdaView {
            data,
            sites: sites.clone(),
            actions,
            ttbase,
            ttype_encoding,
            types: Vec::new(),
        };
        let mut max = 0u64;
        for site in &sites {
            for filter in view.chain(site.3)? {
                if filter > 0 {
                    max = max.max(filter as u64);
                } else if filter < 0 {
                    for id in view.filter(filter)? {
                        max = max.max(id);
                    }
                }
            }
        }
        for k in 1..=max as usize {
            let field = base - k * pointer;
            types.push(reloc_at(field as u64));
        }
        view.types = types;
        return Ok(view);
    }
    Ok(LsdaView { data, sites, actions, ttbase, ttype_encoding, types })
}

/// The relocation at `offset` of `section` of an object: its symbol and
/// addend.
pub fn object_reloc(obj: &Object, section: SectionId, offset: u64) -> Option<Target<SymbolId>> {
    obj.relocations(section).find(|r| r.offset == offset).map(|r| (r.symbol, r.addend))
}

/// The section a section symbol stands for.
pub fn section_of(obj: &Object, symbol: SymbolId) -> Option<SectionId> {
    let s = obj.symbol(symbol);
    match (s.kind, s.place) {
        (SymbolKind::Section, hod_obj::SymbolPlace::Section(section)) => Some(section),
        _ => None,
    }
}

/// The section of an object named `name`.
pub fn find_section(obj: &Object, name: &[u8]) -> Option<SectionId> {
    (0..obj.section_count()).map(SectionId::from_index).find(|&s| obj.section_name(s) == name)
}
