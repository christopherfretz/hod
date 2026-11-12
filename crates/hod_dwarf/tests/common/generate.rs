//! A generated `-g` module for the `llc` oracle (`spec/dwarf.md` §8.2):
//! many files in several directories, rows switching files inside a
//! function, long sequences, large address and line jumps in both
//! directions, explicit line 0, COMDAT functions.

use std::fmt::Write;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// The module's text: `functions` functions of about `rows` located
/// instructions each (the last one `long` times longer).
pub fn module(functions: usize, rows: usize, long: usize) -> String {
    let mut r = Lcg(0x5eed);
    let mut body = String::new();
    let mut meta = String::new();
    // !0 unit, !1 primary file, !2 subroutine type, !3.. files.
    let files = 24;
    let file_id = |k: usize| 3 + k;
    for k in 0..files {
        let (name, dir) = match k % 4 {
            0 => (format!("src/part{k}.c"), "/gen".to_string()),
            1 => (format!("part{k}.h"), "/gen".to_string()),
            2 => (format!("inc{}/part{k}.h", k % 3), format!("/usr/include/x{}", k % 2)),
            _ => (format!("./sub/part{k}.inc"), "/gen".to_string()),
        };
        writeln!(meta, "!{} = !DIFile(filename: \"{name}\", directory: \"{dir}\")", file_id(k))
            .unwrap();
    }
    let mut next = 3 + files;
    for f in 0..functions {
        let comdat = f % 5 == 3;
        let sp = next;
        let mut scopes = vec![sp];
        let home = r.below(files as u64) as usize;
        let start_line = 10 + r.below(5000) as u32;
        writeln!(
            meta,
            "!{sp} = distinct !DISubprogram(name: \"f{f}\", scope: !{0}, file: !{0}, line: {start_line}, type: !2, scopeLine: {start_line}, spFlags: DISPFlagDefinition, unit: !0)",
            file_id(home)
        )
        .unwrap();
        next += 1;
        // Two other files' scopes inside the function.
        for _ in 0..2 {
            let other = r.below(files as u64) as usize;
            writeln!(
                meta,
                "!{next} = !DILexicalBlockFile(scope: !{sp}, file: !{}, discriminator: 0)",
                file_id(other)
            )
            .unwrap();
            scopes.push(next);
            next += 1;
        }
        if comdat {
            writeln!(body, "$f{f} = comdat any").unwrap();
        }
        let linkage = if comdat { "linkonce_odr " } else { "" };
        let comdat_attr = if comdat { " comdat" } else { "" };
        writeln!(body, "define {linkage}void @f{f}(ptr %p) #0{comdat_attr} !dbg !{sp} {{").unwrap();
        let count = if f == functions - 1 { rows * long } else { rows };
        let mut line = i64::from(start_line);
        let mut v = 0;
        for _ in 0..count {
            let jump = match r.below(16) {
                0 => 1000 + r.below(100_000) as i64,
                1 => -(r.below(800) as i64),
                2 => -5,
                3 => 8,
                4 => 9,
                5 => -6,
                6..=10 => 1,
                _ => 0,
            };
            line = (line + jump).clamp(1, 1 << 30);
            let scope = scopes[if r.below(6) == 0 { 1 + r.below(2) as usize } else { 0 }];
            let shown = if r.below(40) == 0 { 0 } else { line };
            let column = r.below(90);
            let loc = next;
            next += 1;
            writeln!(
                meta,
                "!{loc} = !DILocation(line: {shown}, column: {column}, scope: !{scope})"
            )
            .unwrap();
            // Mostly one access per location; sometimes a long run, so that
            // address advances need DW_LNS_advance_pc.
            let repeat = if r.below(25) == 0 { 40 + r.below(80) } else { 1 + r.below(3) };
            for _ in 0..repeat {
                writeln!(body, "  %v{v} = load volatile i32, ptr %p, align 4, !dbg !{loc}")
                    .unwrap();
                writeln!(body, "  store volatile i32 %v{v}, ptr %p, align 4, !dbg !{loc}").unwrap();
                v += 1;
            }
        }
        let ret = next;
        next += 1;
        writeln!(meta, "!{ret} = !DILocation(line: {}, column: 1, scope: !{sp})", line.max(1))
            .unwrap();
        writeln!(body, "  ret void, !dbg !{ret}\n}}\n").unwrap();
    }
    let mut out = String::from("source_filename = \"gen.c\"\n\n");
    out.push_str(&body);
    out.push_str("attributes #0 = { noinline nounwind optnone }\n\n");
    out.push_str("!llvm.dbg.cu = !{!0}\n!llvm.module.flags = !{!900000, !900001}\n");
    out.push_str("!900000 = !{i32 7, !\"Dwarf Version\", i32 5}\n");
    out.push_str("!900001 = !{i32 2, !\"Debug Info Version\", i32 3}\n");
    out.push_str("!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: \"gen\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, nameTableKind: None)\n");
    out.push_str("!1 = !DIFile(filename: \"gen.c\", directory: \"/gen\")\n");
    out.push_str("!2 = !DISubroutineType(types: !{null})\n");
    out.push_str(&meta);
    out
}
