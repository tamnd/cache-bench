//! A memory size, which the profiles write as text and four of the seven servers want spelled differently.
//!
//! `profiles.toml` says `maxmemory = "32gb"`. Redis, Valkey, Dragonfly and Pogocache take exactly that.
//! Garnet wants `32g`. memcached wants `32768`, in megabytes, with no unit at all.
//! Carrying the profile's string around and reformatting it at each call site is how one of those ends up an order of magnitude out, so it is parsed once into a byte count here and spelled on demand.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;

/// A number of bytes.
///
/// Units are binary throughout, so `1kb` is 1024 and `32gb` is 34359738368.
/// Redis draws a distinction between `1k` and `1kb` and means 1000 by the first, and this does not.
/// The original only ever writes `32gb` for Redis and `32g` for Garnet and means the same 32 GiB by both, so treating the two spellings as one quantity is what it already assumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Bytes(pub u64);

impl Bytes {
    /// The count, in bytes.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.0
    }

    /// The count in whole mebibytes, rounded down.
    ///
    /// This is what memcached's `-m` takes.
    #[must_use]
    pub const fn mib(self) -> u64 {
        self.0 / MIB
    }

    /// The count in whole gibibytes, rounded down.
    #[must_use]
    pub const fn gib(self) -> u64 {
        self.0 / GIB
    }

    /// The single letter spelling, such as `32g`, which is the one Garnet takes.
    #[must_use]
    pub fn short(self) -> String {
        let (n, _, short) = self.split();
        format!("{n}{short}")
    }

    /// The largest unit that divides the count exactly, with both spellings of it.
    ///
    /// A size that is not a whole number of anything comes back as a bare byte count, which every server here accepts.
    const fn split(self) -> (u64, &'static str, &'static str) {
        let b = self.0;
        if b >= GIB && b.is_multiple_of(GIB) {
            (b / GIB, "gb", "g")
        } else if b >= MIB && b.is_multiple_of(MIB) {
            (b / MIB, "mb", "m")
        } else if b >= KIB && b.is_multiple_of(KIB) {
            (b / KIB, "kb", "k")
        } else {
            (b, "", "")
        }
    }
}

impl fmt::Display for Bytes {
    /// The two letter spelling, such as `32gb`, which is the one Redis, Valkey, Dragonfly and Pogocache take.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (n, long, _) = self.split();
        write!(f, "{n}{long}")
    }
}

/// Text that is not a memory size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadSize(pub String);

impl fmt::Display for BadSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a memory size, expected a whole number and an optional unit, such as 32gb or 8g or 32768mb",
            self.0
        )
    }
}

impl std::error::Error for BadSize {}

impl FromStr for Bytes {
    type Err = BadSize;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadSize(s.to_owned());
        let lower = s.trim().to_ascii_lowercase();
        // Two letter units first, or 32gb would be read as 32 bytes with some letters after it.
        let units = [
            ("gb", GIB),
            ("mb", MIB),
            ("kb", KIB),
            ("g", GIB),
            ("m", MIB),
            ("k", KIB),
            ("b", 1),
        ];
        let (digits, mult) = units
            .into_iter()
            .find_map(|(unit, mult)| lower.strip_suffix(unit).map(|d| (d, mult)))
            .unwrap_or((lower.as_str(), 1));
        let n: u64 = digits.trim_end().parse().map_err(|_| bad())?;
        n.checked_mul(mult).map(Self).ok_or_else(bad)
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::Bytes;

    #[test]
    fn the_spellings_a_profile_might_use() {
        assert_eq!("32gb".parse::<Bytes>().unwrap(), Bytes(34_359_738_368));
        assert_eq!("32GB".parse::<Bytes>().unwrap(), Bytes(34_359_738_368));
        assert_eq!("32g".parse::<Bytes>().unwrap(), Bytes(34_359_738_368));
        assert_eq!("32768mb".parse::<Bytes>().unwrap(), Bytes(34_359_738_368));
        assert_eq!("8gb".parse::<Bytes>().unwrap(), Bytes(8_589_934_592));
        assert_eq!("512mb".parse::<Bytes>().unwrap(), Bytes(536_870_912));
        assert_eq!("4096".parse::<Bytes>().unwrap(), Bytes(4096));
    }

    // The three spellings the servers want, from one parsed value.
    #[test]
    fn one_size_three_flags() {
        let m: Bytes = "32gb".parse().unwrap();
        assert_eq!(m.to_string(), "32gb");
        assert_eq!(m.short(), "32g");
        assert_eq!(m.mib(), 32768);
    }

    #[test]
    fn a_size_that_is_not_a_whole_unit_stays_a_byte_count() {
        let odd = Bytes(1_500_000_000);
        assert_eq!(odd.to_string(), "1500000000");
        assert_eq!(odd.short(), "1500000000");
    }

    #[test]
    fn rubbish_is_refused() {
        for text in ["", "gb", "1.5gb", "-1gb", "32 gigabytes", "lots"] {
            assert!(text.parse::<Bytes>().is_err(), "{text} was accepted");
        }
    }

    // The profile files hold these as strings and every result file records the profile, so both directions have to agree.
    #[test]
    fn round_trips_through_serde_as_text() {
        let m: Bytes = "8gb".parse().unwrap();
        let text = serde_json::to_string(&m).unwrap();
        assert_eq!(text, r#""8gb""#);
        assert_eq!(serde_json::from_str::<Bytes>(&text).unwrap(), m);
    }
}
