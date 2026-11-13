//! Option sets of the generator (`--profile`), and the census of what
//! their programs' IR contains (`--census`). Contract: `spec/harness.md`
//! "Random programs", "Profiles", "Census".

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::case::Flavour;

/// What makes a profile's programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generator {
    /// csmith: `base` options, then the seed's flavour's, and a
    /// probability configuration when it is not empty (csmith's
    /// `--probability-configuration`, written to the seed's directory).
    Csmith {
        base: &'static [&'static str],
        flavours: &'static [Flavour],
        probabilities: &'static str,
    },
    /// [`super::abi::program`].
    Abi,
}

/// A named option set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: &'static str,
    /// One line for the help.
    pub about: &'static str,
    pub generator: Generator,
}

impl Profile {
    /// The flavour of `seed`: `seed % flavours` (the ABI generator has one).
    pub fn flavour(&self, seed: u64) -> Flavour {
        match self.generator {
            Generator::Csmith { flavours, .. } => {
                let at = usize::try_from(seed % flavours.len() as u64).unwrap_or(0);
                flavours[at]
            }
            Generator::Abi => Flavour { name: "abi", extra: &[] },
        }
    }

    /// The default profile: WP-154's options.
    pub fn default_profile() -> &'static Self {
        &PROFILES[0]
    }

    pub fn by_name(name: &str) -> Option<&'static Self> {
        PROFILES.iter().find(|p| p.name == name)
    }
}

/// The sizes of WP-154's run.
const SMALL: &[&str] =
    &["--max-funcs", "6", "--max-block-depth", "4", "--max-array-len-per-dim", "6"];

const C: Flavour = Flavour { name: "c", extra: &[] };
const C_FLOAT: Flavour = Flavour { name: "c-float", extra: &["--float"] };

/// The profiles; the first is the default.
pub const PROFILES: [Profile; 7] = [
    Profile {
        name: "default",
        about: "WP-154's options: small programs, --float for one seed in three",
        generator: Generator::Csmith { base: SMALL, flavours: &[C, C, C_FLOAT], probabilities: "" },
    },
    Profile {
        name: "aggregates",
        about: "structs and unions everywhere: as values, arguments, returns, packed, bit-fields",
        generator: Generator::Csmith {
            base: &[
                "--max-funcs",
                "8",
                "--max-block-depth",
                "4",
                "--max-array-len-per-dim",
                "6",
                "--max-struct-fields",
                "10",
                "--max-union-fields",
                "5",
                "--max-struct-nested-level",
                "3",
                "--fixed-struct-fields",
                "--packed-struct",
                "--bitfields",
                "--arg-structs",
                "--return-structs",
                "--arg-unions",
                "--return-unions",
                "--vol-struct-union-fields",
            ],
            flavours: &[C],
            probabilities: "more_struct_union_type_prob=90\n\nbitfields_creation_prob=70\n\n\
                            bitfield_in_normal_struct_prob=50\n\nstruct_as_ltype_prob=60\n\n\
                            union_as_ltype_prob=50\n",
        },
    },
    Profile {
        name: "float",
        about: "--float in every program, floating-point values as left-hand sides",
        generator: Generator::Csmith {
            base: SMALL,
            flavours: &[C_FLOAT],
            probabilities: "float_as_ltype_prob=40\n",
        },
    },
    Profile {
        name: "wide",
        about: "larger programs: wider and more arrays, deeper blocks and pointers, more functions",
        generator: Generator::Csmith {
            base: &[
                "--max-funcs",
                "10",
                "--max-block-depth",
                "5",
                "--max-array-len-per-dim",
                "10",
                "--max-array-dim",
                "3",
                "--max-pointer-depth",
                "3",
                "--max-block-size",
                "5",
            ],
            flavours: &[C, C_FLOAT],
            probabilities: "new_array_var_prob=50\n",
        },
    },
    Profile {
        name: "volatile",
        about: "volatile objects, fields and pointers everywhere",
        generator: Generator::Csmith {
            base: &[
                "--max-funcs",
                "6",
                "--max-block-depth",
                "4",
                "--max-array-len-per-dim",
                "6",
                "--volatiles",
                "--volatile-pointers",
                "--vol-struct-union-fields",
                "--strict-volatile-rule",
            ],
            flavours: &[C],
            probabilities: "regular_volatile_prob=90\n\nfield_volatile_prob=70\n\naccess_once_var_prob=50\n",
        },
    },
    Profile {
        name: "narrow",
        about: "narrow integers: 8-bit and 16-bit types only, no 64-bit math",
        generator: Generator::Csmith {
            base: &[
                "--max-funcs",
                "6",
                "--max-block-depth",
                "4",
                "--max-array-len-per-dim",
                "6",
                "--no-longlong",
                "--no-math64",
                "--int8",
                "--uint8",
            ],
            flavours: &[C],
            probabilities: "(simple_types_prob,void_prob=0,char_prob=1,int_prob=0,short_prob=1,\
                            long_prob=0,long_long_prob=0,uchar_prob=1,uint_prob=0,ushort_prob=1,\
                            ulong_prob=0,ulong_long_prob=0,float_prob=0)\n\n\
                            (safe_ops_size_prob,safe_ops_size_int8=1,safe_ops_size_int16=1,\
                            safe_ops_size_int32=0,safe_ops_size_int64=0)\n",
        },
    },
    Profile {
        name: "abi",
        about: "hod's own generator: calls with many arguments, aggregates by value, variadic \
                functions, long double, __int128, bit-fields (fuzz/abi.rs)",
        generator: Generator::Abi,
    },
];

/// What the IR of a profile's programs contains: for each feature, the
/// programs that have it and its occurrences.
#[derive(Clone, Debug, Default)]
pub struct Census {
    pub programs: usize,
    pub features: BTreeMap<String, (usize, usize)>,
}

impl Census {
    /// Adds one module's features.
    pub fn add(&mut self, module: &BTreeMap<String, usize>) {
        self.programs += 1;
        for (feature, count) in module {
            let entry = self.features.entry(feature.clone()).or_default();
            entry.0 += 1;
            entry.1 += count;
        }
    }

    /// The table: one line per feature, by group, `programs occurrences`.
    pub fn text(&self) -> String {
        let mut text = format!("census: {} programs\n", self.programs);
        let _ = writeln!(text, "  {:<40} {:>8} {:>10}", "feature", "programs", "count");
        for (feature, (programs, count)) in &self.features {
            let _ = writeln!(text, "  {feature:<40} {programs:>8} {count:>10}");
        }
        text
    }
}

/// The features that matter for the calling convention and the support
/// matrix, named as `group:name`: `op:<opcode>`, `type:<type>` (integer
/// widths, floating-point types, `vector`, `packed-struct`),
/// `attr:<parameter attribute>`, `call:varargs`, `define:varargs`,
/// `intrinsic:<name>`, `param:aggregate`, `ret:aggregate`,
/// `volatile:load`/`store`.
pub fn scan(text: &str) -> BTreeMap<String, usize> {
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |key: String| *found.entry(key).or_default() += 1;
    let mut in_function = false;
    for line in text.lines() {
        // Metadata and strings name no type of the program.
        if line.starts_with('!') || line.starts_with(';') {
            continue;
        }
        let code = without_strings(line);
        types(&code, &mut add);
        for attribute in ["byval", "sret", "inreg", "byref", "signext", "zeroext", "inalloca"] {
            let count = code.match_indices(attribute).count();
            for _ in 0..count {
                add(format!("attr:{attribute}"));
            }
        }
        if code.contains("<{") && code.contains(" = type ") {
            add("type:packed-struct".to_owned());
        }
        if let Some(header) = code.strip_prefix("define ") {
            in_function = true;
            let (before_name, params) = header.split_once('(').unwrap_or((header, ""));
            if params.contains("...") {
                add("define:varargs".to_owned());
            }
            let ret = before_name.rsplit_once(" @").map_or("", |(r, _)| r);
            if ret.ends_with('}')
                || ret.ends_with(']')
                || ret.ends_with(">")
                || ret.contains("%struct")
            {
                add("ret:aggregate".to_owned());
            }
            for param in top_level(params.rsplit_once(')').map_or(params, |(p, _)| p)) {
                let param = param.trim_start();
                if param.starts_with('{')
                    || param.starts_with('[')
                    || param.starts_with('<')
                    || param.starts_with('%')
                {
                    add("param:aggregate".to_owned());
                }
            }
            continue;
        }
        if code == "}" {
            in_function = false;
            continue;
        }
        if !in_function {
            continue;
        }
        let instruction = code.trim_start();
        if instruction.is_empty() || instruction.ends_with(':') || instruction.starts_with("#dbg_")
        {
            continue;
        }
        // A block label with a comment of its predecessors.
        if instruction
            .split_once(':')
            .is_some_and(|(label, rest)| !label.contains(' ') && rest.trim_start().starts_with(';'))
        {
            continue;
        }
        let rest = match instruction.split_once(" = ") {
            Some((name, rest)) if name.starts_with('%') => rest,
            _ => instruction,
        };
        let mut words = rest.split_whitespace();
        let mut opcode = words.next().unwrap_or("");
        if matches!(opcode, "tail" | "musttail" | "notail") {
            opcode = words.next().unwrap_or("");
        }
        if opcode.is_empty() {
            continue;
        }
        add(format!("op:{opcode}"));
        if matches!(opcode, "load" | "store") && rest.contains(" volatile ") {
            add(format!("volatile:{opcode}"));
        }
        if opcode == "call" || opcode == "invoke" {
            if let Some(at) = rest.find("@llvm.") {
                let name: String = rest[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
                    .collect();
                // `llvm.memcpy.p0.p0.i64` is `llvm.memcpy`.
                let short: Vec<&str> = name.split('.').take(2).collect();
                add(format!("intrinsic:{}", short.join(".")));
            }
            let callee = rest.split_once('@').map_or(rest, |(before, _)| before);
            if callee.contains("...)") {
                add("call:varargs".to_owned());
            }
        }
    }
    found
}

/// `line` with the text of its quoted strings removed.
fn without_strings(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
            out.push(c);
        } else if !quoted {
            out.push(c);
        }
    }
    out
}

/// The types `code` names: integer widths, floating-point types, vectors.
fn types(code: &str, add: &mut impl FnMut(String)) {
    let bytes = code.as_bytes();
    let word_char =
        |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'%' || b == b'@';
    let mut at = 0;
    while at < bytes.len() {
        if word_char(bytes[at]) {
            let start = at;
            while at < bytes.len() && word_char(bytes[at]) {
                at += 1;
            }
            let word = &code[start..at];
            if let Some(width) = word.strip_prefix('i')
                && !width.is_empty()
                && width.bytes().all(|b| b.is_ascii_digit())
            {
                add(format!("type:i{width}"));
            } else if matches!(word, "half" | "bfloat" | "float" | "double" | "x86_fp80" | "fp128")
            {
                add(format!("type:{word}"));
            }
        } else {
            if bytes[at] == b'<'
                && code[at + 1..]
                    .split_once(" x ")
                    .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            {
                add("type:vector".to_owned());
            }
            at += 1;
        }
    }
}

/// The items of a parameter or argument list, split at its top-level
/// commas.
pub fn top_level(list: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, c) in list.char_indices() {
        match c {
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !list[start..].trim().is_empty() {
        items.push(&list[start..]);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "Random programs", "Census": what is counted.
    #[test]
    fn census_of_a_module() {
        let module = "\
%struct.S = type <{ i8, i32 }>
@g = global x86_fp80 0xK0, align 16
@.str = private constant [6 x i8] c\"float\\00\"
define { i64, i32 } @f(ptr byval(%struct.S) align 8 %0, <2 x float> %1, i128 %2, ...) {
entry:
  %3 = load volatile i32, ptr %0, align 4
  %4 = tail call i32 (ptr, ...) @printf(ptr @.str, i32 %3)
  call void @llvm.memcpy.p0.p0.i64(ptr %0, ptr %0, i64 8, i1 false)
  br label %5

5:                                                ; preds = %entry
  ret { i64, i32 } zeroinitializer
}
";
        let found = scan(module);
        for (key, count) in [
            ("type:packed-struct", 1),
            ("type:x86_fp80", 1),
            ("type:i128", 1),
            ("type:vector", 1),
            ("attr:byval", 1),
            ("define:varargs", 1),
            ("ret:aggregate", 1),
            ("param:aggregate", 1),
            ("op:load", 1),
            ("op:call", 2),
            ("op:br", 1),
            ("op:ret", 1),
            ("volatile:load", 1),
            ("call:varargs", 1),
            ("intrinsic:llvm.memcpy", 1),
        ] {
            assert_eq!(found.get(key), Some(&count), "{key}: {found:?}");
        }
        // The string's text is no type.
        assert_eq!(found.get("type:float"), Some(&1), "{found:?}");
        let mut census = Census::default();
        census.add(&found);
        census.add(&found);
        assert_eq!(census.features["op:call"], (2, 4));
        assert!(census.text().contains("op:call"));
    }

    #[test]
    fn profiles_are_named_once() {
        assert_eq!(Profile::default_profile().name, "default");
        for (i, p) in PROFILES.iter().enumerate() {
            assert!(PROFILES[i + 1..].iter().all(|q| q.name != p.name), "{}", p.name);
            assert_eq!(Profile::by_name(p.name), Some(p));
        }
        assert_eq!(
            top_level("i32 %a, { i8, i16 } %b, ptr byval(%s) %c"),
            ["i32 %a", " { i8, i16 } %b", " ptr byval(%s) %c"]
        );
    }
}
