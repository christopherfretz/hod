//! Address-taken blocks in a function without an `indirectbr` (WP-115,
//! `spec/codegen.md` §9.6, §6.1): `mark_reachable` records the
//! function's `block_offsets` from `hod_ir`'s address-taken mark, so a
//! `blockaddress` in data of such a block is placed, and binds only the
//! unreachable address-taken blocks to the trap.
//!
//! The run: `@f` stores the addresses of two of its blocks through a
//! pointer (formed in code, from labels) while a table in data holds
//! the same addresses (from `block_offsets`); a C driver compares them.
//! `llc` deletes the unreachable block and writes `1` for its address in
//! code and in data; hod writes the trap's address in both. Either way
//! code and data agree, the address is not null and differs from every
//! reachable block's, so the program prints the same with either object.

mod common;

use common::linking::{Toolchain, exit_code};
use common::{emit, ll_header, llc, objdump, parse, write_object};
use hod_cg::Options;
use hod_testutil::Scratch;

const PROGRAM: &str = r"
@tab = global [3 x ptr] [ptr blockaddress(@f, %a), ptr blockaddress(@f, %dead), ptr blockaddress(@f, %b)]

define i32 @f(i32 %x, ptr %out) {
entry:
  store ptr blockaddress(@f, %a), ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr blockaddress(@f, %dead), ptr %o1
  %o2 = getelementptr ptr, ptr %out, i64 2
  store ptr blockaddress(@f, %b), ptr %o2
  %c = icmp eq i32 %x, 0
  br i1 %c, label %a, label %b
a:
  ret i32 1
b:
  ret i32 2
dead:
  ret i32 3
}
";

const DRIVER: &str = r#"
#include <stdio.h>
extern void *tab[3];
int f(int, void **);
int main(void) {
  void *code[3];
  int r0 = f(0, code), r1 = f(1, code);
  printf("%d %d", r0, r1);
  for (int i = 0; i < 3; i++) printf(" %d", code[i] == tab[i]);
  printf(" %d %d %d\n", tab[1] != 0, tab[1] != tab[0] && tab[1] != tab[2], tab[0] != tab[2]);
  return 0;
}
"#;

// §9.6: the table is placed, and agrees with the code's own addresses,
// under every linker, plain and stressed, as with `llc`'s object.
#[test]
fn block_addresses_in_data_without_an_indirectbr_run() {
    let text = format!("{}{PROGRAM}", ll_header(22));
    let (module, bodies) = parse(&text);
    let dir = Scratch::new("a64-block-address-taken");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &["-w"]);
    let mut objects = Vec::new();
    for (tag, stress) in [("hod", hod_cg::Stress::default()), ("stress", hod_cg::Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        objects.push((tag, write_object(&dir, &format!("{tag}.o"), &emit(&module, &bodies, &options))));
    }
    if let Some(object) = llc(&dir, "llc", &text, &[]) {
        objects.push(("llc", object));
    }
    for (tag, object) in &objects {
        for linker in &toolchain.linkers {
            let exe = dir.join(format!("{tag}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &exe);
            assert_eq!(exit_code(&ran), Some(0), "{tag} {}", linker.name);
            assert_eq!(ran.stdout_text(), "1 2 1 1 1 1 1 1\n", "{tag} {}", linker.name);
        }
    }
}

// §6.1: without an `indirectbr`, an unreachable block whose address is
// not taken gets no trap (as before); one whose address is taken does.
#[test]
fn only_an_address_taken_unreachable_block_gets_the_trap() {
    let traps = |text: &str| -> usize {
        let text = format!("{}{text}", ll_header(22));
        let (module, bodies) = parse(&text);
        let dir = Scratch::new("a64-block-address-trap");
        let object = write_object(&dir, "t.o", &emit(&module, &bodies, &Options::default()));
        let Some(lines) = objdump(&object) else { return usize::MAX };
        lines.iter().filter(|line| line.starts_with("brk")).count()
    };
    let function = |taken: &str| {
        format!(
            "@tab = global [1 x ptr] [ptr blockaddress(@g, {taken})]\n\
             define i32 @g(i32 %x) {{\nentry:\n  br label %a\na:\n  ret i32 1\n\
             dead:\n  ret i32 3\n}}\n"
        )
    };
    let (live, dead) = (traps(&function("%a")), traps(&function("%dead")));
    if live == usize::MAX || dead == usize::MAX {
        return;
    }
    assert_eq!((live, dead), (0, 1));
}
