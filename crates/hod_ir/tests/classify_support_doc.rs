//! `docs/status/support.md` is generated from the support matrix
//! (`spec/ir.md` §8.10): this test regenerates it in memory and fails
//! when the checked-in file differs.
//!
//! After a change of the matrix, rewrite the file with
//! `HOD_BLESS=1 cargo test -p hod_ir --test classify_support_doc`.

use std::path::PathBuf;

use hod_ir::{SUPPORT_GROUPS, SUPPORT_ROWS, STRUCTURAL_RULES, support_markdown};

fn checked_in() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/status/support.md")
}

/// §8.10: the checked-in file is the generated text, byte for byte.
#[test]
fn the_checked_in_file_is_the_generated_one() {
    let generated = support_markdown();
    let path = checked_in();
    if std::env::var_os("HOD_BLESS").is_some_and(|value| value == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    if on_disk != generated {
        let line = on_disk
            .lines()
            .zip(generated.lines())
            .position(|(have, want)| have != want)
            .unwrap_or_else(|| on_disk.lines().count().min(generated.lines().count()));
        panic!(
            "{} is stale (first difference at line {}): the support matrix changed.\n\
             Regenerate it with: HOD_BLESS=1 cargo test -p hod_ir --test classify_support_doc",
            path.display(),
            line + 1
        );
    }
}

/// §8.10: the text is a function of the matrix alone, names every row
/// once in the table's order, and names every group and structural rule.
#[test]
fn the_generated_file_is_deterministic_and_complete() {
    let generated = support_markdown();
    assert_eq!(generated, support_markdown());
    assert!(generated.ends_with('\n') && !generated.ends_with("\n\n"));
    assert!(generated.lines().all(|line| line == line.trim_end()), "trailing blanks");

    let mut from = 0;
    for row in &SUPPORT_ROWS {
        let cell = format!("| `{}` |", row.name);
        let at = generated[from..].find(&cell).unwrap_or_else(|| panic!("{} out of order", row.name));
        from += at + cell.len();
        assert_eq!(generated.matches(&cell).count(), 1, "{}", row.name);
    }
    for (group, _) in SUPPORT_GROUPS {
        assert!(generated.contains(&format!("## `{group}`")), "{group}");
    }
    for rule in STRUCTURAL_RULES {
        assert!(generated.contains(&format!("| `{}` |", rule.name)), "{}", rule.name);
    }
    assert!(generated.contains("classify_support_doc"), "how to regenerate it");
}
