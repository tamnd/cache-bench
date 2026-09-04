//! A set of CPUs, written the way `taskset -c` writes them.
//!
//! The original pins the cache to `0-15` and the load generator to `16-31` and those two strings are constants in a shell script.
//! Here they are a profile field, which means they can be wrong, which means they have to be checked.
//! The check that matters is that the two sets do not overlap, because a load generator sharing a core with the server under test is measuring the two of them fighting.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A set of CPU numbers, such as `0-15` or `0,2,4` or `0-3,8`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CpuSet(BTreeSet<u32>);

impl CpuSet {
    /// How many CPUs are in the set.
    ///
    /// This is the ceiling on any thread count pinned into it, and going above it is how a sweep ends up measuring oversubscription without meaning to.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set is empty, which no usable pin is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The highest CPU number in the set.
    #[must_use]
    pub fn highest(&self) -> Option<u32> {
        self.0.iter().next_back().copied()
    }

    /// Whether the two sets have a CPU in common.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.0.intersection(&other.0).next().is_some()
    }

    /// The CPUs, ascending.
    pub fn cpus(&self) -> impl Iterator<Item = u32> {
        self.0.iter().copied()
    }
}

/// Text that is not a CPU list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadCpuSet(pub String);

impl fmt::Display for BadCpuSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a CPU list, expected something like 0-15 or 0,2,4",
            self.0
        )
    }
}

impl std::error::Error for BadCpuSet {}

impl FromStr for CpuSet {
    type Err = BadCpuSet;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadCpuSet(s.to_owned());
        let mut cpus = BTreeSet::new();
        for part in s.split(',') {
            let part = part.trim();
            match part.split_once('-') {
                Some((lo, hi)) => {
                    let lo: u32 = lo.trim().parse().map_err(|_| bad())?;
                    let hi: u32 = hi.trim().parse().map_err(|_| bad())?;
                    if lo > hi {
                        return Err(bad());
                    }
                    cpus.extend(lo..=hi);
                }
                None => {
                    cpus.insert(part.parse().map_err(|_| bad())?);
                }
            }
        }
        if cpus.is_empty() {
            return Err(bad());
        }
        Ok(Self(cpus))
    }
}

impl fmt::Display for CpuSet {
    /// Written back as ranges, so a set read as `0,1,2,3` comes out as `0-3`.
    ///
    /// This is the form that goes into a log line and into `host.json`, and a list of sixteen numbers where a range would do is harder to check at a glance.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        let mut cpus = self.cpus().peekable();
        while let Some(lo) = cpus.next() {
            let mut hi = lo;
            while cpus.peek() == Some(&(hi + 1)) {
                hi = cpus.next().unwrap_or(hi);
            }
            if !first {
                f.write_str(",")?;
            }
            first = false;
            if lo == hi {
                write!(f, "{lo}")?;
            } else {
                write!(f, "{lo}-{hi}")?;
            }
        }
        Ok(())
    }
}

impl Serialize for CpuSet {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CpuSet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::CpuSet;

    #[test]
    fn the_two_pins_the_original_uses() {
        let cache: CpuSet = "0-15".parse().unwrap();
        let bench: CpuSet = "16-31".parse().unwrap();
        assert_eq!(cache.len(), 16);
        assert_eq!(bench.len(), 16);
        assert_eq!(cache.highest(), Some(15));
        assert!(!cache.overlaps(&bench));
    }

    #[test]
    fn a_shared_core_is_visible() {
        let a: CpuSet = "0-3".parse().unwrap();
        let b: CpuSet = "3-7".parse().unwrap();
        assert!(a.overlaps(&b));
    }

    #[test]
    fn lists_and_ranges_and_both() {
        assert_eq!("0,2,4".parse::<CpuSet>().unwrap().len(), 3);
        assert_eq!("0-3,8".parse::<CpuSet>().unwrap().len(), 5);
        assert_eq!("7".parse::<CpuSet>().unwrap().len(), 1);
    }

    // A set written any way comes back as ranges, since that is the form that goes into a log line and into host.json.
    #[test]
    fn written_back_as_ranges() {
        for (text, want) in [
            ("0-15", "0-15"),
            ("0,1,2,3", "0-3"),
            ("0-3,8", "0-3,8"),
            ("4,2,0", "0,2,4"),
            ("3", "3"),
        ] {
            assert_eq!(text.parse::<CpuSet>().unwrap().to_string(), want);
        }
    }

    #[test]
    fn rubbish_is_refused() {
        for text in ["", "-", "0-", "a-b", "15-0", "0..3", "0 15"] {
            assert!(text.parse::<CpuSet>().is_err(), "{text} was accepted");
        }
    }
}
