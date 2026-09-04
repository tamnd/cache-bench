//! The cache servers under test.

use std::fmt;
use std::str::FromStr;

/// The seven cache servers this harness measures.
///
/// The order here is the order the original writes them in `bench-all.sh`.
/// It is not the order the charts use, which comes from sorted result filenames, so do not rely on this for anything a reader will see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CacheKind {
    /// memcached, driven over the memcache text protocol.
    Memcache,
    /// Dragonfly.
    Dragonfly,
    /// Valkey.
    Valkey,
    /// Redis.
    Redis,
    /// Microsoft Garnet.
    Garnet,
    /// Pogocache.
    Pogocache,
    /// yo, from tamnd/yo.
    Yo,
}

impl CacheKind {
    /// Every kind, in the order the original sweeps them.
    pub const ALL: [Self; 7] = [
        Self::Memcache,
        Self::Dragonfly,
        Self::Valkey,
        Self::Redis,
        Self::Garnet,
        Self::Pogocache,
        Self::Yo,
    ];

    /// The short name used in result filenames and on chart legends.
    ///
    /// These match the original's names, so memcached is `memcache`, and a results directory from either harness is readable by the other.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Memcache => "memcache",
            Self::Dragonfly => "dragonfly",
            Self::Valkey => "valkey",
            Self::Redis => "redis",
            Self::Garnet => "garnet",
            Self::Pogocache => "pogocache",
            Self::Yo => "yo",
        }
    }
}

impl fmt::Display for CacheKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A name in a filename that is not one of the seven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCache(pub String);

impl fmt::Display for UnknownCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown cache {:?}", self.0)
    }
}

impl std::error::Error for UnknownCache {}

impl FromStr for CacheKind {
    type Err = UnknownCache;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.name() == s)
            .ok_or_else(|| UnknownCache(s.to_owned()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::CacheKind;

    #[test]
    fn names_are_unique_and_stable() {
        let mut names: Vec<&str> = CacheKind::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "two kinds share a name");
    }

    #[test]
    fn names_round_trip() {
        for kind in CacheKind::ALL {
            assert_eq!(kind.name().parse::<CacheKind>().unwrap(), kind);
        }
    }

    #[test]
    fn memcached_is_called_memcache() {
        // The original's name, and changing it would silently make a results directory from one harness unreadable by the other.
        assert_eq!(CacheKind::Memcache.name(), "memcache");
    }
}
