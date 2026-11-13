//! The permitted differences of `spec/dwarf.md` §5.7 that are recognised
//! from the IR both programs were built from: the classes of pairs
//! `load-placement` and `is-constant`, and the `return-column` of a
//! debugger's stop after `finish`. Contract: `spec/harness.md` "debug",
//! "Lines" and "Permitted differences of sessions".
//!
//! The IR is read as text (`llvm-dis` syntax, as the front ends write it):
//! the `!DILocation`s, and per defined function the opcode, line and
//! column of each instruction with a `!dbg`, the file of its scope
//! (`DIFile` of the `DILocation`'s scope, joined as the line table joins
//! it), whether its block has a predecessor (the entry block, or a block
//! some `label %N` names: `llc` deletes the others, even at `-O0`) and
//! the pointer operand of a load or `getelementptr`.

use std::collections::{BTreeMap, BTreeSet};

/// One instruction with a location: its opcode, line and column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub opcode: String,
    pub line: u32,
    pub column: u32,
    /// The file of the location's scope; `None` when the IR does not say
    /// (it then matches any file).
    pub file: Option<String>,
    /// Its block is the entry block or has a predecessor.
    pub reachable: bool,
    /// A load's or `getelementptr`'s pointer operand (`%13`).
    pub pointer: Option<String>,
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
        let files = Files::of(text);
        let mut current: Option<(String, IrFunction, Blocks)> = None;
        for line in text.lines() {
            if let Some(name) = defined(line) {
                current = Some((name, IrFunction::default(), Blocks::default()));
                continue;
            }
            let Some((name, function, blocks)) = current.as_mut() else { continue };
            if line.starts_with('}') {
                let (name, mut function) = (std::mem::take(name), std::mem::take(function));
                blocks.mark(&mut function.insts);
                self.functions.insert(name, function);
                current = None;
                continue;
            }
            blocks.read(line);
            if line.contains("@llvm.is.constant.") {
                function.calls_is_constant = true;
            }
            if let (Some(op), Some(n)) = (opcode(line), dbg(line))
                && let Some(&(l, c)) = locations.get(&n)
            {
                let pointer =
                    matches!(op, "load" | "getelementptr").then(|| pointer(line)).flatten();
                function.insts.push(Located {
                    opcode: op.to_owned(),
                    line: l,
                    column: c,
                    file: files.of_location(n),
                    reachable: true,
                    pointer,
                });
                blocks.of_inst.push(blocks.current.clone());
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

    /// The permitted class of a `(function, file, line)` pair only one
    /// side has (`spec/dwarf.md` §5.7), by the opcodes of the function's
    /// instructions at that line (any column) of that file, in blocks
    /// with a predecessor:
    ///
    /// - `load-placement`: they are loads and `getelementptr`s, at least
    ///   one load (either side);
    /// - `post-indexed-load`: hod's side, they are `getelementptr`s whose
    ///   pointer operand some load of the function reads, which `llc`
    ///   folds into that load's post-increment;
    /// - `is-constant`: hod's side, in a function that calls
    ///   `llvm.is.constant`.
    pub fn pair_class(
        &self,
        function: &str,
        file: &str,
        line: u32,
        hod_side: bool,
    ) -> Option<PairClass> {
        let f = self.functions.get(function)?;
        let at: Vec<&Located> = f
            .insts
            .iter()
            .filter(|i| {
                i.line == line && i.reachable && i.file.as_deref().is_none_or(|x| x == file)
            })
            .collect();
        let ops: BTreeSet<&str> = at.iter().map(|i| i.opcode.as_str()).collect();
        if ops.contains("load") && ops.iter().all(|op| matches!(*op, "load" | "getelementptr")) {
            return Some(PairClass::LoadPlacement);
        }
        if hod_side && !at.is_empty() && at.iter().all(|i| f.post_indexed(i)) {
            return Some(PairClass::PostIndexedLoad);
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
    PostIndexedLoad,
    IsConstant,
}

impl IrFunction {
    /// `i` is a `getelementptr` whose pointer operand a load of the
    /// function reads (the post-indexed load `llc` folds it into).
    fn post_indexed(&self, i: &Located) -> bool {
        i.opcode == "getelementptr"
            && i.pointer.as_ref().is_some_and(|p| {
                self.insts.iter().any(|l| l.opcode == "load" && l.pointer.as_ref() == Some(p))
            })
    }
}

/// The first `ptr %x` operand of an instruction line (a load's or a
/// `getelementptr`'s pointer: their type comes first and is not `ptr %`).
fn pointer(line: &str) -> Option<String> {
    let at = line.find("ptr %")? + 4;
    let rest = &line[at..];
    let end = if rest[1..].starts_with('"') {
        rest[2..].find('"').map(|e| e + 3)?
    } else {
        rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '$')))
            .map_or(rest.len(), |e| e + 1)
    };
    Some(rest[..end].to_owned())
}

/// The files of a module's locations: `DIFile`s, the `file:` of each
/// scope, and each `DILocation`'s scope.
#[derive(Default)]
struct Files {
    /// `DIFile` id: the joined path (the name when absolute, else
    /// `directory/name`, as `lines.rs` joins the line table's).
    names: BTreeMap<u32, String>,
    /// Scope id: its `file:` id.
    scopes: BTreeMap<u32, u32>,
    /// `DILocation` id: its `scope:` id.
    locations: BTreeMap<u32, u32>,
}

/// The id of a field `name: !N` of a metadata line.
fn field_ref(fields: &str, name: &str) -> Option<u32> {
    let at = fields.find(name)? + name.len();
    let digits: String =
        fields[at..].strip_prefix('!')?.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The string of a field `name: "..."` of a metadata line.
fn field_str<'t>(fields: &'t str, name: &str) -> Option<&'t str> {
    let at = fields.find(name)? + name.len();
    fields[at..].strip_prefix('"')?.split('"').next()
}

impl Files {
    fn of(text: &str) -> Self {
        let mut files = Self::default();
        for line in text.lines() {
            let Some((id, rest)) = line.strip_prefix('!').and_then(|r| r.split_once(" = ")) else {
                continue;
            };
            let Ok(id) = id.parse::<u32>() else { continue };
            let rest = rest.strip_prefix("distinct ").unwrap_or(rest);
            if rest.starts_with("!DIFile(") {
                let name = field_str(rest, "filename: ").unwrap_or_default();
                let dir = field_str(rest, "directory: ").unwrap_or_default();
                let joined = if name.starts_with('/') || dir.is_empty() {
                    name.to_owned()
                } else {
                    format!("{dir}/{name}")
                };
                files.names.insert(id, joined);
            } else if rest.starts_with("!DILocation(") {
                if let Some(scope) = field_ref(rest, "scope: ") {
                    files.locations.insert(id, scope);
                }
            } else if rest.starts_with("!DI")
                && let Some(file) = field_ref(rest, " file: ")
            {
                files.scopes.insert(id, file);
            }
        }
        files
    }

    fn of_location(&self, location: u32) -> Option<String> {
        let scope = self.locations.get(&location)?;
        self.names.get(self.scopes.get(scope)?).cloned()
    }
}

/// A function's blocks while it is read: the current block's label
/// (`None` for the entry block), the labels some `label %N` names, and
/// each located instruction's block.
#[derive(Default)]
struct Blocks {
    current: Option<String>,
    named: BTreeSet<String>,
    of_inst: Vec<Option<String>>,
}

impl Blocks {
    fn read(&mut self, line: &str) {
        if !line.starts_with([' ', '\t', ';']) && !line.is_empty() {
            if let Some((label, _)) = line.split_once(':') {
                self.current = Some(label.trim_matches('"').to_owned());
            }
            return;
        }
        let mut rest = line;
        while let Some(at) = rest.find("label %") {
            rest = &rest[at + 7..];
            let name = if let Some(quoted) = rest.strip_prefix('"') {
                quoted.split('"').next().unwrap_or_default()
            } else {
                let end = rest
                    .find(|c: char| {
                        !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '$'))
                    })
                    .unwrap_or(rest.len());
                &rest[..end]
            };
            self.named.insert(name.to_owned());
        }
    }

    /// Marks the instructions of blocks without a predecessor.
    fn mark(&mut self, insts: &mut [Located]) {
        for (inst, block) in insts.iter_mut().zip(&self.of_inst) {
            inst.reachable = block.as_ref().is_none_or(|b| self.named.contains(b));
        }
    }
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
        assert_eq!(facts.pair_class("main", "c.c", 9, false), Some(PairClass::LoadPlacement));
        assert_eq!(facts.pair_class("main", "c.c", 20, true), Some(PairClass::IsConstant));
        assert_eq!(facts.pair_class("main", "c.c", 20, false), None);
        assert_eq!(facts.pair_class("other", "c.c", 9, true), None);
        assert!(facts.has_call_at(20, 13) && !facts.has_call_at(20, 7));
    }

    // `post-indexed-load`, and the file and predecessor checks (WP-111):
    // libstdc++'s `_Sp_counted_base::_M_release_last_use`, whose line 187
    // also has a terminate landing pad without predecessors, which `llc`
    // deletes.
    const POST_INDEXED: &str = "\
define void @release(ptr %0) personality ptr @p !dbg !10 {
  %2 = alloca ptr, align 8
  %3 = load ptr, ptr %2, align 8
  %4 = load ptr, ptr %3, align 8, !dbg !20
  call void %4(ptr %3), !dbg !20
  %5 = getelementptr inbounds nuw %\"class.C\", ptr %3, i32 0, i32 2, !dbg !21
  %6 = getelementptr inbounds i8, ptr %2, i64 4, !dbg !24
  br label %7, !dbg !20

7:                                                ; preds = %1
  %8 = load i32, ptr %5, align 4, !dbg !23
  ret void, !dbg !23

9:                                                ; No predecessors!
  %10 = landingpad { ptr, i32 }
          catch ptr null, !dbg !22
  call void @t(ptr null), !dbg !22
  unreachable, !dbg !22
}
!1 = !DIFile(filename: \"c.cpp\", directory: \"/w\")
!2 = !DIFile(filename: \"/abs/h.h\", directory: \"/w\")
!10 = distinct !DISubprogram(name: \"release\", scope: !1, file: !1, line: 1)
!11 = distinct !DILexicalBlock(scope: !10, file: !1, line: 3, column: 6)
!20 = !DILocation(line: 2, column: 3, scope: !10)
!21 = !DILocation(line: 3, column: 46, scope: !11)
!22 = !DILocation(line: 3, column: 6, scope: !11)
!23 = !DILocation(line: 4, column: 3, scope: !10)
!24 = !DILocation(line: 3, column: 9, scope: !12)
!12 = distinct !DILexicalBlockFile(scope: !10, file: !2, discriminator: 0)
";

    #[test]
    fn post_indexed_loads_by_file_and_predecessor() {
        let facts = IrFacts::of_modules([POST_INDEXED]);
        let f = &facts.functions["release"];
        let gep = &f.insts[2];
        assert_eq!(
            (gep.file.as_deref(), gep.pointer.as_deref(), gep.reachable),
            (Some("/w/c.cpp"), Some("%3"), true)
        );
        assert_eq!(f.insts[3].file.as_deref(), Some("/abs/h.h"));
        assert!(f.insts[4].reachable && !f.insts.last().unwrap().reachable);
        let class = |file, line, hod| facts.pair_class("release", file, line, hod);
        assert_eq!(class("/w/c.cpp", 3, true), Some(PairClass::PostIndexedLoad));
        // llc's side, another file's line 3 (a gep of a slot no load
        // reads), and a line without instructions.
        assert_eq!(class("/w/c.cpp", 3, false), None);
        assert_eq!(class("/abs/h.h", 3, true), None);
        assert_eq!(class("/w/c.cpp", 5, true), None);
        // The landing pad counted (it had a predecessor): not the class.
        let reached = POST_INDEXED.replace("br label %7", "br label %9");
        let facts = IrFacts::of_modules([reached.as_str()]);
        assert_eq!(facts.pair_class("release", "/w/c.cpp", 3, true), None);
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
