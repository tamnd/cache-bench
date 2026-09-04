//! Types, the on disk JSON model, config and hardware profiles.
//!
//! Nothing in here does I/O beyond serialising and deserialising. It builds and
//! tests on every platform, which matters because the parts of this project
//! that are hard to get right are the JSON bytes, the statistics and the chart
//! arithmetic, and none of those should need a benchmark host to work on.

/// The seven cache servers this harness measures.
///
/// The order here is the order the original writes them in `bench-all.sh`. It
/// is not the order the charts use, which comes from sorted result filenames,
/// so do not rely on this for anything a reader will see.
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
    /// These match the original's names, so memcached is `memcache`, and a
    /// results directory from either harness is readable by the other.
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

#[cfg(test)]
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
}
