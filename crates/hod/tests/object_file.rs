//! `hod::compile_object` (`spec/cli.md` "The facade", WP-312): the
//! object answered unwritten writes, through any sink, the bytes that
//! `hod::compile_releasing` returns for the same compile, and answers the
//! same refusals. Over every module of `tests/ir`, for every target
//! (i686 included), at one thread and at several.

use std::borrow::Cow;
use std::io::Write;
use std::path::PathBuf;

use hod::{CompileError, Options, OutputKind, RelocModel, Report, Settings, Threads};

const TRIPLES: [&str; 3] =
    ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu", "i686-unknown-linux-gnu"];

/// A sink that takes `step` bytes per call, and fails once `room` bytes
/// are taken.
struct Trickle {
    bytes: Vec<u8>,
    step: usize,
    room: usize,
}

impl Write for Trickle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() >= self.room {
            return Err(std::io::Error::other("full"));
        }
        let take = buf.len().min(self.step).min(self.room - self.bytes.len());
        self.bytes.extend_from_slice(&buf[..take]);
        Ok(take)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn seeds() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ir");
    let mut seeds: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/ir")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "ll"))
        .collect();
    seeds.sort();
    seeds
}

fn text(error: &CompileError) -> String {
    format!("{error:?}")
}

#[test]
fn compile_object_writes_the_bytes_of_compile_releasing() {
    let mut compiled = 0;
    let mut refused = 0;
    for seed in seeds() {
        let input = std::fs::read(&seed).unwrap();
        for triple in TRIPLES {
            for threads in [1, 4] {
                let options = Options {
                    triple: Some(triple.to_owned()),
                    relocation_model: Some(RelocModel::Pic),
                    output_kind: OutputKind::Object,
                    ..Options::default()
                };
                let settings = Settings {
                    threads: Threads { max: threads, per_thread: 1, ..Threads::default() },
                    ..Settings::default()
                };
                let what = format!("{} {triple} threads={threads}", seed.display());
                let mut report = Report::default();
                let bytes = hod::compile_releasing(
                    &mut Cow::Borrowed(&input),
                    &options,
                    &settings,
                    &mut report,
                );
                let mut report = Report::default();
                // Owned, as the tool gives it: freed once read.
                let mut owned = Cow::Owned(input.clone());
                let object = hod::compile_object(&mut owned, &options, &settings, &mut report);
                let (bytes, object) = match (bytes, object) {
                    (Ok(bytes), Ok(object)) => (bytes, object),
                    (Err(a), Err(b)) => {
                        assert_eq!(text(&a), text(&b), "{what}");
                        // A refusal, never a bug of the write path (an
                        // i386 object once panicked there, asked its `Arch`).
                        assert!(!matches!(b, CompileError::Panic(_)), "{what}: {b:?}");
                        refused += 1;
                        continue;
                    }
                    (bytes, object) => {
                        panic!("{what}: {:?} against {:?}", bytes.err(), object.err())
                    }
                };
                assert!(owned.is_empty(), "{what}: the owned input is freed");
                assert_eq!(object.size(), bytes.len() as u64, "{what}");
                assert_eq!(object.to_vec().unwrap(), bytes, "{what}");
                let mut whole = Vec::new();
                assert_eq!(object.write_to(&mut whole).unwrap().unwrap(), bytes.len() as u64);
                assert!(whole == bytes, "{what}: write_to differs from compile_releasing");
                let mut trickle = Trickle { bytes: Vec::new(), step: 4093, room: usize::MAX };
                object.write_to(&mut trickle).unwrap().unwrap();
                assert!(trickle.bytes == bytes, "{what}: a trickling sink differs");
                // A sink that fails part way: an I/O error, not a panic,
                // after the bytes it took.
                let room = bytes.len() / 2;
                let mut full = Trickle { bytes: Vec::new(), step: usize::MAX, room };
                assert!(object.write_to(&mut full).unwrap().is_err(), "{what}");
                assert_eq!(full.bytes, bytes[..room], "{what}");
                compiled += 1;
            }
        }
    }
    // Most seeds compile for both 64-bit targets.
    assert!(compiled >= 200, "{compiled} compiled, {refused} refused");
}

/// The ELFCLASS32 object through every write path of the facade: a
/// vector, a whole sink, a trickling sink and a file, each the bytes of
/// `hod::compile` (the whole-buffer path), header class and machine
/// checked.
#[test]
fn an_i386_object_is_written_through_every_path() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/ir");
    let input = std::fs::read(dir.join("odd_width_store.ll")).unwrap();
    let options = Options {
        triple: Some("i686-unknown-linux-gnu".to_owned()),
        relocation_model: Some(RelocModel::Static),
        output_kind: OutputKind::Object,
        ..Options::default()
    };
    let expected = hod::compile(&input, &options).unwrap();
    assert_eq!(expected[4], 1, "ELFCLASS32");
    assert_eq!(u16::from_le_bytes([expected[18], expected[19]]), 3, "EM_386");
    let mut report = Report::default();
    let object =
        hod::compile_object(&mut Cow::Owned(input), &options, &Settings::default(), &mut report)
            .unwrap();
    assert_eq!(object.size(), expected.len() as u64);
    assert!(object.to_vec().unwrap() == expected, "to_vec");
    let mut whole = Vec::new();
    object.write_to(&mut whole).unwrap().unwrap();
    assert!(whole == expected, "write_to");
    let mut trickle = Trickle { bytes: Vec::new(), step: 4093, room: usize::MAX };
    object.write_to(&mut trickle).unwrap().unwrap();
    assert!(trickle.bytes == expected, "a trickling sink");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("i386-object-file.o");
    let mut file = std::fs::File::create(&path).unwrap();
    object.write_to(&mut file).unwrap().unwrap();
    file.flush().unwrap();
    drop(file);
    assert!(std::fs::read(&path).unwrap() == expected, "a file");
    std::fs::remove_file(&path).unwrap();
}
