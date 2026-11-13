//! The records of a benchmark run and their file. Contract:
//! `spec/harness.md` "bench", "Results file" and "Machine".
//!
//! One format serves the results of a run and a committed baseline: header
//! lines `# <key><TAB><value>`, then one line of [`FIELDS`] tab-separated
//! fields per input and tool. Every number is an integer, so a file read
//! and written again is the same bytes.

use std::fmt::Write as _;
use std::path::Path;

/// The format's version, the value of the first header line.
pub const FORMAT: &str = "hod-bench 1";

/// The names of a line's fields, in order.
pub const FIELDS: [&str; 26] = [
    "status",
    "input",
    "class",
    "debug",
    "tool",
    "ir_bytes",
    "functions",
    "ir_hash",
    "runs",
    "wall_us_min",
    "wall_us_median",
    "wall_us_max",
    "cpu_us",
    "rss_kib",
    "rss_kib_min",
    "rss_kib_max",
    "object_bytes",
    "wall_ratio",
    "cpu_ratio",
    "rss_ratio",
    "size_ratio",
    "ir_bytes_per_s",
    "functions_per_s",
    "phases",
    "detail",
    "instructions",
];

/// The fields of the files written before `instructions` was added
/// (WP-153): read, never written.
const FIELDS_BEFORE_INSTRUCTIONS: usize = 25;

/// What a tool did with an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The tool wrote the object itself.
    Ok,
    /// hod handed the module to its fallback, which wrote the object.
    Fallback,
    /// hod declined the module (strict mode); nothing was written.
    Declined,
    /// The tool, or the production of the input, failed. No numbers.
    Error,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Fallback => "fallback",
            Self::Declined => "declined",
            Self::Error => "error",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Ok, Self::Fallback, Self::Declined, Self::Error]
            .into_iter()
            .find(|s| s.name() == text)
    }
}

/// A tool configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tool {
    /// The production of the input; only ever in an `error` record.
    Generate,
    /// `llc -O0 -filetype=obj`, the reference.
    Llc,
    /// hod in strict mode: what hod does by itself.
    Hod,
    /// hod with `llc` as its fallback: what a hybrid build runs.
    HodFallback,
}

impl Tool {
    /// The configurations that are measured, in the order of a round.
    pub const MEASURED: [Tool; 3] = [Tool::Llc, Tool::Hod, Tool::HodFallback];

    pub fn name(self) -> &'static str {
        match self {
            Self::Generate => "generate",
            Self::Llc => "llc",
            Self::Hod => "hod",
            Self::HodFallback => "hod+fallback",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Generate, Self::Llc, Self::Hod, Self::HodFallback]
            .into_iter()
            .find(|t| t.name() == text)
    }
}

/// An input's size class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Small,
    Typical,
    Large,
    Pathological,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Typical => "typical",
            Self::Large => "large",
            Self::Pathological => "pathological",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Small, Self::Typical, Self::Large, Self::Pathological]
            .into_iter()
            .find(|c| c.name() == text)
    }
}

/// A ratio to `llc`'s figure, in units of 1/10000 (`1.0000` is equal).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ratio(pub u64);

impl Ratio {
    /// `value / reference`, rounded to the nearest 1/10000; `None` when the
    /// reference is zero.
    pub fn of(value: u64, reference: u64) -> Option<Self> {
        if reference == 0 {
            return None;
        }
        let scaled = u128::from(value) * 10_000 + u128::from(reference) / 2;
        Some(Self(u64::try_from(scaled / u128::from(reference)).unwrap_or(u64::MAX)))
    }

    fn parse(text: &str) -> Option<Self> {
        let (whole, fraction) = text.split_once('.')?;
        if fraction.len() != 4 || !all_digits(whole) || !all_digits(fraction) {
            return None;
        }
        Some(Self(whole.parse::<u64>().ok()?.checked_mul(10_000)? + fraction.parse::<u64>().ok()?))
    }
}

impl std::fmt::Display for Ratio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{:04}", self.0 / 10_000, self.0 % 10_000)
    }
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// One line: an input measured with one tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub status: Status,
    /// The input's name (`stl-heavy`, `stl-heavy-g`).
    pub input: String,
    pub class: Class,
    /// Whether the input carries debug information.
    pub debug: bool,
    pub tool: Tool,
    /// The size of the IR file. With the two fields after it, `None` only
    /// when the input could not be produced.
    pub ir_bytes: Option<u64>,
    /// The number of function definitions in the IR.
    pub functions: Option<u64>,
    /// `fnv1a64:<16 hex digits>` of the IR file.
    pub ir_hash: Option<String>,
    /// How many runs the numbers are taken from; 0 for an error.
    pub runs: u32,
    /// Wall-clock microseconds of the fastest run, the middle one, the
    /// slowest.
    pub wall_us_min: Option<u64>,
    pub wall_us_median: Option<u64>,
    pub wall_us_max: Option<u64>,
    /// User plus system CPU microseconds, the smallest of the runs.
    pub cpu_us: Option<u64>,
    /// Peak resident memory in KiB: the middle run, the smallest, the
    /// largest.
    pub rss_kib: Option<u64>,
    pub rss_kib_min: Option<u64>,
    pub rss_kib_max: Option<u64>,
    /// The size of the object file; `None` when none was written.
    pub object_bytes: Option<u64>,
    /// This record over `llc`'s for the same input in the same run.
    pub wall_ratio: Option<Ratio>,
    pub cpu_ratio: Option<Ratio>,
    pub rss_ratio: Option<Ratio>,
    pub size_ratio: Option<Ratio>,
    /// hod's phase times of the fastest run, microseconds, in hod's order.
    pub phases: Vec<(String, u64)>,
    /// The reason of a decline or a fallback; what failed, for an error.
    pub detail: String,
    /// The instructions one run executed, counted by `valgrind
    /// --tool=callgrind` (`--instructions`): a figure the load of the
    /// machine does not change.
    pub instructions: Option<u64>,
}

impl Record {
    /// A record with no numbers.
    pub fn blank(status: Status, input: &str, class: Class, debug: bool, tool: Tool) -> Self {
        Self {
            status,
            input: input.to_owned(),
            class,
            debug,
            tool,
            ir_bytes: None,
            functions: None,
            ir_hash: None,
            runs: 0,
            wall_us_min: None,
            wall_us_median: None,
            wall_us_max: None,
            cpu_us: None,
            rss_kib: None,
            rss_kib_min: None,
            rss_kib_max: None,
            object_bytes: None,
            wall_ratio: None,
            cpu_ratio: None,
            rss_ratio: None,
            size_ratio: None,
            phases: Vec::new(),
            detail: String::new(),
            instructions: None,
        }
    }

    /// Whether the record is a time for producing an object (by the tool
    /// or its fallback), which is what throughput and ratios are about.
    pub fn produced(&self) -> bool {
        matches!(self.status, Status::Ok | Status::Fallback)
    }

    /// `amount` per second of the fastest run; `None` unless an object was
    /// produced.
    fn per_second(&self, amount: Option<u64>) -> Option<u64> {
        let wall = self.wall_us_min.filter(|&us| us > 0 && self.produced())?;
        u64::try_from(u128::from(amount?) * 1_000_000 / u128::from(wall)).ok()
    }

    /// Bytes of IR per second of the fastest run.
    pub fn ir_bytes_per_s(&self) -> Option<u64> {
        self.per_second(self.ir_bytes)
    }

    /// Functions per second of the fastest run.
    pub fn functions_per_s(&self) -> Option<u64> {
        self.per_second(self.functions)
    }

    /// How far the middle run is above the fastest, in hundredths of a
    /// percent of the fastest: what one run on this machine is worth.
    pub fn spread_bp(&self) -> Option<u64> {
        let (min, median) = (self.wall_us_min?, self.wall_us_median?);
        (min > 0).then(|| median.saturating_sub(min) * 10_000 / min)
    }

    /// Sets the ratios from `llc`'s record of the same input and run. Only
    /// a record that produced an object is compared.
    pub fn set_ratios(&mut self, llc: &Record) {
        if !(self.produced() && llc.status == Status::Ok) {
            return;
        }
        let ratio = |own: Option<u64>, reference: Option<u64>| Ratio::of(own?, reference?);
        self.wall_ratio = ratio(self.wall_us_min, llc.wall_us_min);
        self.cpu_ratio = ratio(self.cpu_us, llc.cpu_us);
        self.rss_ratio = ratio(self.rss_kib, llc.rss_kib);
        self.size_ratio = ratio(self.object_bytes, llc.object_bytes);
    }

    /// The record as a line of the file, without the newline.
    pub fn to_line(&self) -> String {
        fn number<T: ToString>(value: Option<T>) -> String {
            value.map_or_else(|| "-".to_owned(), |v| v.to_string())
        }
        let phases: Vec<String> =
            self.phases.iter().map(|(name, us)| format!("{name}={us}")).collect();
        [
            self.status.name().to_owned(),
            escape(&self.input),
            self.class.name().to_owned(),
            if self.debug { "g" } else { "-" }.to_owned(),
            self.tool.name().to_owned(),
            number(self.ir_bytes),
            number(self.functions),
            escape(self.ir_hash.as_deref().unwrap_or("")),
            self.runs.to_string(),
            number(self.wall_us_min),
            number(self.wall_us_median),
            number(self.wall_us_max),
            number(self.cpu_us),
            number(self.rss_kib),
            number(self.rss_kib_min),
            number(self.rss_kib_max),
            number(self.object_bytes),
            number(self.wall_ratio),
            number(self.cpu_ratio),
            number(self.rss_ratio),
            number(self.size_ratio),
            number(self.ir_bytes_per_s()),
            number(self.functions_per_s()),
            escape(&phases.join(",")),
            escape(&self.detail),
            number(self.instructions),
        ]
        .join("\t")
    }

    /// Reads a line written by [`Record::to_line`].
    pub fn parse(line: &str) -> Result<Self, String> {
        Self::parse_fields(line, FIELDS.len())
    }

    /// Reads a line of `count` fields: [`FIELDS`], or the first
    /// [`FIELDS_BEFORE_INSTRUCTIONS`] of them (a file of before WP-153).
    fn parse_fields(line: &str, count: usize) -> Result<Self, String> {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != count {
            return Err(format!("{} fields, not {count}", fields.len()));
        }
        let bad = |index: usize| format!("invalid {} '{}'", FIELDS[index], fields[index]);
        let number = |index: usize| match fields[index] {
            "-" => Ok(None),
            text if all_digits(text) => text.parse::<u64>().map(Some).map_err(|_| bad(index)),
            _ => Err(bad(index)),
        };
        let ratio = |index: usize| match fields[index] {
            "-" => Ok(None),
            text => Ratio::parse(text).map(Some).ok_or_else(|| bad(index)),
        };
        let mut phases = Vec::new();
        for item in unescape(fields[23]).split(',').filter(|item| !item.is_empty()) {
            let parsed = item
                .split_once('=')
                .filter(|(name, us)| valid_phase(name) && all_digits(us))
                .and_then(|(name, us)| Some((name.to_owned(), us.parse().ok()?)));
            phases.push(parsed.ok_or_else(|| bad(23))?);
        }
        let record = Self {
            status: Status::parse(fields[0]).ok_or_else(|| bad(0))?,
            input: unescape(fields[1]),
            class: Class::parse(fields[2]).ok_or_else(|| bad(2))?,
            debug: match fields[3] {
                "g" => true,
                "-" => false,
                _ => return Err(bad(3)),
            },
            tool: Tool::parse(fields[4]).ok_or_else(|| bad(4))?,
            ir_bytes: number(5)?,
            functions: number(6)?,
            ir_hash: Some(unescape(fields[7])).filter(|hash| !hash.is_empty()),
            runs: number(8)?.and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(8))?,
            wall_us_min: number(9)?,
            wall_us_median: number(10)?,
            wall_us_max: number(11)?,
            cpu_us: number(12)?,
            rss_kib: number(13)?,
            rss_kib_min: number(14)?,
            rss_kib_max: number(15)?,
            object_bytes: number(16)?,
            wall_ratio: ratio(17)?,
            cpu_ratio: ratio(18)?,
            rss_ratio: ratio(19)?,
            size_ratio: ratio(20)?,
            phases,
            detail: unescape(fields[24]),
            instructions: if count > FIELDS_BEFORE_INSTRUCTIONS { number(25)? } else { None },
        };
        // The two throughput fields are derived; they must be numbers.
        number(21)?;
        number(22)?;
        Ok(record)
    }
}

/// Whether `name` can be a phase's name: lower-case letters, digits, `-`.
pub fn valid_phase(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A free-form field: `\\`, `\t`, `\n`, `\r` for those characters and `-`
/// for an empty value, as in hod's coverage log (`spec/cli.md`).
fn escape(value: &str) -> String {
    if value.is_empty() {
        return "-".to_owned();
    }
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    // A value that is exactly `-` would read back as empty.
    if out == "-" { "\\-".to_owned() } else { out }
}

fn unescape(field: &str) -> String {
    if field == "-" {
        return String::new();
    }
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// A results file or a baseline: what was measured, where and with what.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Results {
    /// The header after its first line, in order: `(key, value)`.
    pub header: Vec<(String, String)>,
    pub records: Vec<Record>,
}

impl Results {
    /// A header value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.header.iter().find(|(k, _)| k == key).map(|(_, value)| value.as_str())
    }

    /// Sets a header value, keeping its place if the key exists.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.header.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.header.push((key.to_owned(), value)),
        }
    }

    /// The record of an input and a tool.
    pub fn find(&self, input: &str, tool: Tool) -> Option<&Record> {
        self.records.iter().find(|r| r.input == input && r.tool == tool)
    }

    /// The file's text.
    pub fn to_text(&self) -> String {
        let mut text = format!("# {FORMAT}\n");
        for (key, value) in &self.header {
            let _ = writeln!(text, "# {key}\t{}", escape(value));
        }
        let _ = writeln!(text, "# fields\t{}", FIELDS.join("\t"));
        for record in &self.records {
            text.push_str(&record.to_line());
            text.push('\n');
        }
        text
    }

    /// Reads a file's text. Errors name the line.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().enumerate();
        match lines.next() {
            Some((_, first)) if first == format!("# {FORMAT}") => {}
            _ => {
                return Err(format!(
                    "line 1: not a benchmark results file (expected '# {FORMAT}')"
                ));
            }
        }
        let mut results = Self::default();
        let mut count = FIELDS.len();
        for (index, line) in lines {
            let at = |message: String| format!("line {}: {message}", index + 1);
            if let Some(header) = line.strip_prefix("# ") {
                let (key, value) = header
                    .split_once('\t')
                    .ok_or_else(|| at("a header line without a value".to_owned()))?;
                if key == "fields" {
                    count = if value == FIELDS.join("\t") {
                        FIELDS.len()
                    } else if value == FIELDS[..FIELDS_BEFORE_INSTRUCTIONS].join("\t") {
                        FIELDS_BEFORE_INSTRUCTIONS
                    } else {
                        return Err(at("the fields are not this version's".to_owned()));
                    };
                } else if !results.records.is_empty() {
                    return Err(at("a header line after the records".to_owned()));
                } else if results.get(key).is_some() {
                    return Err(at(format!("a second '{key}'")));
                } else {
                    results.header.push((key.to_owned(), unescape(value)));
                }
            } else {
                results.records.push(Record::parse_fields(line, count).map_err(at)?);
            }
        }
        Ok(results)
    }

    /// Reads a file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("'{}': {e}", path.display()))
    }

    /// Writes the file, replacing it: to a temporary name beside it, then
    /// renamed, so a reader never sees half a file.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        write_replacing(path, &self.to_text())
    }
}

/// Writes `text` at `path` through a temporary file beside it.
pub fn write_replacing(path: &Path, text: &str) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("cannot write '{}': {e}", path.display());
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".tmp-{}", std::process::id()));
    std::fs::write(&temporary, text).map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(failed)
}

/// `fnv1a64:<16 hex digits>` of `bytes`: the identity of an input.
pub fn fnv1a64(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

/// The machine a run was made on: what makes two runs' absolute numbers
/// comparable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    /// `<arch>-<cpu>-<cores>c`, in lower-case letters, digits and `-`.
    pub id: String,
    /// The CPU as the system names it.
    pub cpu: String,
    pub cores: usize,
    pub arch: String,
}

impl Machine {
    /// This machine, from `/proc/cpuinfo`.
    pub fn detect() -> Self {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self::from_cpuinfo(&cpuinfo, std::env::consts::ARCH, cores)
    }

    /// The machine of a `/proc/cpuinfo` text. The CPU is the first `model
    /// name` (x86), else the first `CPU implementer` and `CPU part` (Arm),
    /// else `unknown`.
    pub fn from_cpuinfo(cpuinfo: &str, arch: &str, cores: usize) -> Self {
        let value = |key: &str| {
            cpuinfo.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name.trim() == key && !value.trim().is_empty()).then(|| value.trim().to_owned())
            })
        };
        let cpu = value("model name")
            .or_else(|| {
                let implementer = value("CPU implementer")?;
                Some(match value("CPU part") {
                    Some(part) => format!("implementer {implementer} part {part}"),
                    None => format!("implementer {implementer}"),
                })
            })
            .unwrap_or_else(|| "unknown".to_owned());
        let id = sanitize(&format!("{arch}-{cpu}-{cores}c"));
        Self { id, cpu, cores, arch: arch.to_owned() }
    }
}

/// Lower-case letters and digits kept, every other run of characters one
/// `-`, none at the ends.
fn sanitize(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Record {
        Record {
            ir_bytes: Some(5_983_380),
            functions: Some(3308),
            ir_hash: Some("fnv1a64:0123456789abcdef".to_owned()),
            runs: 3,
            wall_us_min: Some(650_000),
            wall_us_median: Some(700_000),
            wall_us_max: Some(910_000),
            cpu_us: Some(640_000),
            rss_kib: Some(100_324),
            rss_kib_min: Some(100_200),
            rss_kib_max: Some(100_400),
            object_bytes: Some(2_167_368),
            ..Record::blank(Status::Ok, "stl-heavy", Class::Large, false, Tool::Llc)
        }
    }

    // spec/harness.md "bench", "Ratios and throughput": a ratio is the
    // tool's figure over llc's, to four decimals, rounded to nearest.
    #[test]
    fn ratio_computation() {
        assert_eq!(Ratio::of(1, 3).unwrap().to_string(), "0.3333");
        assert_eq!(Ratio::of(2, 3).unwrap().to_string(), "0.6667");
        assert_eq!(Ratio::of(650, 650).unwrap().to_string(), "1.0000");
        assert_eq!(Ratio::of(1_234_567, 100).unwrap().to_string(), "12345.6700");
        assert_eq!(Ratio::of(0, 5).unwrap().to_string(), "0.0000");
        assert_eq!(Ratio::of(5, 0), None);
        assert_eq!(Ratio::parse("1.0500"), Some(Ratio(10_500)));
        for bad in ["1", "1.05", "1.05000", "-1.0000", ".5000", "1.0x00"] {
            assert_eq!(Ratio::parse(bad), None, "{bad}");
        }

        let llc = sample();
        let mut hod = Record {
            tool: Tool::Hod,
            wall_us_min: Some(65_000),
            cpu_us: Some(320_000),
            rss_kib: Some(25_081),
            object_bytes: Some(2_167_368 * 2),
            ..sample()
        };
        hod.set_ratios(&llc);
        let shown = |r: Option<Ratio>| r.unwrap().to_string();
        assert_eq!(shown(hod.wall_ratio), "0.1000");
        assert_eq!(shown(hod.cpu_ratio), "0.5000");
        assert_eq!(shown(hod.rss_ratio), "0.2500");
        assert_eq!(shown(hod.size_ratio), "2.0000");

        // A figure either side lacks gives no ratio, and a decline is not
        // a time to compare.
        let mut partial = Record { cpu_us: None, ..hod.clone() };
        partial.cpu_ratio = None;
        partial.set_ratios(&llc);
        assert_eq!(partial.cpu_ratio, None);
        let mut declined = Record { status: Status::Declined, wall_ratio: None, ..sample() };
        declined.tool = Tool::Hod;
        declined.set_ratios(&llc);
        assert_eq!((declined.wall_ratio, declined.rss_ratio), (None, None));
        let mut against_error = Record { wall_ratio: None, ..sample() };
        against_error.set_ratios(&Record { status: Status::Error, ..sample() });
        assert_eq!(against_error.wall_ratio, None);
    }

    // "Ratios and throughput": bytes of IR and functions per second of
    // the fastest run.
    #[test]
    fn throughput() {
        let record = sample();
        assert_eq!(record.ir_bytes_per_s(), Some(5_983_380 * 1_000_000 / 650_000));
        assert_eq!(record.functions_per_s(), Some(5089));
        assert_eq!(record.spread_bp(), Some(769));
        let declined = Record { status: Status::Declined, ..sample() };
        assert_eq!((declined.ir_bytes_per_s(), declined.functions_per_s()), (None, None));
        let error = Record::blank(Status::Error, "x", Class::Small, false, Tool::Llc);
        assert_eq!((error.ir_bytes_per_s(), error.spread_bp()), (None, None));
    }

    // "Results file": a record is 26 fields, and reads back as written.
    #[test]
    fn record_lines_round_trip() {
        let llc = sample();
        let line = llc.to_line();
        assert_eq!(
            line,
            "ok\tstl-heavy\tlarge\t-\tllc\t5983380\t3308\tfnv1a64:0123456789abcdef\t3\t\
             650000\t700000\t910000\t640000\t100324\t100200\t100400\t2167368\t-\t-\t-\t-\t\
             9205200\t5089\t-\t-\t-"
        );
        assert_eq!(line.split('\t').count(), FIELDS.len());
        assert_eq!(Record::parse(&line).unwrap(), llc);

        let mut hod = Record {
            status: Status::Fallback,
            debug: true,
            tool: Tool::HodFallback,
            phases: vec![
                ("read".to_owned(), 410),
                ("decide".to_owned(), 3),
                ("total".to_owned(), 651_000),
            ],
            detail: "codegen-not-implemented".to_owned(),
            ..sample()
        };
        hod.set_ratios(&llc);
        let line = hod.to_line();
        assert!(line.starts_with("fallback\tstl-heavy\tlarge\tg\thod+fallback\t"), "{line}");
        assert!(line.ends_with("\t1.0000\t1.0000\t1.0000\t1.0000\t9205200\t5089\tread=410,decide=3,total=651000\tcodegen-not-implemented\t-"), "{line}");
        assert_eq!(Record::parse(&line).unwrap(), hod);

        // An error has no numbers, and its text is escaped.
        let error = Record {
            detail: "llc exited with status 1:\tbad\nmodule \\ -".to_owned(),
            ..Record::blank(Status::Error, "small-c", Class::Small, false, Tool::Llc)
        };
        let line = error.to_line();
        assert_eq!(
            line,
            "error\tsmall-c\tsmall\t-\tllc\t-\t-\t-\t0\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-\t-\t\
             llc exited with status 1:\\tbad\\nmodule \\\\ -\t-"
        );
        assert_eq!(Record::parse(&line).unwrap(), error);
        let dash = Record { detail: "-".to_owned(), ..error };
        assert_eq!(Record::parse(&dash.to_line()).unwrap(), dash);

        // WP-153: the instructions of a counted run, last.
        let counted = Record { instructions: Some(565_048_220), ..sample() };
        let line = counted.to_line();
        assert!(line.ends_with("\t-\t-\t565048220"), "{line}");
        assert_eq!(Record::parse(&line).unwrap(), counted);
    }

    // WP-153: a file of before `instructions` (25 fields) is read, its
    // records without instructions; it is written with 26.
    #[test]
    fn files_without_instructions_are_read() {
        let mut results = Results::default();
        results.set("machine", "m");
        results.records.push(sample());
        let new = results.to_text();
        let old: String = new
            .lines()
            .map(|line| match line.strip_suffix("\tinstructions") {
                Some(fields) => fields.to_owned(),
                None if line.starts_with('#') => line.to_owned(),
                None => line.rsplit_once('\t').unwrap().0.to_owned(),
            })
            .map(|line| line + "\n")
            .collect();
        assert_ne!(old, new);
        assert_eq!(Results::parse(&old).unwrap(), results);
        assert_eq!(Results::parse(&old).unwrap().to_text(), new);
        // A 25-field record under the 26-field line is refused.
        let mixed = new.replace(&sample().to_line(), old.lines().last().unwrap());
        assert_eq!(Results::parse(&mixed).unwrap_err(), "line 4: 25 fields, not 26");
    }

    // "Results file": what is not a record is refused, with the field.
    #[test]
    fn malformed_lines() {
        let good = sample().to_line();
        let with = |index: usize, value: &str| {
            let mut fields: Vec<&str> = good.split('\t').collect();
            fields[index] = value;
            Record::parse(&fields.join("\t")).unwrap_err()
        };
        assert_eq!(Record::parse("ok\tx").unwrap_err(), "2 fields, not 26");
        assert_eq!(with(0, "fine"), "invalid status 'fine'");
        assert_eq!(with(2, "huge"), "invalid class 'huge'");
        assert_eq!(with(3, "yes"), "invalid debug 'yes'");
        assert_eq!(with(4, "gcc"), "invalid tool 'gcc'");
        assert_eq!(with(9, "1.5"), "invalid wall_us_min '1.5'");
        assert_eq!(with(8, "-"), "invalid runs '-'");
        assert_eq!(with(17, "1.5"), "invalid wall_ratio '1.5'");
        assert_eq!(with(21, "fast"), "invalid ir_bytes_per_s 'fast'");
        assert_eq!(with(23, "read"), "invalid phases 'read'");
        assert_eq!(with(23, "Read=1"), "invalid phases 'Read=1'");
    }

    // "Results file": the header, the field names, the records; the file
    // read and written again is the same bytes.
    #[test]
    fn results_file_round_trip() {
        let mut results = Results::default();
        results.set("machine", "aarch64-implementer-0x61-part-0x000-24c");
        results.set("llvm", "22.1.8");
        results.set("producer", "clang version 22.1.8\twith a tab");
        results.set("llvm", "22.1.9");
        results.records.push(sample());
        results.records.push(Record::blank(Status::Error, "x", Class::Small, true, Tool::Generate));
        let text = results.to_text();
        assert!(text.starts_with("# hod-bench 1\n# machine\taarch64-implementer-0x61-part-0x000-24c\n# llvm\t22.1.9\n"), "{text}");
        assert!(text.contains("\n# fields\tstatus\tinput\tclass\t"), "{text}");
        let read = Results::parse(&text).unwrap();
        assert_eq!(read, results);
        assert_eq!(read.to_text(), text);
        assert_eq!(read.get("producer"), Some("clang version 22.1.8\twith a tab"));
        assert_eq!(read.get("absent"), None);
        assert_eq!(read.find("stl-heavy", Tool::Llc), Some(&results.records[0]));
        assert_eq!(read.find("stl-heavy", Tool::Hod), None);

        for (bad, message) in [
            ("", "line 1: not a benchmark results file (expected '# hod-bench 1')"),
            ("# hod-bench 2\n", "line 1: not a benchmark results file (expected '# hod-bench 1')"),
            ("# hod-bench 1\n# machine\n", "line 2: a header line without a value"),
            ("# hod-bench 1\n# a\t1\n# a\t2\n", "line 3: a second 'a'"),
            ("# hod-bench 1\n# fields\tstatus\n", "line 2: the fields are not this version's"),
            ("# hod-bench 1\nok\n", "line 2: 1 fields, not 26"),
        ] {
            assert_eq!(Results::parse(bad).unwrap_err(), message);
        }
        let late = format!("{text}# late\t1\n");
        assert!(Results::parse(&late).unwrap_err().ends_with("a header line after the records"));
    }

    // "Results file": written through a temporary file, replacing.
    #[test]
    fn results_are_written_and_read() {
        let scratch = hod_testutil::Scratch::new("bench-record");
        let path = scratch.join("deep/results.tsv");
        let mut results = Results::default();
        results.set("runs", "3");
        results.records.push(sample());
        results.write(&path).unwrap();
        results.records.clear();
        results.write(&path).unwrap();
        assert_eq!(Results::read(&path).unwrap(), results);
        let names: Vec<_> = std::fs::read_dir(scratch.join("deep"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["results.tsv"]);
        assert!(
            Results::read(&scratch.join("absent.tsv")).unwrap_err().starts_with("cannot read '")
        );
    }

    // "Inputs": the identity of an input is FNV-1a, 64 bits, of its bytes.
    #[test]
    fn input_hash() {
        assert_eq!(fnv1a64(b""), "fnv1a64:cbf29ce484222325");
        assert_eq!(fnv1a64(b"a"), "fnv1a64:af63dc4c8601ec8c");
        assert_eq!(fnv1a64(b"foobar"), "fnv1a64:85944171f73967e8");
    }

    // "Machine": the id is made of the architecture, the CPU model and
    // the core count, and of nothing that names a host or a user.
    #[test]
    fn machine_id() {
        let x86 = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Ryzen 9 5950X 16-Core Processor\n\
                   processor\t: 1\nmodel name\t: other\n";
        let machine = Machine::from_cpuinfo(x86, "x86_64", 32);
        assert_eq!(machine.cpu, "AMD Ryzen 9 5950X 16-Core Processor");
        assert_eq!(machine.id, "x86-64-amd-ryzen-9-5950x-16-core-processor-32c");
        let arm = "processor\t: 0\nBogoMIPS\t: 48.00\nCPU implementer\t: 0x61\nCPU architecture: 8\n\
                   CPU variant\t: 0x0\nCPU part\t: 0x000\n";
        let machine = Machine::from_cpuinfo(arm, "aarch64", 24);
        assert_eq!(machine.cpu, "implementer 0x61 part 0x000");
        assert_eq!(machine.id, "aarch64-implementer-0x61-part-0x000-24c");
        assert_eq!((machine.cores, machine.arch.as_str()), (24, "aarch64"));
        assert_eq!(Machine::from_cpuinfo("", "riscv64", 4).id, "riscv64-unknown-4c");
        assert_eq!(
            Machine::from_cpuinfo("model name : (R) @ 2.4GHz!", "x86_64", 1).id,
            "x86-64-r-2-4ghz-1c"
        );
        let here = Machine::detect();
        assert!(here.cores >= 1 && here.id.ends_with(&format!("-{}c", here.cores)), "{here:?}");
    }
}
