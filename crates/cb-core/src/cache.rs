//! The cache servers under test, and the argv each one is started with.
//!
//! The flags are the original's, read off `cmd/bench/main.go` rather than off its README, with the thread count and the memory limit coming from the profile instead of being constants.
//! They are here rather than in `cb-cache` because they are a table, they have no I/O in them, and a table with no I/O in it should be testable on a laptop with no cache server installed.

use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use crate::compat::Compat;
use crate::size::Bytes;

/// Garnet's index size, which the original hardcodes at two gigabytes regardless of the memory limit.
///
/// Left as it is rather than scaled with the profile, because scaling it is a change to how Garnet is configured and this table's job is to be the original's table.
const GARNET_INDEX: &str = "2g";

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

    /// Which protocol memtier should speak to this server.
    #[must_use]
    pub const fn protocol(self) -> Protocol {
        match self {
            Self::Memcache => Protocol::MemcacheText,
            _ => Protocol::Resp,
        }
    }

    /// The full command line for starting this server.
    ///
    /// Everything that varies between runs comes in through [`Launch`], so this is a table and not a policy.
    /// The thread flag, the memory limit, persistence off and the listening address are all any of the seven get.
    /// Nothing here is tuning, and the fairness rules in the spec say that a flag added for one server has to be considered for the other six or not added at all.
    #[must_use]
    pub fn argv(self, launch: &Launch<'_>) -> Vec<OsString> {
        let threads = launch.threads.to_string();
        let memory = self.memory_value(launch);
        let flags = self.flags(&threads, &memory);

        let mut argv: Vec<OsString> = Vec::with_capacity(flags.len() + 4);
        argv.push(launch.binary.into());
        argv.extend(flags.into_iter().map(OsString::from));

        let (port_flag, socket_flag) = self.listen_flags();
        match launch.endpoint {
            Endpoint::Tcp(port) => {
                argv.push(port_flag.into());
                argv.push(port.to_string().into());
            }
            Endpoint::Unix(path) => {
                argv.push(socket_flag.into());
                argv.push(path.into());
                // Every server but yo turns the TCP listener off by being given port zero, which is also how the original does it.
                if self == Self::Yo {
                    argv.push("--no-port".into());
                } else {
                    argv.push(port_flag.into());
                    argv.push("0".into());
                }
            }
        }

        // memcached refuses to start as root without being told that root is what you meant.
        if self == Self::Memcache && launch.as_root {
            argv.push("-u".into());
            argv.push("root".into());
        }

        argv
    }

    /// Everything between the binary and the listening address, which is where the seven look least alike.
    fn flags<'f>(self, threads: &'f str, memory: &'f str) -> Vec<&'f str> {
        match self {
            Self::Pogocache => vec!["-t", threads, "--maxmemory", memory],
            Self::Redis | Self::Valkey => vec![
                "--appendonly",
                "no",
                "--save",
                "",
                "--io-threads",
                threads,
                "--maxmemory",
                memory,
            ],
            Self::Dragonfly => vec![
                "--dir",
                "",
                "--dbfilename",
                "",
                "--proactor_threads",
                threads,
                "--maxmemory",
                memory,
            ],
            Self::Memcache => vec!["-m", memory, "-t", threads],
            Self::Garnet => vec![
                "--no-obj",
                "--aof-null-device",
                "--readcache",
                "false",
                "--index",
                GARNET_INDEX,
                "--memory",
                memory,
                "--miniothreads",
                threads,
                "--maxiothreads",
                threads,
                "--minthreads",
                threads,
                "--maxthreads",
                threads,
            ],
            Self::Yo => vec!["serve", "--maxmemory", memory, "--threads", threads],
        }
    }

    /// The memory limit, spelled the way this server spells it.
    fn memory_value(self, launch: &Launch<'_>) -> String {
        match self {
            // Megabytes, no unit.
            Self::Memcache => launch.maxmemory.mib().to_string(),
            // One letter units.
            Self::Garnet => launch.maxmemory.short(),
            // D12. The original computes Dragonfly's limit from the thread count and gets 31gb every time.
            Self::Dragonfly if launch.compat.is_upstream() => {
                let mb = u64::from(launch.threads).saturating_mul(256).max(32_384);
                format!("{}gb", mb / 1024)
            }
            _ => launch.maxmemory.to_string(),
        }
    }

    /// How this server spells the port flag and the unix socket flag.
    ///
    /// The two that came from the memcached side of the world take short flags and the five that came from the Redis side take long ones.
    const fn listen_flags(self) -> (&'static str, &'static str) {
        match self {
            Self::Memcache | Self::Pogocache => ("-p", "-s"),
            _ => ("--port", "--unixsocket"),
        }
    }
}

/// What memtier should speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The Redis protocol, which is memtier's default.
    Resp,
    /// The memcache text protocol.
    MemcacheText,
}

impl Protocol {
    /// The value for memtier's `--protocol`, or `None` when memtier's default is already right.
    ///
    /// The original passes the flag only for memcached and leaves it off otherwise, and since the flag ends up in the recorded command line, leaving it off is part of matching.
    #[must_use]
    pub const fn memtier(self) -> Option<&'static str> {
        match self {
            Self::Resp => None,
            Self::MemcacheText => Some("memcache_text"),
        }
    }
}

/// Where a server should listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint<'a> {
    /// A unix socket, which is what every sweep uses.
    ///
    /// It takes the network stack out of the measurement, which is the whole reason the original uses it.
    Unix(&'a Path),
    /// A TCP port.
    ///
    /// Only for a server whose unix socket support is missing or broken on the build in front of you, and a cell measured this way is not comparable with one that was not.
    Tcp(u16),
}

/// Everything about one server start that is not the server itself.
#[derive(Debug, Clone, Copy)]
pub struct Launch<'a> {
    /// The compiled server binary, from `config.jsonc`.
    pub binary: &'a Path,
    /// I/O threads, which is the x axis of every chart in the project.
    pub threads: u32,
    /// The memory limit, from the profile.
    pub maxmemory: Bytes,
    /// Where to listen.
    pub endpoint: Endpoint<'a>,
    /// Whether to reproduce the original's Dragonfly memory formula.
    pub compat: Compat,
    /// Whether this process is running as root, which only memcached cares about.
    pub as_root: bool,
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
    use std::path::Path;

    use super::{CacheKind, Endpoint, Launch, Protocol};
    use crate::compat::Compat;
    use crate::size::Bytes;

    fn launch(binary: &str) -> Launch<'_> {
        Launch {
            binary: Path::new(binary),
            threads: 4,
            maxmemory: Bytes(32 * 1024 * 1024 * 1024),
            endpoint: Endpoint::Unix(Path::new("/tmp/cb.sock")),
            compat: Compat::Corrected,
            as_root: false,
        }
    }

    fn words(kind: CacheKind, launch: &Launch<'_>) -> Vec<String> {
        kind.argv(launch)
            .into_iter()
            .map(|w| w.to_string_lossy().into_owned())
            .collect()
    }

    /// A command line written the way you would type it, into the argv a process actually gets.
    ///
    /// Two servers are given an empty argument, which is how Redis and Dragonfly are told to write nothing to disk, and there is no way to write that in a whitespace separated line. It is spelled `''` here and nowhere else.
    fn split(line: &str) -> Vec<String> {
        line.split(' ')
            .map(|w| if w == "''" { "" } else { w }.to_owned())
            .collect()
    }

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

    // Six of these are the original's command lines, word for word, with the thread count and the memory limit coming from the profile.
    // Spelled out in full rather than checked flag by flag, because a missing flag is the failure that matters here and a loop over flags is exactly what would not catch it.
    #[test]
    fn the_originals_command_lines() {
        let expect = [
            (
                CacheKind::Pogocache,
                "pogocache -t 4 --maxmemory 32gb -s /tmp/cb.sock -p 0",
            ),
            (
                CacheKind::Redis,
                "redis-server --appendonly no --save '' --io-threads 4 --maxmemory 32gb --unixsocket /tmp/cb.sock --port 0",
            ),
            (
                CacheKind::Valkey,
                "valkey-server --appendonly no --save '' --io-threads 4 --maxmemory 32gb --unixsocket /tmp/cb.sock --port 0",
            ),
            (
                CacheKind::Dragonfly,
                "dragonfly --dir '' --dbfilename '' --proactor_threads 4 --maxmemory 32gb --unixsocket /tmp/cb.sock --port 0",
            ),
            (
                CacheKind::Memcache,
                "memcached -m 32768 -t 4 -s /tmp/cb.sock -p 0",
            ),
            (
                CacheKind::Garnet,
                "GarnetServer --no-obj --aof-null-device --readcache false --index 2g --memory 32g --miniothreads 4 --maxiothreads 4 --minthreads 4 --maxthreads 4 --unixsocket /tmp/cb.sock --port 0",
            ),
        ];
        for (kind, want) in expect {
            let want = split(want);
            assert_eq!(words(kind, &launch(&want[0])), want, "{kind}");
        }
    }

    // yo is ours and it is not in the original, so this is the one command line with nobody to check it against.
    // Same four things every other server gets and nothing else, which is the fairness rule the spec sets out.
    #[test]
    fn yo_gets_the_same_four_things_everybody_else_gets() {
        assert_eq!(
            words(CacheKind::Yo, &launch("yodb")),
            [
                "yodb",
                "serve",
                "--maxmemory",
                "32gb",
                "--threads",
                "4",
                "--unixsocket",
                "/tmp/cb.sock",
                "--no-port",
            ]
        );
    }

    // Nothing is ever tuned per server beyond the four things, so no argv may carry a flag the others have no equivalent of.
    // Persistence is the only thing that is a flag on some and not a concept on others.
    #[test]
    fn every_server_gets_a_thread_count_a_memory_limit_and_a_socket() {
        let l = launch("bin");
        for kind in CacheKind::ALL {
            let argv = words(kind, &l);
            assert!(argv.iter().any(|w| w == "4"), "{kind} has no thread count");
            assert!(
                argv.iter().any(|w| w.starts_with("32")),
                "{kind} has no memory limit"
            );
            assert!(
                argv.iter().any(|w| w == "/tmp/cb.sock"),
                "{kind} has no socket"
            );
        }
    }

    // D12. The original floors Dragonfly's limit at 32384 megabytes and then divides by 1024 in integer arithmetic, which is 31, so Dragonfly is the only server that never gets the 32 gigabytes every other server gets.
    #[test]
    fn dragonfly_gets_31gb_upstream_and_the_profiles_limit_here() {
        let mut l = launch("dragonfly");
        assert!(words(CacheKind::Dragonfly, &l).contains(&"32gb".to_owned()));

        l.compat = Compat::Upstream;
        for threads in [1_u32, 4, 16] {
            l.threads = threads;
            assert!(
                words(CacheKind::Dragonfly, &l).contains(&"31gb".to_owned()),
                "{threads} threads"
            );
        }
        // Nothing sweeps this high, but the formula only stops producing 31 above 128 threads and the arithmetic should be the original's rather than the constant it happens to produce.
        l.threads = 128;
        assert!(words(CacheKind::Dragonfly, &l).contains(&"32gb".to_owned()));
    }

    // The one flag that depends on who is running the sweep rather than on what is being measured.
    #[test]
    fn memcached_is_told_that_root_was_deliberate() {
        let mut l = launch("memcached");
        assert!(!words(CacheKind::Memcache, &l).contains(&"-u".to_owned()));
        l.as_root = true;
        let argv = words(CacheKind::Memcache, &l);
        assert_eq!(&argv[argv.len() - 2..], ["-u", "root"]);

        // Nobody else grows a flag from it.
        let plain = launch("memcached");
        for kind in CacheKind::ALL {
            if kind != CacheKind::Memcache {
                assert_eq!(words(kind, &l), words(kind, &plain), "{kind}");
            }
        }
    }

    // TCP is for a server whose unix socket support is broken on the build in front of you, and the port flag is spelled two ways across the seven.
    #[test]
    fn tcp_replaces_the_socket_rather_than_joining_it() {
        let mut l = launch("bin");
        l.endpoint = Endpoint::Tcp(6379);
        for kind in CacheKind::ALL {
            let flag = if matches!(kind, CacheKind::Memcache | CacheKind::Pogocache) {
                "-p"
            } else {
                "--port"
            };
            let argv = words(kind, &l);
            assert_eq!(&argv[argv.len() - 2..], [flag, "6379"], "{kind}");
            assert!(
                !argv.iter().any(|w| w == "/tmp/cb.sock"),
                "{kind} is still listening on a socket as well"
            );
        }
    }

    #[test]
    fn only_memcached_needs_a_protocol_flag() {
        for kind in CacheKind::ALL {
            let want = if kind == CacheKind::Memcache {
                Some("memcache_text")
            } else {
                None
            };
            assert_eq!(kind.protocol().memtier(), want, "{kind}");
        }
        assert_eq!(CacheKind::Yo.protocol(), Protocol::Resp);
    }
}
