//! The tokens of the debug nodes lexed ahead on other threads
//! (`spec/ll.md` §4.5): the nodes are cut into pieces, other threads lex
//! each piece's bodies into a [`Recording`] and hand them over in node
//! order, and the debug nodes' reading replays them (§2.8). A token is a
//! function of the source and the offset it is looked for at, so what
//! is read is what reading without them gives; a token not recorded is
//! lexed where it is wanted.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use crate::index::NodeEnt;
use crate::lexer::{Lexer, Recording, Tok};
use crate::meta::MdKind;

/// The bytes of node bodies of a piece: about 8,000 tokens of debug
/// metadata, 128 KiB of recording, well under [`Recording::MAX`].
pub(crate) const PRELEX_PIECE_BYTES: usize = 64 << 10;

/// The most threads that lex ahead. A token is lexed two to three times
/// as fast as the node loop reads it, so one keeps ahead; more only
/// contend for the hand-over and the caches (measured in
/// `spec/landed/ll/WP-303.md`: on a 16 MB `-g` text, 1.5 to 3 ms less
/// module-level parse with one, up to 3 ms more with seven).
pub(crate) const PRELEX_THREADS: usize = 1;

/// The pieces lexed and not yet replayed, at most, per lexing thread:
/// what bounds the memory of the recordings.
const AHEAD_PER_THREAD: usize = 2;

/// Whether the node loop of the debug nodes reads `kind`'s body where it
/// stands: every kind but the expressions, which are read before it.
fn lexed_ahead(kind: MdKind) -> bool {
    kind != MdKind::Expression
}

/// The first node of each piece of `nodes`, then `nodes.len()`: a piece
/// ends at the first node at which its bodies reach `piece_bytes`.
pub(crate) fn cuts(nodes: &[NodeEnt], piece_bytes: usize) -> Vec<usize> {
    let mut cuts = vec![0];
    let mut bytes = 0;
    for (at, node) in nodes.iter().enumerate() {
        if bytes >= piece_bytes {
            cuts.push(at);
            bytes = 0;
        }
        bytes += (node.span.end - node.span.start) as usize;
    }
    cuts.push(nodes.len());
    cuts
}

/// The tokens of `nodes`' bodies, as the node loop asks for them: each
/// from the start of its body up to the one that begins at its end (the
/// closing bracket); none of a node past an `Eof` or `Error` token.
pub(crate) fn lex_piece(lexer: &mut Lexer<'_>, nodes: &[NodeEnt], recording: &mut Recording) {
    for node in nodes {
        if !lexed_ahead(node.kind) || recording.last_from() >= Some(node.span.start as usize) {
            continue;
        }
        lexer.set_pos(node.span.start as usize);
        loop {
            let from = lexer.pos();
            let token = lexer.next_token();
            if matches!(token.kind, Tok::Eof | Tok::Error) {
                break;
            }
            recording.push(from, token);
            if token.start >= node.span.end {
                break;
            }
        }
    }
}

/// The hand-over of the pieces: the lexing threads take them in turn and
/// put each one lexed in its place; the node loop takes them in order.
pub(crate) struct Feed {
    state: Mutex<State>,
    /// A piece was put in place.
    ready: Condvar,
    /// A piece was taken, or the feed closed.
    room: Condvar,
    pieces: usize,
    ahead: usize,
}

struct State {
    /// The next piece to lex.
    next: usize,
    /// The pieces taken by the node loop.
    taken: usize,
    /// From piece `taken` on, those lexed.
    done: VecDeque<Option<Recording>>,
    /// Storage given back, emptied.
    spare: Vec<Recording>,
    closed: bool,
}

impl Feed {
    /// A feed of `pieces` pieces lexed by `threads` threads.
    pub(crate) fn new(pieces: usize, threads: usize) -> Feed {
        let state =
            State { next: 0, taken: 0, done: VecDeque::new(), spare: Vec::new(), closed: false };
        Feed {
            state: Mutex::new(state),
            ready: Condvar::new(),
            room: Condvar::new(),
            pieces,
            ahead: AHEAD_PER_THREAD * threads.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A lexing thread: lexes pieces of `nodes` (cut at `cuts`) until
    /// there are none left or the feed is closed.
    pub(crate) fn work(&self, src: &[u8], nodes: &[NodeEnt], cuts: &[usize]) {
        let mut lexer = Lexer::new(src);
        loop {
            let (piece, mut recording) = {
                let mut state = self.lock();
                loop {
                    if state.closed || state.next == self.pieces {
                        return;
                    }
                    if state.next < state.taken + self.ahead {
                        break;
                    }
                    state = self.room.wait(state).unwrap_or_else(PoisonError::into_inner);
                }
                let piece = state.next;
                state.next += 1;
                (piece, state.spare.pop().unwrap_or_default())
            };
            lex_piece(&mut lexer, &nodes[cuts[piece]..cuts[piece + 1]], &mut recording);
            let mut state = self.lock();
            let slot = piece - state.taken;
            if state.done.len() <= slot {
                state.done.resize_with(slot + 1, || None);
            }
            state.done[slot] = Some(recording);
            drop(state);
            self.ready.notify_one();
        }
    }

    /// The next piece in order, once lexed; `used`, the storage of the
    /// one before, is kept for a later piece.
    pub(crate) fn take(&self, mut used: Recording) -> Recording {
        used.clear();
        let mut state = self.lock();
        state.spare.push(used);
        let recording = loop {
            if let Some(slot) = state.done.front_mut()
                && let Some(recording) = slot.take()
            {
                state.done.pop_front();
                break recording;
            }
            state = self.ready.wait(state).unwrap_or_else(PoisonError::into_inner);
        };
        state.taken += 1;
        drop(state);
        self.room.notify_all();
        recording
    }

    /// No more pieces are wanted: the lexing threads stop.
    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.room.notify_all();
    }
}

/// Closes its feed when dropped, also when the node loop ends early (an
/// error, or a panic unwinding), so that no lexing thread waits for room.
pub(crate) struct Closing<'f>(pub(crate) &'f Feed);

impl Drop for Closing<'_> {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{Feed, cuts, lex_piece, lexed_ahead};
    use crate::ParseOptions;
    use crate::index::{NodeEnt, scan};
    use crate::lexer::{Lexer, Recording, Tok, Token};
    use crate::parser::run_split;

    /// Every seed of `tests/ir` and every `.ll` of this crate's test data
    /// that has debug metadata.
    fn texts() -> Vec<(String, Vec<u8>)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut texts = Vec::new();
        for dir in
            [root.join("../../tests/ir"), root.join("tests/data"), root.join("tests/data/debug")]
        {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            paths.sort();
            for path in paths {
                if path.extension().is_some_and(|ext| ext == "ll") {
                    let text = std::fs::read(&path).unwrap();
                    if text.windows(13).any(|w| w == b"DICompileUnit") {
                        texts.push((path.display().to_string(), text));
                    }
                }
            }
        }
        assert!(texts.len() >= 2, "the texts with debug metadata are found");
        texts.push(("synthetic".to_string(), synthetic(60).into_bytes()));
        texts
    }

    /// A module whose metadata names tuples ahead of and behind the
    /// nodes that name them, writes nodes and locations in place, and
    /// has strings with escapes and comments between nodes: every move
    /// of the node loop.
    fn synthetic(count: usize) -> String {
        use std::fmt::Write as _;
        let mut out = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n");
        for index in 0..count {
            let sp = 20 + 10 * index;
            let _ = write!(
                out,
                "define i32 @f{index}(i32 %a) !dbg !{sp} {{\nentry:\n  \
                 #dbg_value(i32 %a, !{var}, !DIExpression(), !{loc})\n  \
                 ret i32 %a, !dbg !{loc}\n}}\n",
                var = sp + 1,
                loc = sp + 2
            );
        }
        let last = 20 + 10 * count;
        let _ = write!(
            out,
            "!llvm.dbg.cu = !{{!0}}\n!llvm.module.flags = !{{!1, !2}}\n\
             !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, \
             producer: \"lex \\22ahead\\22\", isOptimized: false, runtimeVersion: 0, \
             emissionKind: FullDebug, retainedTypes: !{last})\n\
             !1 = !{{i32 7, !\"Dwarf Version\", i32 5}}\n\
             !2 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
             !3 = !DIFile(filename: \"ahead.cpp\", directory: \"/ahead\")\n\
             !4 = !DISubroutineType(types: !5)\n!5 = !{{!6, !6}}\n\
             !6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n\
             !7 = !DIExpression(DW_OP_plus_uconst, 8)\n"
        );
        for index in 0..count {
            let sp = 20 + 10 * index;
            let line = index + 1;
            let _ = write!(
                out,
                "!{sp} = distinct !DISubprogram(name: \"f{index}\", linkageName: \"_Z1f{index}i\", \
                 scope: !3, file: !3, line: {line}, type: !4, scopeLine: {line}, \
                 spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !{nodes})\n\
                 !{var} = !DILocalVariable(name: \"a\", arg: 1, scope: !{sp}, file: !3, \
                 line: {line}, type: !6)\n\
                 !{loc} = !DILocation(line: {line}, column: 7, scope: !{sp}, \
                 inlinedAt: !DILocation(line: 1, scope: !{sp}))\n\
                 ; a comment between nodes\n\
                 !{elems} = !{{!{m1}, !{m2}}}\n\
                 !{ty} = distinct !DICompositeType(tag: DW_TAG_structure_type, \
                 name: \"S{index}\\09x\", file: !3, line: {line}, size: 64, elements: !{elems}, \
                 identifier: \"_ZTS1S{index}\")\n\
                 !{m1} = !DIDerivedType(tag: DW_TAG_member, name: \"x\", scope: !{ty}, \
                 baseType: !6, size: 32)\n\
                 !{m2} = !DIDerivedType(tag: DW_TAG_member, name: \"y\", scope: !{ty}, \
                 baseType: !6, size: 32, offset: 32)\n\
                 !{nodes} = !{{!{var}}}\n\
                 !{g} = !DIGlobalVariableExpression(var: !DIGlobalVariable(name: \"g{index}\", \
                 scope: !0, file: !3, line: {line}, type: !6, isLocal: false, \
                 isDefinition: true), expr: !7)\n",
                var = sp + 1,
                loc = sp + 2,
                ty = sp + 3,
                m1 = sp + 4,
                m2 = sp + 5,
                nodes = sp + 6,
                g = sp + 7,
                elems = sp + 8,
            );
        }
        let types: Vec<String> = (0..count).map(|index| format!("!{}", 23 + 10 * index)).collect();
        let _ = writeln!(out, "!{last} = !{{{}}}", types.join(", "));
        out
    }

    /// The tokens of one node's body as the node loop asks for them, with
    /// the lexer's message for an error token.
    fn body(lexer: &mut Lexer<'_>, node: &NodeEnt, seek: bool) -> Vec<(Token, &'static str)> {
        if seek {
            lexer.seek(node.span.start as usize);
        } else {
            lexer.set_pos(node.span.start as usize);
        }
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            let error = if token.kind == Tok::Error { lexer.error() } else { "" };
            tokens.push((token, error));
            if matches!(token.kind, Tok::Eof | Tok::Error) || token.start >= node.span.end {
                return tokens;
            }
        }
    }

    /// The node loop over `text` with the node spans `nodes`, lexed, and
    /// replayed from pieces of `piece_bytes`, moving ahead `jump` nodes
    /// and back before each node (as `define_tuples` does): the same
    /// tokens.
    fn replay_is_lexing(name: &str, text: &[u8], nodes: &[NodeEnt], piece_bytes: usize) {
        let cuts = cuts(nodes, piece_bytes);
        let mut plain = Lexer::new(text);
        let mut replaying = Lexer::new(text);
        let mut ahead = Lexer::new(text);
        for piece in cuts.windows(2) {
            let mut recording = Recording::default();
            lex_piece(&mut ahead, &nodes[piece[0]..piece[1]], &mut recording);
            replaying.replay(recording);
            for at in piece[0]..piece[1] {
                if !lexed_ahead(nodes[at].kind) {
                    continue;
                }
                let jump = (at * 7 + 3) % 5;
                if let Some(later) = nodes.get(at + jump) {
                    let lexed = body(&mut plain, later, false);
                    assert_eq!(
                        lexed,
                        body(&mut replaying, later, true),
                        "{name}: node {at}+{jump}"
                    );
                }
                let lexed = body(&mut plain, &nodes[at], false);
                assert_eq!(lexed, body(&mut replaying, &nodes[at], true), "{name}: node {at}");
            }
        }
    }

    /// §4.5: the replayed token stream is the lexed one, on every seed
    /// with debug metadata, for several piece sizes; and on the same
    /// texts with a byte changed inside the nodes (an unterminated
    /// string, a stray byte, a bracket) and cut short, the nodes as the
    /// whole text's scan found them.
    #[test]
    fn replayed_tokens_are_the_lexed_ones() {
        for (name, text) in texts() {
            let nodes = scan(&text).expect("a seed scans").nodes;
            for piece in [1, 97, 4096, usize::MAX] {
                replay_is_lexing(&name, &text, &nodes, piece);
            }
            let Some(first) = nodes.first() else { continue };
            let from = first.span.start as usize;
            let step = (text.len() - from) / 23 + 1;
            for at in (from..text.len()).step_by(step) {
                for bad in [b'"', b'`', b'(', b'!', b'\\'] {
                    let mut changed = text.clone();
                    changed[at] = bad;
                    replay_is_lexing(&name, &changed, &nodes, 211);
                }
                replay_is_lexing(&name, &text[..at], &nodes, 211);
            }
        }
    }

    /// The whole parse, as text: the module and its tables, or the error
    /// with its offset and message.
    fn parsed(text: &[u8], threads: usize, piece_bytes: usize) -> String {
        let result = run_split(text, &ParseOptions::default(), threads, piece_bytes)
            .map_err(|failure| failure.locate(text));
        format!("{result:?}")
    }

    /// §4.5: the module parsed with the debug nodes lexed ahead is the
    /// serial one, on every seed with debug metadata, at several thread
    /// counts and piece sizes; for a text with an error in its nodes (or
    /// cut short), the same first error.
    #[test]
    fn lexing_ahead_is_the_serial_parse() {
        for (name, text) in texts() {
            let serial = parsed(&text, 1, usize::MAX);
            if name == "synthetic" {
                assert!(serial.starts_with("Ok("), "{}", &serial[..serial.len().min(300)]);
            }
            for (threads, piece) in [(2, 1), (2, 300), (4, 64), (8, 5000)] {
                assert_eq!(serial, parsed(&text, threads, piece), "{name}, {threads} threads");
            }
        }
        let texts = texts();
        let (name, text) = texts.iter().max_by_key(|(_, text)| text.len()).unwrap();
        let from = scan(text).unwrap().nodes[0].span.start as usize;
        let step = (text.len() - from) / 41 + 1;
        for at in (from..text.len()).step_by(step) {
            for bad in [b'"', b'`', b'{', b'!', b'0'] {
                let mut changed = text.clone();
                changed[at] = bad;
                let serial = parsed(&changed, 1, usize::MAX);
                assert_eq!(serial, parsed(&changed, 3, 150), "{name}: {bad:?} at {at}");
            }
            let cut = &text[..at];
            assert_eq!(parsed(cut, 1, usize::MAX), parsed(cut, 2, 90), "{name} cut at {at}");
        }
    }

    /// A feed whose reader stops early (an error) lets its lexing threads
    /// end: the scope joins.
    #[test]
    fn a_closed_feed_stops_its_threads() {
        let texts = texts();
        let (_, text) = texts.iter().max_by_key(|(_, text)| text.len()).unwrap();
        let nodes = scan(text).unwrap().nodes;
        let cuts = cuts(&nodes, 1);
        let feed = Feed::new(cuts.len() - 1, 3);
        std::thread::scope(|scope| {
            for _ in 0..3 {
                scope.spawn(|| feed.work(text, &nodes, &cuts));
            }
            let _closing = super::Closing(&feed);
            let first = feed.take(Recording::default());
            assert!(
                first.last_from().is_some() || nodes[0].kind == crate::meta::MdKind::Expression
            );
        });
    }
}
