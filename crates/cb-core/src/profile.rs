//! Hardware profiles, from `profiles.toml`.
//!
//! The original hardcodes a 32 core box: the core pinning, the thread sweep, the memory limit and the client count are constants in a shell script and in the Go source.
//! Here they are data, because none of the machines this port runs on is that box.
//!
//! A profile can be wrong in ways that produce numbers rather than errors, which is what the validation here is for.
//! A thread sweep that goes past the cores it is pinned to measures oversubscription. A load generator sharing cores with the server measures the two of them fighting. A key space too large for the memory limit measures eviction. All three produce a chart that looks fine.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::cache::CacheKind;
use crate::cpuset::CpuSet;
use crate::size::Bytes;

/// How much room the memory limit has to leave over the live data.
///
/// The profiles hold a key count and a value size, and the product of those is the data itself with no per key overhead, no allocator slack and no memtier process in it.
/// A factor of two is not a measurement of any engine's overhead. It is a wide enough margin that a profile passing this check will not evict, which is the only thing the check is for.
const HEADROOM: u64 = 2;

/// Whether a cell is measured with perf attached.
///
/// Written `yes` and `no` in the profile and in every result filename, which is the original's spelling and is why this is not a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PerfMode {
    /// No perf.
    No,
    /// perf attached, which needs a PMU the host may not have.
    Yes,
}

impl PerfMode {
    /// The name used in a result filename.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::Yes => "yes",
        }
    }
}

impl fmt::Display for PerfMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// memtier's value size range, such as `1-1024`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeRange {
    /// Smallest value.
    pub min: u64,
    /// Largest value.
    pub max: u64,
}

impl SizeRange {
    /// The mean value size, which is what a working set estimate is built on.
    ///
    /// memtier picks uniformly across the range, so the mean is the midpoint.
    #[must_use]
    pub const fn average(self) -> u64 {
        u64::midpoint(self.min, self.max)
    }
}

impl fmt::Display for SizeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.min, self.max)
    }
}

impl FromStr for SizeRange {
    type Err = BadProfile;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || BadProfile::SizeRange(s.to_owned());
        let (min, max) = s.split_once('-').ok_or_else(bad)?;
        let min: u64 = min.trim().parse().map_err(|_| bad())?;
        let max: u64 = max.trim().parse().map_err(|_| bad())?;
        if min == 0 || min > max {
            return Err(bad());
        }
        Ok(Self { min, max })
    }
}

impl Serialize for SizeRange {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SizeRange {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// One machine's shape, and the sweep that fits it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// What the machine is, in a sentence, for the chart stamp.
    pub description: String,
    /// How many CPUs the machine has.
    pub cores: u32,
    /// Which CPUs the cache server is pinned to.
    pub cache_pin: CpuSet,
    /// Which CPUs the load generator is pinned to.
    pub bench_pin: CpuSet,
    /// The I/O thread counts to sweep, which is the x axis of every chart.
    pub threads: Vec<u32>,
    /// memtier's thread count.
    pub bench_threads: u32,
    /// Connections per memtier thread.
    pub connections_per_thread: u32,
    /// Operations per connection, per op type.
    pub operations: u64,
    /// memtier's value size range.
    pub size_range: SizeRange,
    /// memtier's key space.
    ///
    /// Explicit here and left at memtier's default upstream, which is the difference between a benchmark that never evicts and one that does.
    pub key_maximum: u64,
    /// The memory limit given to every server.
    pub maxmemory: Bytes,
    /// The pipeline depths to sweep.
    pub pipelines: Vec<u32>,
    /// How many runs go into one cell.
    pub runs: u32,
    /// Whether this host measures cells with perf, without it, or both.
    pub perf: Vec<PerfMode>,
}

impl Profile {
    /// Total connections, which is what goes in the result file.
    #[must_use]
    pub const fn connections(&self) -> u32 {
        self.bench_threads * self.connections_per_thread
    }

    /// How many operations one pass performs in total.
    ///
    /// memtier's `-n` is per connection, and the number that goes in the result file, and that a completed run is checked against, is the total. Keeping the two apart in one place means nothing else has to remember which of them it is holding.
    #[must_use]
    pub fn total_operations(&self) -> u64 {
        self.operations
            .saturating_mul(u64::from(self.connections()))
    }

    /// How much data the sweep will have live at once.
    ///
    /// The key space times the mean value size, with nothing added for per key overhead or allocator slack, so it is a floor rather than an estimate.
    #[must_use]
    pub const fn working_set(&self) -> Bytes {
        Bytes(self.key_maximum.saturating_mul(self.size_range.average()))
    }

    /// How many runs this profile's sweep will produce.
    ///
    /// Worth knowing before starting, because the answer is usually five figures and the difference between one profile and another is days.
    #[must_use]
    pub fn total_runs(&self) -> u64 {
        let cells = self.threads.len() * self.pipelines.len() * self.perf.len();
        // Taken from the engine list rather than written as a number, because the two went out of step the first time an engine was added and the only thing that said so was a test asserting a different literal.
        cells as u64 * u64::from(self.runs) * CacheKind::ALL.len() as u64
    }

    /// Everything that would make this profile measure something other than what it says it measures.
    ///
    /// # Errors
    ///
    /// See [`BadProfile`]. Each one is a way to get numbers rather than a failure, which is why they are refused here rather than noticed later.
    pub fn check(&self) -> Result<(), BadProfile> {
        if self.threads.is_empty() || self.pipelines.is_empty() || self.perf.is_empty() {
            return Err(BadProfile::Empty);
        }
        if self.runs == 0 {
            return Err(BadProfile::Empty);
        }
        if self.cache_pin.overlaps(&self.bench_pin) {
            return Err(BadProfile::SharedCores);
        }
        for (what, pin) in [
            ("cache_pin", &self.cache_pin),
            ("bench_pin", &self.bench_pin),
        ] {
            if let Some(highest) = pin.highest()
                && highest >= self.cores
            {
                return Err(BadProfile::PinOffTheEnd {
                    what,
                    highest,
                    cores: self.cores,
                });
            }
        }
        if let Some(&most) = self.threads.iter().max()
            && most as usize > self.cache_pin.len()
        {
            return Err(BadProfile::TooManyThreads {
                threads: most,
                cores: self.cache_pin.len(),
            });
        }
        if self.bench_threads as usize > self.bench_pin.len() {
            return Err(BadProfile::TooManyBenchThreads {
                threads: self.bench_threads,
                cores: self.bench_pin.len(),
            });
        }
        let needed = self.working_set().bytes().saturating_mul(HEADROOM);
        if self.maxmemory.bytes() < needed {
            return Err(BadProfile::WillEvict {
                working_set: self.working_set(),
                maxmemory: self.maxmemory,
                needed: Bytes(needed),
            });
        }
        Ok(())
    }
}

/// Every profile the file holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profiles {
    /// By name, which is what `--profile` takes and what every result file records.
    pub profiles: BTreeMap<String, Profile>,
}

impl Profiles {
    /// Read and check a `profiles.toml`.
    ///
    /// Every profile in the file is checked, not just the one about to be used, because a profile that cannot work is worth knowing about before the day somebody selects it.
    ///
    /// # Errors
    ///
    /// If the file is not TOML of the right shape, or if any profile in it would measure something other than what it says it measures.
    pub fn parse(text: &str) -> Result<Self, BadProfile> {
        let parsed: Self = toml::from_str(text).map_err(|e| BadProfile::Shape(e.to_string()))?;
        for (name, profile) in &parsed.profiles {
            profile.check().map_err(|why| BadProfile::Named {
                name: name.clone(),
                why: Box::new(why),
            })?;
        }
        Ok(parsed)
    }

    /// One profile by name.
    ///
    /// # Errors
    ///
    /// If the file does not hold a profile with that name.
    pub fn get(&self, name: &str) -> Result<&Profile, BadProfile> {
        self.profiles
            .get(name)
            .ok_or_else(|| BadProfile::NoSuchProfile {
                name: name.to_owned(),
                known: self.profiles.keys().cloned().collect::<Vec<_>>().join(", "),
            })
    }
}

/// Anything that stops a profile being usable.
#[derive(Debug, thiserror::Error)]
pub enum BadProfile {
    /// The file is not TOML of the right shape.
    #[error("profiles are not readable: {0}")]
    Shape(String),
    /// One named profile is unusable.
    #[error("profile {name} is not usable: {why}")]
    Named {
        /// Which one.
        name: String,
        /// What is wrong with it.
        why: Box<BadProfile>,
    },
    /// A profile with nothing to sweep.
    #[error(
        "a profile has to sweep at least one thread count, one pipeline depth and one perf mode, and do at least one run of each"
    )]
    Empty,
    /// The value size range is not a range.
    #[error("{0:?} is not a value size range, expected something like 1-1024")]
    SizeRange(String),
    /// The cache and the load generator share a core.
    #[error(
        "cache_pin and bench_pin share a core, so the load generator and the server under test would be competing for it and the number measured would be the two of them fighting"
    )]
    SharedCores,
    /// A pin names a CPU the machine does not have.
    #[error("{what} goes up to CPU {highest} on a machine with {cores} cores")]
    PinOffTheEnd {
        /// Which pin.
        what: &'static str,
        /// The highest CPU it names.
        highest: u32,
        /// How many the machine has.
        cores: u32,
    },
    /// The thread sweep goes past the cores the server is pinned to.
    #[error(
        "the sweep goes up to {threads} threads on {cores} pinned cores, which measures oversubscription rather than the engine"
    )]
    TooManyThreads {
        /// The highest thread count in the sweep.
        threads: u32,
        /// How many cores the cache is pinned to.
        cores: usize,
    },
    /// The load generator has more threads than pinned cores.
    #[error(
        "the load generator asks for {threads} threads on {cores} pinned cores, so it would be the bottleneck rather than the server"
    )]
    TooManyBenchThreads {
        /// memtier's thread count.
        threads: u32,
        /// How many cores it is pinned to.
        cores: usize,
    },
    /// The key space does not fit the memory limit.
    #[error(
        "the key space is about {working_set} live and maxmemory is {maxmemory}, so the sweep would evict and the benchmark would be measuring eviction policy rather than throughput, which needs at least {needed}"
    )]
    WillEvict {
        /// What the sweep will hold.
        working_set: Bytes,
        /// What the servers are allowed.
        maxmemory: Bytes,
        /// The limit that would pass.
        needed: Bytes,
    },
    /// No profile by that name.
    #[error("no profile called {name:?}, the file has {known}")]
    NoSuchProfile {
        /// What was asked for.
        name: String,
        /// What is there.
        known: String,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{PerfMode, Profile, Profiles};
    use crate::size::Bytes;

    const OURS: &str = include_str!("../../../profiles.toml");

    fn ours() -> Profiles {
        Profiles::parse(OURS).unwrap()
    }

    // The shipped file has to be usable, and this is the test that says so rather than a comment in the file promising it.
    #[test]
    fn every_profile_we_ship_is_usable() {
        let profiles = ours();
        assert!(profiles.profiles.len() >= 3);
        for (name, profile) in &profiles.profiles {
            assert!(profile.check().is_ok(), "{name}");
        }
    }

    // The reference profile is the original's box, and every one of these numbers is read off its driver script.
    // If one of them drifts, our charts stop being comparable with the published ones and nothing else would say so.
    #[test]
    fn the_reference_profile_is_the_originals_box() {
        let p = ours().get("reference").unwrap().clone();
        assert_eq!(p.cores, 32);
        assert_eq!(p.cache_pin.to_string(), "0-15");
        assert_eq!(p.bench_pin.to_string(), "16-31");
        assert_eq!(p.threads, [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16]);
        assert_eq!(p.bench_threads, 16);
        assert_eq!(p.connections(), 256);
        assert_eq!(p.operations, 100_000);
        assert_eq!(p.size_range.to_string(), "1-1024");
        assert_eq!(p.maxmemory, Bytes(32 * 1024 * 1024 * 1024));
        assert_eq!(p.pipelines, [1, 10, 25, 50]);
        assert_eq!(p.runs, 31);
        assert_eq!(p.perf, [PerfMode::No, PerfMode::Yes]);
    }

    // A host with no PMU cannot produce a cycles chart, so running the perf half of its matrix produces files whose only effect is to be filtered back out.
    #[test]
    fn the_host_with_no_pmu_does_not_sweep_perf() {
        assert_eq!(ours().get("wsl32").unwrap().perf, [PerfMode::No]);
    }

    // Two profiles, one twice the size of the other in every dimension, and the difference in wall clock is what makes it worth printing before starting.
    // These numbers went up by a seventh when rugo became the eighth engine, which is a day of extra wall clock on the reference profile and is what an engine costs.
    #[test]
    fn a_sweep_is_five_figures_of_runs() {
        assert_eq!(ours().get("reference").unwrap().total_runs(), 23808);
        assert_eq!(ours().get("wsl32").unwrap().total_runs(), 11904);
    }

    #[test]
    fn a_name_that_is_not_there_says_what_is() {
        let err = ours().get("laptop").unwrap_err().to_string();
        assert!(err.contains("laptop"), "{err}");
        assert!(err.contains("reference"), "{err}");
    }

    fn reference() -> Profile {
        ours().get("reference").unwrap().clone()
    }

    // The three ways a profile measures something other than what it says it measures, each of which produces numbers rather than an error.
    #[test]
    fn a_shared_core_is_refused() {
        let mut p = reference();
        p.bench_pin = "8-23".parse().unwrap();
        assert!(p.check().is_err());
    }

    #[test]
    fn a_sweep_past_the_pinned_cores_is_refused() {
        let mut p = reference();
        p.threads.push(24);
        let err = p.check().unwrap_err().to_string();
        assert!(err.contains("oversubscription"), "{err}");
    }

    #[test]
    fn a_key_space_that_does_not_fit_is_refused() {
        let mut p = reference();
        p.key_maximum = 200_000_000;
        let err = p.check().unwrap_err().to_string();
        assert!(err.contains("evict"), "{err}");
    }

    // Scaling maxmemory down for a smaller box without scaling the key space is the specific mistake this check exists for.
    #[test]
    fn shrinking_the_memory_alone_is_refused() {
        let mut p = reference();
        p.maxmemory = Bytes(2 * 1024 * 1024 * 1024);
        assert!(p.check().is_err());
        // Which is exactly what the small profile does correctly.
        assert!(ours().get("epyc8").unwrap().check().is_ok());
    }

    #[test]
    fn a_pin_off_the_end_of_the_machine_is_refused() {
        let mut p = reference();
        p.cores = 16;
        let err = p.check().unwrap_err().to_string();
        assert!(err.contains("bench_pin"), "{err}");
    }

    // The failure names the profile, because the file holds several and a message about a shared core with no name on it sends you to the wrong one.
    #[test]
    fn a_bad_profile_in_the_file_is_named() {
        let text = OURS.replace(r#"bench_pin = "16-31""#, r#"bench_pin = "8-23""#);
        let err = Profiles::parse(&text).unwrap_err().to_string();
        assert!(err.contains("reference"), "{err}");
        assert!(err.contains("share a core"), "{err}");
    }
}
