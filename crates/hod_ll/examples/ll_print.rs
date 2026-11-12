//! Parses one `.ll` file, module and bodies, and prints it back as LLVM
//! text of the given major (`ll_print 19 in.ll > out.ll`): a hand tool
//! for `spec/ll.md` §7.4 and §11.9. The unsupported constructs, if any,
//! go to standard error, and nothing is printed then.

use std::io::Write as _;
use std::process::ExitCode;

use hod_ir::Dialect;
use hod_ll::{BodyParser, ParseOptions, parse_module};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [major, path] = args.as_slice() else {
        eprintln!("usage: ll_print <llvm major> <file.ll>");
        return ExitCode::FAILURE;
    };
    let Ok(major) = major.parse::<u32>() else {
        eprintln!("{major}: not a number");
        return ExitCode::FAILURE;
    };
    let source = match std::fs::read(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut parsed = match parse_module(&source, &ParseOptions::default()) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut parser = BodyParser::new(&parsed.text);
    let mut bodies = Vec::new();
    for function in parsed.text.functions() {
        match parser.parse(&mut parsed.module, function) {
            Ok(Some(body)) => bodies.push(body),
            Ok(None) => {}
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut unsupported = false;
    for entry in parsed.module.unsupported() {
        eprintln!("unsupported: {entry:?}");
        unsupported = true;
    }
    if unsupported {
        return ExitCode::FAILURE;
    }
    let dialect = match Dialect::new(major) {
        Ok(dialect) => dialect,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut text = String::new();
    if let Err(error) = parsed.module.print(&mut text, dialect, &bodies) {
        eprintln!("{path}: {error}");
        return ExitCode::FAILURE;
    }
    if std::io::stdout().write_all(text.as_bytes()).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
