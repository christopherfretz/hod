//! A reader of the debug metadata of LLVM text, enough for the DIE tests
//! (`spec/dwarf.md` §8.6): `!N = [distinct] !DIKind(...)` nodes, tuples,
//! `!llvm.dbg.cu`, and the `!dbg` attachment of `@global` lines. It fills
//! `hod_ir`'s tables with the builder API (`reserve`, `define`,
//! `DiNode::set`), as `hod_ll` will once it reads debug metadata (WP-054).
//! Fields `hod_ir` does not map are skipped, as a producer that consults
//! the field table does.

use std::collections::HashMap;

use hod_ir::{
    ChecksumKind, DebugInfo, DiFieldValue, DiFlags, DiId, DiKind, DiNode, EmissionKind, ExprId,
    NameTableKind, SpFlags, dw_ate, dw_cc, dw_lang, dw_op, dw_tag,
};

/// What [`load`] read.
pub struct Loaded {
    pub debug: DebugInfo,
    pub units: Vec<DiId>,
    /// `(symbol name, DIGlobalVariableExpression)` per `!dbg` of a global.
    pub globals: Vec<(String, DiId)>,
}

enum Def<'t> {
    Node { distinct: bool, kind: &'t str, args: &'t str },
    Tuple(Vec<Option<u32>>),
    Other,
}

/// Splits at top-level commas: not inside quotes or parentheses.
fn split_args(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start) = (0, false, 0);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if quoted => i += 1,
            b'"' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => depth -= 1,
            b',' if !quoted && depth == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if !s[start..].trim().is_empty() {
        out.push(s[start..].trim());
    }
    out
}

fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'\\') {
            out.push(b'\\');
            i += 2;
        } else if b[i] == b'\\' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).expect("an escape"));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn node_number(s: &str) -> Option<u32> {
    s.strip_prefix('!').and_then(|n| n.parse().ok())
}

/// The number a constant name stands for.
fn constant(name: &str) -> Option<u64> {
    let one = |n: &str| -> Option<u64> {
        if n.starts_with("DIFlag") {
            return DiFlags::named(n.as_bytes()).map(|f| u64::from(f.0));
        }
        if n.starts_with("DISPFlag") {
            return SpFlags::named(n.as_bytes()).map(|f| u64::from(f.0));
        }
        let b = n.as_bytes();
        dw_tag(b)
            .or_else(|| dw_ate(b))
            .or_else(|| dw_lang(b))
            .or_else(|| dw_cc(b))
            .map(u64::from)
            .or_else(|| ChecksumKind::named(b).map(|k| u64::from(k.code())))
            .or_else(|| EmissionKind::named(b).map(|k| u64::from(k.code())))
            .or_else(|| NameTableKind::named(b).map(|k| u64::from(k.code())))
    };
    let mut total = 0;
    for part in name.split('|') {
        total |= one(part.trim())?;
    }
    Some(total)
}

fn expression(debug: &mut DebugInfo, args: &str) -> ExprId {
    let elements: Vec<u64> = split_args(args)
        .iter()
        .map(|e| {
            e.parse::<u64>()
                .ok()
                .or_else(|| dw_op(e.as_bytes()))
                .unwrap_or_else(|| panic!("expression element {e}"))
        })
        .collect();
    debug.expr_from_elements(&elements).expect("an expression")
}

/// Reads the metadata of `text` into a new `DebugInfo`.
pub fn load(text: &str) -> Loaded {
    let mut defs: HashMap<u32, Def<'_>> = HashMap::new();
    let mut order = Vec::new();
    let mut units = Vec::new();
    let mut attached = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("!llvm.dbg.cu = !{") {
            units = split_args(rest.trim_end_matches('}'))
                .iter()
                .map(|u| node_number(u).expect("a unit"))
                .collect();
            continue;
        }
        if let Some(rest) = line.strip_prefix('@') {
            if let (Some((name, _)), Some(at)) = (rest.split_once(" = "), rest.find("!dbg !")) {
                let n: String = rest[at + 6..].chars().take_while(char::is_ascii_digit).collect();
                let name = String::from_utf8(unescape(name.trim_matches('"'))).unwrap();
                attached.push((name, n.parse::<u32>().unwrap()));
            }
            continue;
        }
        let Some((lhs, rhs)) = line.split_once(" = ") else { continue };
        let Some(n) = node_number(lhs) else { continue };
        let (distinct, rhs) = match rhs.strip_prefix("distinct ") {
            Some(r) => (true, r),
            None => (false, rhs),
        };
        let def = if let Some(body) = rhs.strip_prefix("!{") {
            let body = body.trim_end_matches('}');
            let mut elems = Vec::new();
            let mut di = true;
            for e in split_args(body) {
                match e {
                    "null" => elems.push(None),
                    _ => match node_number(e) {
                        Some(m) => elems.push(Some(m)),
                        None => di = false,
                    },
                }
            }
            if di { Def::Tuple(elems) } else { Def::Other }
        } else if let Some(body) = rhs.strip_prefix('!') {
            let open = body.find('(').expect("a node");
            let kind = &body[..open];
            let args = &body[open + 1..body.rfind(')').expect("a node")];
            Def::Node { distinct, kind, args }
        } else {
            Def::Other
        };
        order.push(n);
        defs.insert(n, def);
    }

    let mut debug = DebugInfo::default();
    let mut ids = HashMap::new();
    for &n in &order {
        if let Def::Node { kind, .. } = &defs[&n]
            && DiKind::named(kind.as_bytes()).is_some()
        {
            ids.insert(n, debug.reserve().unwrap());
        }
    }
    let lists: HashMap<u32, Vec<DiId>> = defs
        .iter()
        .filter_map(|(&n, d)| match d {
            Def::Tuple(elems) => {
                Some((n, elems.iter().map(|e| e.map_or(DiId::NONE, |m| ids[&m])).collect()))
            }
            _ => None,
        })
        .collect();
    for &n in &order {
        let Def::Node { distinct, kind, args } = &defs[&n] else { continue };
        let Some(kind) = DiKind::named(kind.as_bytes()) else { continue };
        let mut node = DiNode::llvm_default(kind);
        let mut strings: Vec<Vec<u8>> = Vec::new();
        let mut exprs = Vec::new();
        // First pass: the values that need storage that outlives `node`.
        for arg in split_args(args) {
            let (_, value) = arg.split_once(':').expect("a field");
            let value = value.trim();
            if let Some(s) = value.strip_prefix('"') {
                strings.push(unescape(s.strip_suffix('"').unwrap()));
            } else if let Some(e) = value.strip_prefix("!DIExpression(") {
                exprs.push(expression(&mut debug, e.strip_suffix(')').unwrap()));
            }
        }
        let (mut si, mut ei) = (0, 0);
        for arg in split_args(args) {
            let (field, value) = arg.split_once(':').unwrap();
            let (field, value) = (field.trim(), value.trim());
            let v = if value.starts_with('"') {
                si += 1;
                DiFieldValue::Str(&strings[si - 1])
            } else if value.starts_with("!DIExpression(") {
                ei += 1;
                DiFieldValue::Expr(exprs[ei - 1])
            } else if value == "null" {
                DiFieldValue::Node(None)
            } else if let Some(m) = node_number(value) {
                match lists.get(&m) {
                    Some(list) => DiFieldValue::List(list),
                    None => match ids.get(&m) {
                        Some(&id) => DiFieldValue::Node(Some(id)),
                        None => continue, // a non-debug node
                    },
                }
            } else if value == "true" || value == "false" {
                DiFieldValue::Bool(value == "true")
            } else if let Ok(i) = value.parse::<i64>() {
                if i < 0 { DiFieldValue::Int(i) } else { DiFieldValue::UInt(i as u64) }
            } else if let Ok(u) = value.parse::<u64>() {
                DiFieldValue::UInt(u)
            } else if let Some(c) = constant(value) {
                DiFieldValue::UInt(c)
            } else {
                panic!("{field}: {value}")
            };
            if !kind.fields().contains(&field) && !matches!(field, "isLocal" | "isDefinition") {
                continue;
            }
            node.set(field.as_bytes(), v).unwrap_or_else(|e| panic!("!{n}: {e:?}"));
        }
        let id = ids[&n];
        if *distinct {
            debug.define_distinct(id, &node).unwrap();
        } else {
            debug.define(id, &node).unwrap();
        }
    }
    let units: Vec<DiId> = units.iter().map(|u| ids[u]).collect();
    for &u in &units {
        debug.add_compile_unit(u).unwrap();
    }
    debug.verify().unwrap();
    let globals = attached.into_iter().map(|(s, n)| (s, ids[&n])).collect();
    Loaded { debug, units, globals }
}
