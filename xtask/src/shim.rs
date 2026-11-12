//! `cargo xtask shim`: an LLVM installation directory whose `llc` is hod.
//! Contract: `spec/harness.md` "shim".
//!
//! kiln drives the `opt`, `llc` and `clang++` of one LLVM installation,
//! chosen with `--llvm-dir=<dir>`. The shim is such a directory: every entry
//! of the real installation is a symbolic link to the real one, except
//! `bin/llc`, a script that runs `hod` with the real `llc` as its fallback.
//! Beside it, `kiln++-hod` runs a given `kiln++` with `--llvm-dir=<shim>`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The file that marks a directory as made by this command.
pub const MARKER: &str = ".hod-shim";
/// The compiler wrapper's name.
pub const WRAPPER: &str = "kiln++-hod";
/// What kiln requires in an installation's `bin/`
/// (kiln `crates/kiln_driver/src/llvm.rs`, `REQUIRED_TOOLS`).
pub const REQUIRED_TOOLS: [&str; 3] = ["opt", "llc", "clang++"];
/// Prefix of the temporary files a script is written through.
const TEMP_PREFIX: &str = ".hod-tmp-";

/// What to build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The real LLVM installation (`/usr/lib/llvm-22`).
    pub llvm: PathBuf,
    /// The `hod` binary.
    pub hod: PathBuf,
    /// The directory to create or update.
    pub out: PathBuf,
    /// The `kiln++` to wrap; without it no wrapper is written.
    pub kiln: Option<PathBuf>,
}

/// What was built. All paths are absolute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shim {
    /// The shim directory: pass it to kiln as `--llvm-dir=`.
    pub dir: PathBuf,
    /// The real installation it mirrors.
    pub llvm: PathBuf,
    /// The LLVM major kiln will see.
    pub major: u32,
    /// `<dir>/bin/llc`.
    pub llc: PathBuf,
    /// The real `llc`, hod's fallback.
    pub real_llc: PathBuf,
    /// `<dir>/kiln++-hod`, if a kiln was given.
    pub compiler: Option<PathBuf>,
}

/// One thing the shim directory must contain.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    /// A symbolic link to this target.
    Link(PathBuf),
    /// A regular file with this content and mode.
    File(String, u32),
}

fn err(what: &str, path: &Path, e: &std::io::Error) -> String {
    format!("cannot {what} '{}': {e}", path.display())
}

/// `path` quoted for `sh`: in single quotes, a single quote written `'\''`.
/// A path that is not UTF-8 is an error (the scripts are text).
fn sh_quote(path: &Path) -> Result<String, String> {
    let text = path.to_str().ok_or_else(|| {
        format!("'{}' is not UTF-8; the shim's scripts cannot name it", path.display())
    })?;
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

/// The major of an installation as kiln tells it (kiln
/// `crates/kiln_driver/src/llvm.rs`, `dir_major`), by the two means that
/// carry over to a mirror of the directory: the one numeric entry of
/// `lib/clang/`, else the leading integer of `bin/llvm-config --version`.
fn mirrored_major(dir: &Path) -> Option<u32> {
    let mut numeric = fs::read_dir(dir.join("lib").join("clang"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()));
    if let (Some(major), None) = (numeric.next(), numeric.next()) {
        return Some(major);
    }
    let llvm_config = dir.join("bin").join("llvm-config");
    if !llvm_config.is_file() {
        return None;
    }
    let output = Command::new(llvm_config)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&output.stdout);
    let major = text.trim().split('.').next()?;
    if major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    major.parse().ok()
}

/// kiln's last resort: the `N` of a directory named `llvm-N`, `llvmN` or
/// `llvm@N`.
fn dir_name_major(dir: &Path) -> Option<u32> {
    let name = dir.file_name()?.to_str()?.strip_prefix("llvm")?;
    let digits = name.strip_prefix(['-', '@']).unwrap_or(name);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    std::path::absolute(path).map_err(|e| err("resolve", path, &e))
}

/// The entries of `dir` by name.
fn names(dir: &Path) -> Result<Vec<OsString>, String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| err("read", dir, &e))? {
        names.push(entry.map_err(|e| err("read", dir, &e))?.file_name());
    }
    names.sort();
    Ok(names)
}

/// Makes `path` the entry `want`, touching it only if it differs. Never
/// writes through a symbolic link, and never replaces something this
/// command would not have made.
fn ensure(path: &Path, want: &Entry) -> Result<(), String> {
    let existing = match fs::symlink_metadata(path) {
        Ok(meta) => Some(meta),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(err("inspect", path, &e)),
    };
    match want {
        Entry::Link(target) => {
            match existing {
                None => {}
                Some(meta) if meta.file_type().is_symlink() => {
                    if fs::read_link(path).is_ok_and(|current| current == *target) {
                        return Ok(());
                    }
                    fs::remove_file(path).map_err(|e| err("remove", path, &e))?;
                }
                Some(_) => {
                    return Err(format!(
                        "'{}' exists and is not a symbolic link; refusing to replace it",
                        path.display()
                    ));
                }
            }
            symlink(target, path).map_err(|e| err("create the link", path, &e))
        }
        Entry::File(content, mode) => {
            if let Some(meta) = &existing {
                if meta.is_dir() {
                    return Err(format!(
                        "'{}' is a directory; refusing to replace it",
                        path.display()
                    ));
                }
                let same = meta.is_file()
                    && meta.permissions().mode() & 0o777 == *mode
                    && fs::read(path).is_ok_and(|current| current == content.as_bytes());
                if same {
                    return Ok(());
                }
            }
            // Written beside the destination and renamed over it: a build
            // running at the same time never sees half a script.
            let dir = path.parent().unwrap_or(Path::new("."));
            let mut temp_name = OsString::from(format!("{TEMP_PREFIX}{}-", std::process::id()));
            temp_name.push(path.file_name().unwrap_or_default());
            let temp = dir.join(temp_name);
            fs::write(&temp, content).map_err(|e| err("write", &temp, &e))?;
            fs::set_permissions(&temp, fs::Permissions::from_mode(*mode))
                .map_err(|e| err("set the mode of", &temp, &e))?;
            fs::rename(&temp, path).map_err(|e| err("write", path, &e))
        }
    }
}

/// Makes the directory `dir` hold exactly `want`. Leftovers of an earlier
/// run (links, generated files) are removed; anything else is an error.
fn reconcile(dir: &Path, want: &BTreeMap<OsString, Entry>, keep: &[&str]) -> Result<(), String> {
    for name in names(dir)? {
        if want.contains_key(&name) || keep.iter().any(|k| name == *k) {
            continue;
        }
        let path = dir.join(&name);
        let meta = fs::symlink_metadata(&path).map_err(|e| err("inspect", &path, &e))?;
        let generated = name == WRAPPER
            || name == "llc"
            || name.to_str().is_some_and(|n| n.starts_with(TEMP_PREFIX));
        if meta.file_type().is_symlink() || (meta.is_file() && generated) {
            fs::remove_file(&path).map_err(|e| err("remove", &path, &e))?;
        } else {
            return Err(format!(
                "'{}' was not made by this command; refusing to touch the directory",
                path.display()
            ));
        }
    }
    for (name, entry) in want {
        ensure(&dir.join(name), entry)?;
    }
    Ok(())
}

/// The canonical form of a path that may not exist yet: its nearest existing
/// ancestor resolved, the rest appended.
fn canonical_prefix(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(resolved) = fs::canonicalize(existing) {
            return missing.iter().rev().fold(resolved, |p, part: &&OsStr| p.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The first lines of both generated scripts.
const HEADER: &str = "#!/bin/sh\n\
    # Generated by `cargo xtask shim` (hod, spec/harness.md \"shim\"). Do not edit.\n";

/// A checked request: everything [`build`] will write, decided before
/// anything is written.
struct Plan {
    llvm: PathBuf,
    out: PathBuf,
    major: u32,
    marker: String,
    /// The entries of `<out>/bin`.
    bin: BTreeMap<OsString, Entry>,
    /// The entries of `<out>`, without the marker and `bin`.
    top: BTreeMap<OsString, Entry>,
    has_wrapper: bool,
}

/// The LLVM installation of a request: resolved, and one kiln would accept.
fn real_installation(request: &Request) -> Result<PathBuf, String> {
    let llvm = fs::canonicalize(&request.llvm).map_err(|e| err("find", &request.llvm, &e))?;
    if !llvm.is_dir() {
        return Err(format!("'{}' is not a directory", llvm.display()));
    }
    if llvm.join(MARKER).exists() {
        return Err(format!(
            "'{}' is itself a hod shim; --llvm must name the real LLVM installation",
            llvm.display()
        ));
    }
    for tool in REQUIRED_TOOLS {
        if !llvm.join("bin").join(tool).is_file() {
            return Err(format!(
                "'{}' has no 'bin/{tool}'; kiln needs bin/opt, bin/llc and bin/clang++",
                llvm.display()
            ));
        }
    }
    Ok(llvm)
}

/// The major kiln must see in the mirror `out` of `llvm`.
fn required_major(llvm: &Path, out: &Path) -> Result<u32, String> {
    if let Some(major) = mirrored_major(llvm) {
        return Ok(major);
    }
    match (dir_name_major(llvm), dir_name_major(out)) {
        (Some(real), Some(shim)) if real == shim => Ok(real),
        (Some(real), _) => Err(format!(
            "the LLVM major of '{}' ({real}) is told only by its name; \
             name the --out directory 'llvm-{real}' so that kiln sees the same",
            llvm.display()
        )),
        (None, _) => Err(format!(
            "cannot tell the LLVM major of '{}' (no 'lib/clang/<N>', no working \
             'bin/llvm-config', not named 'llvm-<N>'); kiln would not accept it",
            llvm.display()
        )),
    }
}

fn plan(request: &Request) -> Result<Plan, String> {
    let llvm = real_installation(request)?;
    let hod = absolute(&request.hod)?;
    if !hod.is_file() {
        return Err(format!("'{}' is not a file (--hod names the hod binary)", hod.display()));
    }
    let kiln = request.kiln.as_deref().map(absolute).transpose()?;
    if let Some(kiln) = &kiln
        && !kiln.is_file()
    {
        return Err(format!("'{}' is not a file (--kiln names kiln++)", kiln.display()));
    }
    let out = absolute(&request.out)?;
    let resolved_out = canonical_prefix(&out);
    if resolved_out.starts_with(&llvm) || llvm.starts_with(&resolved_out) {
        return Err(format!(
            "--out '{}' and the LLVM installation '{}' must not contain one another",
            out.display(),
            llvm.display()
        ));
    }
    let major = required_major(&llvm, &out)?;

    let real_bin = llvm.join("bin");
    let llc_script = format!(
        "{HEADER}# hod with llc's command line; what hod does not support goes to the real llc.\n\
         HOD_FALLBACK_LLC={} exec {} \"$@\"\n",
        sh_quote(&real_bin.join("llc"))?,
        sh_quote(&hod)?
    );
    let mut bin = BTreeMap::new();
    for name in names(&real_bin)? {
        let entry = if name == "llc" {
            Entry::File(llc_script.clone(), 0o755)
        } else {
            Entry::Link(real_bin.join(&name))
        };
        bin.insert(name, entry);
    }
    let mut top = BTreeMap::new();
    for name in names(&llvm)? {
        if name == MARKER || name == WRAPPER {
            return Err(format!(
                "'{}' has an entry named '{}', which the shim needs for itself",
                llvm.display(),
                name.to_string_lossy()
            ));
        }
        if name != "bin" {
            top.insert(name.clone(), Entry::Link(llvm.join(&name)));
        }
    }
    if let Some(kiln) = &kiln {
        let script = format!(
            "{HEADER}# kiln++ with hod as its code generator.\n\
             exec {} --llvm-dir={} \"$@\"\n",
            sh_quote(kiln)?,
            sh_quote(&out)?
        );
        top.insert(OsString::from(WRAPPER), Entry::File(script, 0o755));
    }
    // The marker's content is the request: a directory this command made
    // says what it was made from.
    let marker = format!(
        "hod-shim 1\nllvm {}\nhod {}\nkiln {}\n",
        llvm.display(),
        hod.display(),
        kiln.as_ref().map_or("-".to_owned(), |kiln| kiln.display().to_string())
    );
    Ok(Plan { llvm, out, major, marker, bin, top, has_wrapper: kiln.is_some() })
}

/// Makes sure `dir` is a real directory this command may write into:
/// created now, or already there and passing `ours`.
fn claim(dir: &Path, ours: impl Fn(&fs::Metadata) -> bool, refusal: &str) -> Result<(), String> {
    match fs::symlink_metadata(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => {
            fs::create_dir_all(dir).map_err(|e| err("create", dir, &e))
        }
        Err(e) => Err(err("inspect", dir, &e)),
        Ok(meta) if ours(&meta) => Ok(()),
        Ok(_) => Err(format!("'{}' {refusal}", dir.display())),
    }
}

/// Creates or updates the shim directory. Idempotent: a second call with the
/// same request changes nothing.
///
/// # Errors
/// A message for the user: the request does not describe an LLVM
/// installation kiln would accept, or `out` exists and is not a shim
/// directory, or a file operation failed.
pub fn build(request: &Request) -> Result<Shim, String> {
    let plan = plan(request)?;
    let out = &plan.out;
    let marker = out.join(MARKER);
    claim(
        out,
        |meta| meta.is_dir() && fs::symlink_metadata(&marker).is_ok_and(|m| m.is_file()),
        &format!(
            "exists and was not made by this command (no '{MARKER}' file); \
             refusing to write into it"
        ),
    )?;
    ensure(&marker, &Entry::File(plan.marker.clone(), 0o644))?;
    let bin = out.join("bin");
    claim(&bin, fs::Metadata::is_dir, "is not a directory; refusing to write into it")?;
    reconcile(&bin, &plan.bin, &[])?;
    reconcile(out, &plan.top, &[MARKER, "bin"])?;

    let seen = mirrored_major(out).or_else(|| dir_name_major(out));
    if seen != Some(plan.major) {
        return Err(format!(
            "internal error: the shim '{}' shows LLVM major {seen:?}, the installation {}",
            out.display(),
            plan.major
        ));
    }
    Ok(Shim {
        llc: bin.join("llc"),
        real_llc: plan.llvm.join("bin").join("llc"),
        compiler: plan.has_wrapper.then(|| out.join(WRAPPER)),
        dir: plan.out.clone(),
        llvm: plan.llvm,
        major: plan.major,
    })
}

/// `cargo xtask shim --llvm <dir> --hod <hod> --out <dir> [--kiln <kiln++>]`.
pub const USAGE: &str = "\
usage: cargo xtask shim --llvm <dir> --hod <hod binary> --out <dir> [--kiln <kiln++>]

Creates or updates <out>: a directory kiln accepts as --llvm-dir, mirroring the LLVM
installation <dir> with symbolic links, except bin/llc, which runs <hod binary> with the
real llc as its fallback (HOD_STRICT and HOD_COVERAGE_LOG pass through from the
environment). With --kiln, also writes <out>/kiln++-hod, which runs <kiln++> with
--llvm-dir=<out>. <out> must not exist, or must have been made by this command.
";

/// Parses the command's arguments (`--name value` or `--name=value`).
pub fn parse_args(args: &[String]) -> Result<Request, String> {
    let (mut llvm, mut hod, mut out, mut kiln) = (None, None, None, None);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, joined) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let slot = match name {
            "--llvm" => &mut llvm,
            "--hod" => &mut hod,
            "--out" => &mut out,
            "--kiln" => &mut kiln,
            _ => return Err(format!("unknown argument '{arg}'")),
        };
        let value = joined
            .or_else(|| rest.next().cloned())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("'{name}' requires a value"))?;
        *slot = Some(PathBuf::from(value));
    }
    let need = |value: Option<PathBuf>, name: &str| value.ok_or_else(|| format!("missing {name}"));
    Ok(Request {
        llvm: need(llvm, "--llvm <dir>")?,
        hod: need(hod, "--hod <hod binary>")?,
        out: need(out, "--out <dir>")?,
        kiln,
    })
}

/// What the command prints on success.
pub fn summary(shim: &Shim) -> String {
    let mut text = format!(
        "shim: {} (LLVM {} from {})\n  llc:      {} (fallback {})\n",
        shim.dir.display(),
        shim.major,
        shim.llvm.display(),
        shim.llc.display(),
        shim.real_llc.display()
    );
    if let Some(compiler) = &shim.compiler {
        let _ = writeln!(text, "  compiler: {}", compiler.display());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::{Scratch, run_with};
    use std::os::unix::fs::MetadataExt;
    use std::time::Duration;

    fn script(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        hod_testutil::write_script(path, body);
    }

    /// A fake LLVM installation: stub tools, `lib/clang/<major>`, and a
    /// header directory.
    fn fake_llvm(dir: &Path, major: u32) {
        for tool in ["opt", "llc", "clang++", "llvm-as"] {
            script(&dir.join("bin").join(tool), &format!("echo real-{tool} \"$@\""));
        }
        fs::create_dir_all(dir.join("lib/clang").join(major.to_string())).unwrap();
        fs::create_dir_all(dir.join("include/c++/v1")).unwrap();
        fs::write(dir.join("include/c++/v1/vector"), "// vector\n").unwrap();
    }

    /// A stand-in for hod (or kiln): prints its fallback and its arguments.
    const ECHO: &str = "echo \"fallback=$HOD_FALLBACK_LLC strict=$HOD_STRICT log=$HOD_COVERAGE_LOG\"\n\
                        for a in \"$@\"; do echo \"arg=$a\"; done";

    struct Fixture {
        scratch: Scratch,
        request: Request,
    }

    fn fixture(label: &str) -> Fixture {
        let scratch = Scratch::new(label);
        let llvm = scratch.join("real/llvm-22");
        fake_llvm(&llvm, 22);
        script(&scratch.join("tools/hod"), ECHO);
        script(&scratch.join("tools/kiln++"), ECHO);
        let request = Request {
            llvm,
            hod: scratch.join("tools/hod"),
            out: scratch.join("shim"),
            kiln: Some(scratch.join("tools/kiln++")),
        };
        Fixture { scratch, request }
    }

    /// Every path under `dir` with what it is: a link's target, a file's
    /// mode, content and inode, or `dir`.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, String)> {
        let mut found = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(current) = pending.pop() {
            for name in names(&current).unwrap() {
                let path = current.join(name);
                let meta = fs::symlink_metadata(&path).unwrap();
                let what = if meta.file_type().is_symlink() {
                    format!("link {} ino {}", fs::read_link(&path).unwrap().display(), meta.ino())
                } else if meta.is_dir() {
                    pending.push(path.clone());
                    "dir".to_owned()
                } else {
                    format!(
                        "file {:o} ino {} {:?}",
                        meta.permissions().mode() & 0o777,
                        meta.ino(),
                        fs::read_to_string(&path).unwrap()
                    )
                };
                found.push((path.strip_prefix(dir).unwrap().to_path_buf(), what));
            }
        }
        found.sort();
        found
    }

    fn run_script(path: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
        let mut command = Command::new(path);
        command.args(args).env_remove("HOD_STRICT").env_remove("HOD_COVERAGE_LOG");
        command.env("HOD_FALLBACK_LLC", "from-the-environment").envs(env.iter().copied());
        let outcome = run_with(&mut command, b"", Duration::from_secs(20));
        assert!(outcome.success(), "{}: {}", path.display(), outcome.stderr_text());
        outcome.stdout_text()
    }

    // spec/harness.md "shim": the layout.
    #[test]
    fn layout() {
        let f = fixture("shim-layout");
        let shim = build(&f.request).unwrap();
        let out = fs::canonicalize(&f.request.out).unwrap();
        let llvm = fs::canonicalize(&f.request.llvm).unwrap();
        assert_eq!(fs::canonicalize(&shim.dir).unwrap(), out);
        assert_eq!((shim.major, &shim.llvm), (22, &llvm));
        assert_eq!(shim.llc, shim.dir.join("bin/llc"));
        assert_eq!(shim.real_llc, llvm.join("bin/llc"));
        assert_eq!(shim.compiler, Some(shim.dir.join("kiln++-hod")));

        let entries: Vec<PathBuf> = snapshot(&shim.dir).into_iter().map(|(p, _)| p).collect();
        let expected: Vec<PathBuf> = [
            ".hod-shim",
            "bin",
            "bin/clang++",
            "bin/llc",
            "bin/llvm-as",
            "bin/opt",
            "include",
            "kiln++-hod",
            "lib",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(entries, expected);
        for mirrored in ["bin/clang++", "bin/llvm-as", "bin/opt", "include", "lib"] {
            let link = shim.dir.join(mirrored);
            assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "{mirrored}");
            assert_eq!(fs::read_link(&link).unwrap(), llvm.join(mirrored), "{mirrored}");
        }
        // What kiln looks for (kiln_driver/src/llvm.rs `check`, `dir_major`;
        // toolchain.rs `find_tool`, `libcxx_include_base`).
        for tool in REQUIRED_TOOLS {
            assert!(shim.dir.join("bin").join(tool).is_file(), "{tool}");
        }
        assert!(shim.dir.join("lib/clang/22").is_dir());
        assert!(shim.dir.join("bin/../include/c++/v1/vector").is_file());
        let marker = fs::read_to_string(shim.dir.join(MARKER)).unwrap();
        assert!(marker.starts_with("hod-shim 1\nllvm "), "{marker}");
        assert!(summary(&shim).contains("LLVM 22"));
    }

    // spec/harness.md "shim": `bin/llc` is hod with the real llc as its
    // fallback; the other tools are the real ones.
    #[test]
    fn llc_runs_hod_with_the_real_llc_as_fallback() {
        let f = fixture("shim-llc");
        let shim = build(&f.request).unwrap();
        let text = run_script(&shim.llc, &["-O0", "a b", "-o", "x.o", "-"], &[]);
        let fallback = shim.real_llc.display();
        assert_eq!(
            text,
            format!("fallback={fallback} strict= log=\narg=-O0\narg=a b\narg=-o\narg=x.o\narg=-\n")
        );
        let text =
            run_script(&shim.llc, &[], &[("HOD_STRICT", "1"), ("HOD_COVERAGE_LOG", "/x/cov.tsv")]);
        assert_eq!(text, format!("fallback={fallback} strict=1 log=/x/cov.tsv\n"));
        assert_eq!(run_script(&shim.dir.join("bin/opt"), &["-S"], &[]), "real-opt -S\n");
        assert_eq!(run_script(&shim.dir.join("bin/clang++"), &[], &[]), "real-clang++\n");
    }

    // spec/harness.md "shim": the wrapper puts --llvm-dir=<shim> first.
    #[test]
    fn wrapper_runs_kiln_with_the_shim() {
        let f = fixture("shim-wrapper");
        let shim = build(&f.request).unwrap();
        let text = run_script(shim.compiler.as_deref().unwrap(), &["-c", "x y.cpp"], &[]);
        let lines: Vec<&str> = text.lines().skip(1).collect();
        let dir = format!("arg=--llvm-dir={}", shim.dir.display());
        assert_eq!(lines, [dir.as_str(), "arg=-c", "arg=x y.cpp"]);
    }

    // spec/harness.md "shim": paths are quoted for the shell.
    #[test]
    fn paths_with_spaces_and_quotes() {
        let scratch = Scratch::new("shim-quote");
        let llvm = scratch.join("it's real/llvm-22");
        fake_llvm(&llvm, 22);
        script(&scratch.join("my tools/hod $1"), ECHO);
        script(&scratch.join("my tools/kiln++"), ECHO);
        let request = Request {
            llvm,
            hod: scratch.join("my tools/hod $1"),
            out: scratch.join("the 'shim'"),
            kiln: Some(scratch.join("my tools/kiln++")),
        };
        let shim = build(&request).unwrap();
        let text = run_script(&shim.llc, &["-O0"], &[]);
        assert_eq!(text, format!("fallback={} strict= log=\narg=-O0\n", shim.real_llc.display()));
        let text = run_script(shim.compiler.as_deref().unwrap(), &[], &[]);
        assert!(text.ends_with(&format!("arg=--llvm-dir={}\n", shim.dir.display())), "{text}");
        assert_eq!(sh_quote(Path::new("a'b c")).unwrap(), "'a'\\''b c'");
    }

    // spec/harness.md "shim": running the command again changes nothing.
    #[test]
    fn idempotent() {
        let f = fixture("shim-idempotent");
        let first = build(&f.request).unwrap();
        let before = snapshot(&first.dir);
        let second = build(&f.request).unwrap();
        assert_eq!(first, second);
        assert_eq!(snapshot(&second.dir), before, "nothing is rewritten, not even in place");
    }

    // spec/harness.md "shim": a changed request updates the directory:
    // tools that came or went, another hod, no kiln.
    #[test]
    fn rebuild_follows_the_request() {
        let f = fixture("shim-rebuild");
        let shim = build(&f.request).unwrap();
        // A crashed run's temporary file is cleaned up too.
        fs::write(shim.dir.join("bin").join(format!("{TEMP_PREFIX}123-llc")), "partial").unwrap();
        script(&f.request.llvm.join("bin/llvm-objcopy"), "echo real-objcopy");
        fs::remove_file(f.request.llvm.join("bin/llvm-as")).unwrap();
        script(&f.scratch.join("tools/hod2"), "echo hod2");
        let request =
            Request { hod: f.scratch.join("tools/hod2"), kiln: None, ..f.request.clone() };
        let rebuilt = build(&request).unwrap();
        assert_eq!(rebuilt.compiler, None);
        let entries: Vec<PathBuf> = snapshot(&rebuilt.dir).into_iter().map(|(p, _)| p).collect();
        let expected: Vec<PathBuf> = [
            ".hod-shim",
            "bin",
            "bin/clang++",
            "bin/llc",
            "bin/llvm-objcopy",
            "bin/opt",
            "include",
            "lib",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(entries, expected);
        assert_eq!(run_script(&rebuilt.llc, &[], &[]), "hod2\n");
        assert!(fs::read_to_string(rebuilt.dir.join(MARKER)).unwrap().ends_with("kiln -\n"));
    }

    // spec/harness.md "shim": an --out that exists and is not a shim is
    // refused and left alone.
    #[test]
    fn refuses_a_directory_it_did_not_make() {
        let f = fixture("shim-refuse");
        fs::create_dir_all(&f.request.out).unwrap();
        let empty = build(&f.request).unwrap_err();
        assert!(empty.contains("was not made by this command"), "{empty}");
        assert!(names(&f.request.out).unwrap().is_empty());

        fs::write(f.request.out.join("precious.txt"), "data").unwrap();
        assert!(build(&f.request).unwrap_err().contains("was not made by this command"));
        assert_eq!(names(&f.request.out).unwrap(), ["precious.txt"]);

        // A file, and a link to a directory, are not shim directories.
        let file = Request { out: f.scratch.join("tools/hod"), ..f.request.clone() };
        assert!(build(&file).unwrap_err().contains("was not made by this command"));
        let made = build(&Request { out: f.scratch.join("made"), ..f.request.clone() }).unwrap();
        symlink(&made.dir, f.scratch.join("link")).unwrap();
        let link = Request { out: f.scratch.join("link"), ..f.request.clone() };
        assert!(build(&link).unwrap_err().contains("was not made by this command"));

        // Something foreign inside a shim directory stops an update.
        fs::write(made.dir.join("notes.txt"), "mine").unwrap();
        let again = build(&Request { out: made.dir.clone(), ..f.request.clone() }).unwrap_err();
        assert!(again.contains("notes.txt") && again.contains("refusing"), "{again}");
        assert_eq!(fs::read_to_string(made.dir.join("notes.txt")).unwrap(), "mine");
    }

    // spec/harness.md "shim": nothing is written through a link, so a shim
    // whose `bin` was replaced by a link to the real one cannot damage it.
    #[test]
    fn never_writes_through_links() {
        let f = fixture("shim-links");
        let shim = build(&f.request).unwrap();
        let real_llc = fs::read_to_string(&shim.real_llc).unwrap();
        for name in names(&shim.dir.join("bin")).unwrap() {
            fs::remove_file(shim.dir.join("bin").join(name)).unwrap();
        }
        fs::remove_dir(shim.dir.join("bin")).unwrap();
        symlink(f.request.llvm.join("bin"), shim.dir.join("bin")).unwrap();
        let refused = build(&f.request).unwrap_err();
        assert!(refused.contains("is not a directory"), "{refused}");
        assert_eq!(fs::read_to_string(&shim.real_llc).unwrap(), real_llc);

        // A generated file replaced by a link is replaced, not followed.
        let f = fixture("shim-links2");
        let shim = build(&f.request).unwrap();
        fs::remove_file(&shim.llc).unwrap();
        symlink(&shim.real_llc, &shim.llc).unwrap();
        build(&f.request).unwrap();
        assert!(fs::symlink_metadata(&shim.llc).unwrap().is_file());
        assert_eq!(fs::read_to_string(&shim.real_llc).unwrap(), real_llc);
    }

    // spec/harness.md "shim": requests that cannot give kiln a usable
    // installation are errors.
    #[test]
    fn rejects_unusable_requests() {
        let f = fixture("shim-reject");
        let with = |change: &dyn Fn(&mut Request)| {
            let mut request = f.request.clone();
            change(&mut request);
            build(&request).unwrap_err()
        };
        let missing = with(&|r| r.llvm = f.scratch.join("nowhere"));
        assert!(missing.contains("cannot find"), "{missing}");
        let no_hod = with(&|r| r.hod = f.scratch.join("tools/missing"));
        assert!(no_hod.contains("--hod"), "{no_hod}");
        let no_kiln = with(&|r| r.kiln = Some(f.scratch.join("tools/missing")));
        assert!(no_kiln.contains("--kiln"), "{no_kiln}");
        let inside = with(&|r| r.out = r.llvm.join("shim"));
        assert!(inside.contains("must not contain one another"), "{inside}");
        let around = with(&|r| r.out = f.scratch.join("real"));
        assert!(around.contains("must not contain one another"), "{around}");
        assert!(!f.request.out.exists(), "a refused request creates nothing");

        fs::remove_file(f.request.llvm.join("bin/clang++")).unwrap();
        let no_tool = with(&|_| {});
        assert!(no_tool.contains("no 'bin/clang++'"), "{no_tool}");

        // A shim is not an LLVM installation to mirror.
        let g = fixture("shim-reject-nested");
        let shim = build(&g.request).unwrap();
        let nested =
            build(&Request { llvm: shim.dir, out: g.scratch.join("shim2"), ..g.request.clone() });
        assert!(nested.unwrap_err().contains("is itself a hod shim"));
    }

    // spec/harness.md "shim": kiln tells the major from `lib/clang/<N>`,
    // else `bin/llvm-config --version`, else the directory's name; the shim
    // must show the same major by the same means.
    #[test]
    fn major_detection() {
        let f = fixture("shim-major");
        // llvm-config only.
        fs::remove_dir_all(f.request.llvm.join("lib")).unwrap();
        script(&f.request.llvm.join("bin/llvm-config"), "echo 19.1.7");
        assert_eq!(build(&f.request).unwrap().major, 19);
        // The name only: the shim must carry the same name.
        fs::remove_file(f.request.llvm.join("bin/llvm-config")).unwrap();
        let by_name = build(&Request { out: f.scratch.join("x/shim"), ..f.request.clone() });
        assert!(by_name.unwrap_err().contains("name the --out directory 'llvm-22'"));
        let named = Request { out: f.scratch.join("x/llvm-22"), ..f.request.clone() };
        assert_eq!(build(&named).unwrap().major, 22);
        // Nothing tells.
        let anonymous = f.scratch.join("real/toolchain");
        fake_llvm(&anonymous, 22);
        fs::remove_dir_all(anonymous.join("lib")).unwrap();
        let unknown =
            build(&Request { llvm: anonymous, out: f.scratch.join("y"), ..f.request.clone() });
        assert!(unknown.unwrap_err().contains("cannot tell the LLVM major"));
        assert_eq!(dir_name_major(Path::new("/usr/lib64/llvm19")), Some(19));
        assert_eq!(dir_name_major(Path::new("/opt/llvm@22")), Some(22));
        assert_eq!(dir_name_major(Path::new("/opt/llvm-main")), None);
    }

    #[test]
    fn argument_parsing() {
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let request = parse_args(&args(&[
            "--llvm",
            "/usr/lib/llvm-22",
            "--hod=target/debug/hod",
            "--out",
            "out dir",
            "--kiln=/k/kiln++",
        ]))
        .unwrap();
        assert_eq!(
            request,
            Request {
                llvm: "/usr/lib/llvm-22".into(),
                hod: "target/debug/hod".into(),
                out: "out dir".into(),
                kiln: Some("/k/kiln++".into()),
            }
        );
        let no_kiln = parse_args(&args(&["--out=o", "--hod=h", "--llvm=l"])).unwrap();
        assert_eq!(no_kiln.kiln, None);
        for (bad, needle) in [
            (&["--llvm", "l", "--hod", "h"][..], "missing --out"),
            (&["--llvm"], "'--llvm' requires a value"),
            (&["--out="], "'--out' requires a value"),
            (&["--llvm=l", "--hod=h", "--out=o", "extra"], "unknown argument 'extra'"),
        ] {
            let error = parse_args(&args(bad)).unwrap_err();
            assert!(error.contains(needle), "{bad:?}: {error}");
        }
    }
}
