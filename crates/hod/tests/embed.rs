//! `spec/cli.md` "Embedding": the version of the producer surface is the
//! spec's, and what a program that compiles in process relies on of the
//! session's type.

use std::path::PathBuf;

// "Embedding": `EMBED_API` is the last row of the spec's table, so that a
// bump and its line land together.
#[test]
fn embed_api_is_the_last_row_of_the_table() {
    let spec = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/cli.md");
    let spec = std::fs::read_to_string(spec).expect("spec/cli.md");
    let section = spec.split("\n## Embedding\n").nth(1).expect("the section");
    let section = section.split("\n## ").next().unwrap_or_default();
    let header = "| `EMBED_API` | since | the change |";
    let table = section.split(header).nth(1).expect("the table");
    let rows: Vec<u32> = table
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| line.trim_start_matches('|').split('|').next().unwrap().trim().parse())
        .collect::<Result<_, _>>()
        .expect("each row starts with a version");
    let expected: Vec<u32> = (1..=hod::EMBED_API).collect();
    assert_eq!(rows, expected, "one row per version, the last EMBED_API");
}

// "Embedding", "Threads and re-entrancy": a session moves between
// threads.
#[test]
fn a_session_is_send() {
    fn send<T: Send>() {}
    send::<hod::Session>();
    send::<hod::Settings>();
    send::<hod::Report>();
    send::<hod::CompileError>();
}
