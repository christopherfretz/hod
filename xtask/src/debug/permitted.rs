//! The permitted differences of `spec/dwarf.md` §5.7 that are recognised
//! from the IR both programs were built from: the classes of pairs
//! `load-placement` and `is-constant`, and the `return-column` of a
//! debugger's stop after `finish`. Contract: `spec/harness.md` "debug",
//! "Lines" and "Permitted differences of sessions".
//!
//! The IR is read as text (`llvm-dis` syntax, as the front ends write it):
//! the `!DILocation`s, and per defined function the opcode, line and
//! column of each instruction with a `!dbg`. Scopes are not followed: a
//! location's line and column are what the line table shows.

use std::collections::{BTreeMap, BTreeSet};

/// One instruction with a location: its opcode, line and column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub opcode: String,
    pub line: u32,
    pub column: u32,
}

/// What the classes need of one function.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IrFunction {
    pub insts: Vec<Located>,
    /// It calls `llvm.is.constant.*`.
    pub calls_is_constant: bool,
}

/// What the classes need of a set of modules: functions by name (the
/// symbol, as `llvm-nm` prints it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IrFacts {
    pub functions: BTreeMap<String, IrFunction>,
}

/// `!N = !DILocation(line: L, column: C, ...)`: `N`, `L`, `C` (0 without
/// a column).
fn location(line: &str) -> Option<(u32, u32, u32)> {
    let rest = line.strip_prefix('!')?;
    let (id, rest) = rest.split_once(" = ")?;
    let fields = rest.strip_prefix("!DILocation(")?.strip_suffix(')').unwrap_or(rest);
    let field = |name: &str| {
        fields.split(", ").find_map(|f| f.strip_prefix(name)?.trim().parse::<u32>().ok())
    };
    Some((id.parse().ok()?, field("line:")?, field("column:").unwrap_or(0)))
}

/// The name of `define ... @name(`.
fn defined(line: &str) -> Option<String> {
    let rest = line.strip_prefix("define ")?;
    let at = rest.find('@')?;
    let name = &rest[at + 1..];
    if let Some(quoted) = name.strip_prefix('"') {
        return Some(quoted.split('"').next()?.to_owned());
    }
    let end = name.find('(')?;
    Some(name[..end].to_owned())
}

/// The opcode of an instruction line: after `%x = ` and a call's
/// `tail`/`musttail`/`notail`.
fn opcode(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    if rest.starts_with('%') {
        rest = rest.split_once(" = ")?.1;
    }
    for prefix in ["tail ", "musttail ", "notail "] {
        rest = rest.strip_prefix(prefix).unwrap_or(rest);
    }
    let word = rest.split(|c: char| !c.is_ascii_lowercase() && c != '_').next()?;
    (!word.is_empty()).then_some(word)
}

/// The `!dbg !N` of an instruction line.
fn dbg(line: &str) -> Option<u32> {
    let at = line.rfind("!dbg !")?;
    let digits: String = line[at + 6..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

impl IrFacts {
    /// Adds a module's functions.
    pub fn add_module(&mut self, text: &str) {
        let locations: BTreeMap<u32, (u32, u32)> =
            text.lines().filter_map(location).map(|(id, l, c)| (id, (l, c))).collect();
        let mut current: Option<(String, IrFunction)> = None;
        for line in text.lines() {
            if let Some(name) = defined(line) {
                current = Some((name, IrFunction::default()));
                continue;
            }
            let Some((name, function)) = current.as_mut() else { continue };
            if line.starts_with('}') {
                let (name, function) = (std::mem::take(name), std::mem::take(function));
                self.functions.insert(name, function);
                current = None;
                continue;
            }
            if line.contains("@llvm.is.constant.") {
                function.calls_is_constant = true;
            }
            if let (Some(op), Some(&(l, c))) =
                (opcode(line), dbg(line).and_then(|n| locations.get(&n)))
            {
                function.insts.push(Located { opcode: op.to_owned(), line: l, column: c });
            }
        }
    }

    /// Facts of several modules' texts.
    pub fn of_modules<'t>(texts: impl IntoIterator<Item = &'t str>) -> Self {
        let mut facts = Self::default();
        for text in texts {
            facts.add_module(text);
        }
        facts
    }

    /// The permitted class of a `(function, line)` pair only one side
    /// has (`spec/dwarf.md` §5.7), by the opcodes of the function's
    /// instructions at that line (any column):
    ///
    /// - `load-placement`: they are loads and `getelementptr`s, at least
    ///   one load (either side);
    /// - `is-constant`: hod's side, in a function that calls
    ///   `llvm.is.constant`.
    pub fn pair_class(&self, function: &str, line: u32, hod_side: bool) -> Option<PairClass> {
        let f = self.functions.get(function)?;
        let ops: BTreeSet<&str> =
            f.insts.iter().filter(|i| i.line == line).map(|i| i.opcode.as_str()).collect();
        if ops.contains("load") && ops.iter().all(|op| matches!(*op, "load" | "getelementptr")) {
            return Some(PairClass::LoadPlacement);
        }
        if hod_side && f.calls_is_constant {
            return Some(PairClass::IsConstant);
        }
        None
    }

    /// Whether some `call` or `invoke` (of any function of the modules)
    /// is at `line` and `column`.
    pub fn has_call_at(&self, line: u32, column: u32) -> bool {
        self.functions.values().any(|f| {
            f.insts.iter().any(|i| {
                i.line == line
                    && i.column == column
                    && matches!(i.opcode.as_str(), "call" | "invoke")
            })
        })
    }
}

/// A permitted class read from the IR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PairClass {
    LoadPlacement,
    IsConstant,
}

// ------------------------------------------------- sessions ----

/// `<file>:<line>:<column>` at the end of an lldb `frame #N:` line's
/// position (` at file:L:C`): the text before the column, the line, the
/// column.
fn frame_position(line: &str) -> Option<(&str, u32, u32)> {
    let at = line.rfind(" at ")?;
    let position = &line[at + 4..];
    let (before, column) = position.rsplit_once(':')?;
    let (_, number) = before.rsplit_once(':')?;
    let cut = at + 4 + before.len();
    Some((&line[..cut], number.parse().ok()?, column.parse().ok()?))
}

/// The `return-column` rule (`spec/dwarf.md` §5.7): two masked lldb
/// transcripts that differ only in stops after `finish` (`stop reason =
/// step out`), where in each differing pair of lines the `frame #0:`
/// line has the same text and line and another column, `llc`'s column
/// (or, WP-136, hod's) being that of a `call` or `invoke` at that line,
/// and the source
/// caret line (`^`) under it moves with the column. Any other
/// difference, or a different number of lines, is not permitted.
pub fn only_return_columns(hod: &str, llc: &str, facts: &IrFacts) -> bool {
    let (ours, theirs): (Vec<&str>, Vec<&str>) = (hod.lines().collect(), llc.lines().collect());
    if ours.len() != theirs.len() || ours == theirs {
        return false;
    }
    let mut step_out = false;
    let mut moved = false;
    for (a, b) in ours.iter().zip(&theirs) {
        if b.contains("stop reason = ") {
            step_out = b.contains("stop reason = step out");
            moved = false;
        }
        if a == b {
            continue;
        }
        if !step_out {
            return false;
        }
        let is_caret = |l: &str| l.trim() == "^";
        if is_caret(a) && is_caret(b) && moved {
            continue;
        }
        match (frame_position(a), frame_position(b)) {
            (Some((pa, la, ca)), Some((pb, lb, cb)))
                if pa == pb && la == lb && b.trim_start().starts_with("frame #0:") =>
            {
                // On x86-64 the roles swap (`spec/dwarf.md` §6.17): hod's
                // copy of the result out of `rax` keeps the call's
                // location where `llc` coalesced it.
                if !facts.has_call_at(lb, cb) && !facts.has_call_at(la, ca) {
                    return false;
                }
                moved = true;
            }
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const IR: &str = "\
define dso_local i32 @main() #0 !dbg !10 {
  %1 = alloca i32, align 4
  %2 = load ptr, ptr %1, align 8, !dbg !20
  %3 = call i32 @bump(ptr noundef %1, i32 noundef 1), !dbg !21
  store i32 %3, ptr %1, align 4, !dbg !22
  %4 = call i1 @llvm.is.constant.i64(i64 0), !dbg !24
  ret i32 0, !dbg !23
}
define linkonce_odr void @\"quoted name\"() {
  ret void, !dbg !23
}
!20 = !DILocation(line: 9, column: 3, scope: !10)
!21 = !DILocation(line: 20, column: 13, scope: !10)
!22 = !DILocation(line: 20, column: 7, scope: !10)
!23 = !DILocation(line: 21, scope: !10)
!24 = !DILocation(line: 22, column: 3, scope: !10)
";

    // spec/harness.md "debug", "Lines": the IR's facts and the classes.
    #[test]
    fn reads_ir_and_classifies_pairs() {
        let facts = IrFacts::of_modules([IR]);
        let main = &facts.functions["main"];
        assert!(main.calls_is_constant);
        let ops: Vec<&str> = main.insts.iter().map(|i| i.opcode.as_str()).collect();
        assert_eq!(ops, ["load", "call", "store", "call", "ret"]);
        assert_eq!(main.insts[4].column, 0);
        assert!(facts.functions.contains_key("quoted name"));
        assert_eq!(facts.pair_class("main", 9, false), Some(PairClass::LoadPlacement));
        assert_eq!(facts.pair_class("main", 20, true), Some(PairClass::IsConstant));
        assert_eq!(facts.pair_class("main", 20, false), None);
        assert_eq!(facts.pair_class("other", 9, true), None);
        assert!(facts.has_call_at(20, 13) && !facts.has_call_at(20, 7));
    }

    // spec/harness.md "debug", "Permitted differences of sessions".
    #[test]
    fn return_columns() {
        let facts = IrFacts::of_modules([IR]);
        let session = |column: u32, caret: &str| {
            format!(
                "(lldb) finish\nProcess ? stopped\n* thread #1, name = 'prog', stop reason = step out\n\
                 Return value: (int) $0 = 5\n\n    frame #0: 0x? prog`main at c.c:20:{column}\n\
                 -> 20  \t  int got = bump(&c, 1);\n    \t{caret}^\n(lldb) continue\n"
            )
        };
        let (llc, hod) = (session(13, "          "), session(7, "    "));
        assert!(only_return_columns(&hod, &llc, &facts));
        // Equal transcripts are no permitted difference (nothing to permit).
        assert!(!only_return_columns(&llc, &llc, &facts));
        // hod's column is the call's, llc's not (x86-64, WP-136).
        assert!(only_return_columns(&llc, &hod, &facts));
        // Neither column is a call's.
        assert!(!only_return_columns(&session(9, "      "), &hod, &facts));
        // Not after `finish`.
        let stepped = |s: &str| s.replace("step out", "step over");
        assert!(!only_return_columns(&stepped(&hod), &stepped(&llc), &facts));
        // Another line differs.
        let other = hod.replace("$0 = 5", "$0 = 6");
        assert!(!only_return_columns(&other, &llc, &facts));
        // Another line number.
        let line = hod.replace("c.c:20:7", "c.c:21:7");
        assert!(!only_return_columns(&line, &llc, &facts));
    }
}
