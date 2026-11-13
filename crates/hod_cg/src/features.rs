//! Target-feature parsing (`spec/codegen.md` §7.3, "Target features",
//! WP-117): the reading of a `target-features` string or of `-mattr`
//! against a target's [`FeatureTable`]. The tables are the targets'
//! (AArch64: `hod_a64::features`, §5.5.2).

/// A feature hod has not judged, or an item that is not `+name` or
/// `-name`.
pub const UNKNOWN: &str = "attr:target-feature-unknown";

/// A refused feature: the reason, and the item as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused<'a> {
    /// The stable name: [`UNKNOWN`] or one of the target's reasons.
    pub reason: &'static str,
    /// The item that was refused, sign included.
    pub feature: &'a str,
}

/// What one target's code is correct under.
#[derive(Clone, Copy, Debug)]
pub struct FeatureTable {
    /// Features hod's code needs: `-name` is refused with the reason
    /// (`+name` is in [`FeatureTable::available`]).
    pub required: &'static [(&'static str, &'static str)],
    /// Features that only make instructions available, accepted either
    /// way.
    pub available: &'static [&'static str],
    /// The reason of a feature that changes the contract of the code, if
    /// the name is one: `+name` is refused with it, `-name` (the
    /// default) accepted.
    pub contract: fn(&str) -> Option<&'static str>,
}

impl FeatureTable {
    /// Checks one feature string: items separated by `,`, each `+name` or
    /// `-name`; empty items are skipped (`-mattr` drops them, and LLVM's
    /// parser does).
    ///
    /// # Errors
    /// The first item hod's code is not correct under, with its reason.
    pub fn check<'a>(&self, features: &'a str) -> Result<(), Refused<'a>> {
        features
            .split(',')
            .filter(|item| !item.is_empty())
            .try_for_each(|item| self.check_one(item))
    }

    /// One item of a feature string: a required feature turned off is
    /// refused, an available one accepted, a contract feature refused
    /// when on and accepted when off, anything else (or an item without a
    /// sign) [`UNKNOWN`].
    ///
    /// # Errors
    /// As [`FeatureTable::check`].
    pub fn check_one<'a>(&self, item: &'a str) -> Result<(), Refused<'a>> {
        let refuse = |reason| Err(Refused { reason, feature: item });
        let (on, name) = match item.split_at_checked(1) {
            Some(("+", name)) => (true, name),
            Some(("-", name)) => (false, name),
            _ => return refuse(UNKNOWN),
        };
        if !on && let Some(&(_, reason)) = self.required.iter().find(|&&(n, _)| n == name) {
            return refuse(reason);
        }
        if self.available.contains(&name) {
            return Ok(());
        }
        match ((self.contract)(name), on) {
            // The contract features are off by default: turning one off
            // is what hod's code already keeps.
            (Some(_), false) => Ok(()),
            (Some(reason), true) => refuse(reason),
            (None, _) => refuse(UNKNOWN),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: FeatureTable = FeatureTable {
        required: &[("sse2", "test:no-sse2")],
        available: &["sse2", "avx"],
        contract: |name| name.starts_with("retpoline").then_some("test:retpoline"),
    };

    fn reason(features: &str) -> Option<&'static str> {
        TABLE.check(features).err().map(|refused| refused.reason)
    }

    /// §7.3 "Target features": the order of the rules.
    #[test]
    fn rules() {
        assert_eq!(reason(""), None);
        assert_eq!(reason(",+sse2,,-avx,+avx,-retpoline-x"), None);
        assert_eq!(reason("+avx,-sse2"), Some("test:no-sse2"));
        assert_eq!(reason("+retpoline-x"), Some("test:retpoline"));
        assert_eq!(reason("+bogus"), Some(UNKNOWN));
        assert_eq!(reason("-bogus"), Some(UNKNOWN));
        assert_eq!(reason("avx"), Some(UNKNOWN));
        assert_eq!(reason("+"), Some(UNKNOWN));
        assert_eq!(
            TABLE.check("+avx,+bogus,+retpoline-x"),
            Err(Refused { reason: UNKNOWN, feature: "+bogus" })
        );
    }
}
