//! A chunk of `llc`'s code: for tests of module emission that need real
//! functions before `hod` has a target. Each function a module defines
//! gets the bytes `llc` compiled for it and `llc`'s relocations in them,
//! turned back into `SymRef`s (a relocation against a section symbol
//! names the module symbol at that offset).

#![allow(dead_code)]

use hod_cg::{Chunk, FrameDesc, FuncRecord, Reloc, SymRef};
use hod_ir::Module;
use hod_obj::{Arch, RelocKind};

use crate::elfview::Elf;

/// The chunk of every function `functions` lists (in that order), from
/// `llc`'s object `theirs` of the same module.
pub fn chunk_from(
    module: &Module,
    functions: &[hod_ir::SymId],
    theirs: &Elf,
    align: &dyn Fn(&str) -> u32,
) -> Chunk {
    let mut chunk = Chunk::new();
    for &sym in functions {
        let name = String::from_utf8_lossy(module.symbol(sym).name()).into_owned();
        let found = theirs
            .symbols
            .iter()
            .find(|s| s.name == name && s.info & 0xf == 2)
            .unwrap_or_else(|| panic!("llc has no function {name}"));
        let section = found.shndx as usize;
        let start = found.value;
        let end = found.value + found.size;
        let code = &theirs.data(section)[start as usize..end as usize];
        let code_start = chunk.code.len() as u32;
        chunk.code.extend_from_slice(code);
        let relocs_start = chunk.relocs.len() as u32;
        for (offset, kind, symbol, addend) in theirs.relocs_in(section) {
            if offset < start || offset >= end {
                continue;
            }
            let (target, addend) = match symbol {
                Ok(name) => (name, addend),
                // A section symbol: the module symbol there.
                Err(target_section) => {
                    let at = theirs
                        .symbols
                        .iter()
                        .filter(|s| {
                            s.shndx as usize == target_section
                                && matches!(s.info & 0xf, 1 | 2 | 6)
                                && s.value as i64 <= addend
                                && (addend < (s.value + s.size.max(1)) as i64)
                        })
                        .max_by_key(|s| s.value)
                        .unwrap_or_else(|| panic!("{name}: no symbol at {addend} in a section"));
                    (at.name.clone(), addend - at.value as i64)
                }
            };
            let target = target.strip_prefix(".L").unwrap_or(&target);
            let sym = module
                .find_symbol(target.as_bytes())
                .unwrap_or_else(|| panic!("{name}: llc refers to {target}, not a module symbol"));
            chunk.relocs.push(Reloc {
                offset: (offset - start) as u32,
                kind: RelocKind::from_elf_type(Arch::AArch64, kind),
                sym: SymRef::Module(sym),
                addend,
            });
        }
        let empty = 0..0;
        chunk.funcs.push(FuncRecord {
            sym,
            align: align(&name),
            frame: FrameDesc::default(),
            has_pads: false,
            code: code_start..chunk.code.len() as u32,
            relocs: relocs_start..chunk.relocs.len() as u32,
            cfi: empty.clone(),
            call_sites: empty.clone(),
            pads: empty.clone(),
            clauses: empty.clone(),
            type_ids: empty.clone(),
            filter_ids: empty.clone(),
            lines: empty.clone(),
            vars: empty.clone(),
            islands: empty.clone(),
            block_offsets: empty,
        });
    }
    chunk
}
