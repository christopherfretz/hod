//! `spec/asm.md` §5.4 (WP-188): the system registers of `mrs` and `msr`.
//! The table is `llvm-mc-22`'s: regenerated from the tool and compared;
//! every name and access in it, and the generic form at its edges,
//! encodes to `llvm-mc`'s words; what `llvm-mc` refuses is refused.

use std::fmt::Write as _;

use hod_asm::a64::sysreg::{self, EXTRA, RAND, READ, TABLE, WRITE};
use hod_asm::a64::{Code, Features, assemble};
use hod_testutil::{Scratch, run, tool};

const ALL: Features = Features { lse: true, crc: true, fp: true, rand: true };

fn hod(body: &str, features: Features) -> Result<Vec<u32>, &'static str> {
    let mut code = Code::default();
    assemble(body, features, &mut code, &mut String::new()).map_err(|r| r.0)?;
    Ok(code.words)
}

/// `llvm-mc-22`'s stdout for `source`, assembled (`--show-encoding`) or
/// disassembled; errors stay on stderr.
fn mc(scratch: &Scratch, name: &str, source: &str, args: &[&str]) -> Option<String> {
    let mc = tool("llvm-mc-22")?;
    let src = scratch.write(name, source);
    let mut all: Vec<&std::ffi::OsStr> = vec!["--triple=aarch64-unknown-linux-gnu".as_ref()];
    all.extend(args.iter().map(std::ffi::OsStr::new));
    all.push(src.as_os_str());
    Some(run(&mc, all).stdout_text())
}

/// The `(lower-case operand name, word)` of each instruction line of
/// `llvm-mc --show-encoding` output: the operand that is not `x0`.
fn encoded(out: &str) -> Vec<(String, u32)> {
    let mut found = Vec::new();
    for line in out.lines() {
        let Some((text, enc)) = line.split_once("// encoding: [") else { continue };
        let bytes: Vec<u8> = enc
            .trim_end_matches(']')
            .split(',')
            .map(|b| u8::from_str_radix(b.trim().trim_start_matches("0x"), 16).unwrap())
            .collect();
        let word = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let name = text
            .split([' ', '\t', ','])
            .filter(|w| !w.is_empty())
            .nth(if text.contains("mrs") { 2 } else { 1 })
            .unwrap()
            .to_ascii_lowercase();
        found.push((name, word));
    }
    found
}

/// The table as `llvm-mc-22` says it: the named registers of the ID
/// space (from the disassembler) and of [`EXTRA`], with the accesses it
/// accepts with and without `+rand`.
fn regenerate(scratch: &Scratch) -> Option<Vec<(String, u16, u8)>> {
    let mut words = String::new();
    for crm in 0..8u16 {
        for op2 in 0..8u16 {
            let w = sysreg::mrs(0, sysreg::field(3, 0, 0, crm, op2));
            for b in w.to_le_bytes() {
                write!(words, "0x{b:02x} ").unwrap();
            }
            words.push('\n');
        }
    }
    let dis = mc(scratch, "id.txt", &words, &["--disassemble"])?;
    let mut names: Vec<String> = dis
        .lines()
        .filter_map(|l| l.split_once("x0,").map(|(_, n)| n.trim().to_ascii_lowercase()))
        .filter(|n| sysreg::generic(n).is_none())
        .collect();
    names.extend(EXTRA.iter().map(|s| (*s).to_string()));
    let mut reads = String::new();
    let mut writes = String::new();
    for n in &names {
        writeln!(reads, "mrs x0, {n}").unwrap();
        writeln!(writes, "msr {n}, x0").unwrap();
    }
    let field = |w: u32| ((w >> 5) & 0xffff) as u16;
    let mut table: Vec<(String, u16, u8)> = Vec::new();
    for (feature, mattr) in [(0, "-mattr=+fp-armv8"), (RAND, "-mattr=+rand")] {
        for (access, src) in [(READ, &reads), (WRITE, &writes)] {
            let out = mc(scratch, "names.s", src, &["--show-encoding", mattr])?;
            for (name, word) in encoded(&out) {
                if !names.contains(&name) {
                    continue;
                }
                match table.iter_mut().find(|e| e.0 == name) {
                    Some(e) => {
                        assert_eq!(e.1, field(word), "{name}");
                        e.2 |= access;
                    }
                    None => table.push((name, field(word), access | feature)),
                }
            }
        }
    }
    table.sort();
    Some(table)
}

#[test]
fn sysreg_table_is_llvm_mc_s() {
    let scratch = Scratch::new("hod-asm-sysreg");
    let Some(theirs) = regenerate(&scratch) else { return };
    let ours: Vec<(String, u16, u8)> =
        TABLE.iter().map(|&(n, f, b)| (n.to_string(), f, b)).collect();
    if ours != theirs {
        let mut text = String::new();
        for (n, f, b) in &theirs {
            let mut bits = vec![];
            for (bit, s) in [(READ, "READ"), (WRITE, "WRITE"), (RAND, "RAND")] {
                if b & bit != 0 {
                    bits.push(s);
                }
            }
            writeln!(text, "    (\"{n}\", 0x{f:04x}, {}),", bits.join(" | ")).unwrap();
        }
        panic!("the table is not llvm-mc-22's; regenerated:\n{text}");
    }
    println!("hod_asm: {} system registers equal llvm-mc-22's table", ours.len());
}

/// Every row in every access it allows, in upper and lower case, and
/// the generic form at the edges of each field, encode as `llvm-mc-22`.
#[test]
fn every_row_and_the_generic_form_encode_as_llvm_mc() {
    let mut bodies = Vec::new();
    for &(name, _, bits) in TABLE {
        if bits & READ != 0 {
            bodies.push(format!("mrs x3, {name}"));
            bodies.push(format!("mrs xzr, {}", name.to_ascii_uppercase()));
        }
        if bits & WRITE != 0 {
            bodies.push(format!("msr {name}, x30"));
            bodies.push(format!("msr {}, xzr", name.to_ascii_uppercase()));
        }
    }
    for op0 in 0..4 {
        for (op1, crn, crm, op2) in [(0, 0, 0, 0), (7, 15, 15, 7), (3, 14, 0, 2), (5, 9, 6, 3)] {
            bodies.push(format!("mrs x7, s{op0}_{op1}_c{crn}_c{crm}_{op2}"));
            bodies.push(format!("msr S{op0}_{op1}_C{crn}_C{crm}_{op2}, x9"));
        }
    }
    let mut ours = Vec::new();
    let mut source = String::new();
    for body in &bodies {
        let words = hod(body, ALL).unwrap_or_else(|e| panic!("{body:?} refused: {e}"));
        assert_eq!(words.len(), 1, "{body}");
        ours.push(words[0]);
        writeln!(source, "{body}").unwrap();
    }
    let scratch = Scratch::new("hod-asm-sysreg-enc");
    let Some(out) = mc(&scratch, "all.s", &source, &["--show-encoding", "-mattr=+rand"]) else {
        return;
    };
    let theirs: Vec<u32> = encoded(&out).into_iter().map(|(_, w)| w).collect();
    assert_eq!(theirs.len(), ours.len(), "llvm-mc-22 refused some of:\n{source}");
    let mut report = String::new();
    for ((body, a), b) in bodies.iter().zip(&ours).zip(&theirs) {
        if a != b {
            writeln!(report, "  {body}: hod {a:08x}, llvm-mc {b:08x}").unwrap();
        }
    }
    assert!(report.is_empty(), "llvm-mc-22 disagrees:\n{report}");
    println!("hod_asm: {} system-register bodies equal llvm-mc-22's", bodies.len());
}

/// What `llvm-mc` refuses is refused by name.
#[test]
fn refusals() {
    let base = Features { rand: false, ..ALL };
    for (body, features, name) in [
        ("mrs x0, midr_el2", ALL, "asm:operand/sysreg"),
        ("mrs x0, frob", ALL, "asm:operand/sysreg"),
        ("msr cntvct_el0, x0", ALL, "asm:operand/sysreg"),
        ("msr midr_el1, x0", ALL, "asm:operand/sysreg"),
        ("msr rndr, x0", ALL, "asm:operand/sysreg"),
        ("mrs x0, s4_0_c0_c0_0", ALL, "asm:operand/sysreg"),
        ("mrs x0, s3_0_c16_c0_0", ALL, "asm:operand/sysreg"),
        ("mrs x0, s3_0_c00_c0_0", ALL, "asm:operand/sysreg"),
        ("mrs x0, id_aa64zfr0_el1", ALL, "asm:operand/sysreg"),
        ("mrs x0, tco", ALL, "asm:operand/sysreg"),
        ("msr daifset, #3", ALL, "asm:operand/sysreg"),
        ("msr spsel, #1", ALL, "asm:operand/sysreg"),
        ("mrs x0, rndr", base, "asm:feature"),
        ("mrs w0, midr_el1", ALL, "asm:operand/register"),
        ("mrs sp, nzcv", ALL, "asm:operand/register"),
        ("mrs x0", ALL, "asm:operand/form"),
    ] {
        assert_eq!(hod(body, features).err(), Some(name), "{body}");
    }
    assert!(hod("mrs x0, rndr", Features::from_attribute("+v8a,+rand")).is_ok());
    assert!(hod("mrs x0, s3_3_c2_c4_0", base).is_ok(), "the generic form needs no feature");
}
