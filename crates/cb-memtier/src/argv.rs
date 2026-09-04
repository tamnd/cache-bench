//! What gets handed to `memtier_benchmark`.
//!
//! Three invocations per measured run: a warmup SET pass that is thrown away, a measured SET pass and a measured GET pass. All three are the same command line apart from the ratio and where the JSON lands, which is how the original does it and there is no reason to change it.
//!
//! Building this in one place, as data, is the point. The original spreads the same flags across a shell script and a Go program whose defaults disagree with the script, so reading either one tells you what might have been measured rather than what was. Here there is one function, it is pure, and a test compares its output against the exact argv the published results were produced with.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use cb_core::{Profile, Protocol};

/// Which of the three invocations this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// The SET pass that fills the key space and is then discarded.
    ///
    /// Not a courtesy. Several of these engines allocate lazily, so without it the measured SET pass is partly a measurement of hash table growth, and the engines grow differently.
    Warmup,
    /// The measured SET pass.
    Sets,
    /// The measured GET pass.
    Gets,
}

impl Pass {
    /// memtier's SET to GET ratio for this pass.
    #[must_use]
    pub const fn ratio(self) -> &'static str {
        match self {
            Self::Warmup | Self::Sets => "1:0",
            Self::Gets => "0:1",
        }
    }

    /// What this pass is called in a log line and in the name of its JSON file.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Sets => "sets",
            Self::Gets => "gets",
        }
    }

    /// Whether the result is kept.
    #[must_use]
    pub const fn measured(self) -> bool {
        !matches!(self, Self::Warmup)
    }
}

/// One `memtier_benchmark` command line.
#[derive(Debug)]
pub struct Invocation<'a> {
    /// Where the sweep's shape comes from.
    pub profile: &'a Profile,
    /// The pipeline depth for this cell.
    pub pipeline: u32,
    /// Which pass.
    pub pass: Pass,
    /// Which wire protocol the server under test speaks.
    pub protocol: Protocol,
    /// The unix socket the server is listening on.
    pub socket: &'a Path,
    /// Where memtier writes its own JSON, which is then read back and thrown away.
    pub json_out: &'a Path,
}

/// The percentiles asked for, which are the five the result file carries.
///
/// MIN, MAX and AVG are reported by memtier without being asked for, which is why there are eight latency figures in a result file and only five here.
pub const PERCENTILES: &str = "50,90,99,99.9,99.99";

impl Invocation<'_> {
    /// The argument list, not counting the binary itself.
    ///
    /// Order is the original's. It makes no difference to memtier and it makes a large difference to anybody diffing a log line here against a log line there.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let profile = self.profile;
        let mut out: Vec<OsString> = vec![
            "-c".into(),
            profile.connections_per_thread.to_string().into(),
            "-t".into(),
            profile.bench_threads.to_string().into(),
            "-n".into(),
            profile.operations.to_string().into(),
            "--distinct-client-seed".into(),
            "--hide-histogram".into(),
            "--key-prefix".into(),
            "".into(),
            "--ratio".into(),
            self.pass.ratio().into(),
            "--data-size-range".into(),
            profile.size_range.to_string().into(),
            "--pipeline".into(),
            self.pipeline.to_string().into(),
            "--json-out-file".into(),
            self.json_out.as_os_str().to_owned(),
            "--print-percentiles".into(),
            PERCENTILES.into(),
            "--key-pattern=P:P".into(),
            // Explicit, where the original leaves it at memtier's default of ten million.
            // The default is only safe because the original's memory limit is thirty two gigabytes. A profile that shrinks the limit without shrinking this turns the whole thing into an eviction benchmark that still produces plausible numbers. Recorded as D8.
            "--key-maximum".into(),
            profile.key_maximum.to_string().into(),
            "-S".into(),
            self.socket.as_os_str().to_owned(),
        ];
        if self.protocol == Protocol::MemcacheText {
            out.push("--protocol".into());
            out.push("memcache_text".into());
        }
        out
    }

    /// The argv as one line, for a log.
    #[must_use]
    pub fn line(&self, binary: &Path) -> String {
        let mut out = binary.display().to_string();
        for arg in self.argv() {
            out.push(' ');
            out.push_str(&quote(&arg));
        }
        out
    }
}

/// An argument as it would have to be typed into a shell.
///
/// Only for logs. Nothing here is ever handed to a shell, because the argv goes straight to `execve`, but a log line that cannot be pasted back into a terminal is a log line nobody can reproduce a run from. The empty key prefix is the one that matters, since it vanishes without this.
fn quote(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_=/.,:".contains(c));
    if plain {
        return text.into_owned();
    }
    format!("\"{}\"", text.replace('"', "\\\""))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a profile that will not parse is a failed test"
)]
mod tests {
    use std::path::Path;

    use cb_core::{Profile, Profiles, Protocol};

    use super::{Invocation, Pass};

    fn profile() -> Profile {
        Profiles::parse(include_str!("../../../profiles.toml"))
            .expect("the committed profiles parse")
            .profiles
            .get("reference")
            .expect("the reference profile is the original's own")
            .clone()
    }

    fn argv(pass: Pass, protocol: Protocol) -> Vec<String> {
        Invocation {
            profile: &profile(),
            pipeline: 1,
            pass,
            protocol,
            socket: Path::new("/tmp/cachebench.sock"),
            json_out: Path::new("/tmp/bench-set.json"),
        }
        .argv()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
    }

    // The argv the original's published numbers were produced with, on the profile that describes its machine.
    // Everything after this test is a variation on this list, so if this drifts the whole comparison to the original is void.
    #[test]
    fn the_set_pass_is_the_originals_own_command_line() {
        let want = [
            "-c",
            "16",
            "-t",
            "16",
            "-n",
            "100000",
            "--distinct-client-seed",
            "--hide-histogram",
            "--key-prefix",
            "",
            "--ratio",
            "1:0",
            "--data-size-range",
            "1-1024",
            "--pipeline",
            "1",
            "--json-out-file",
            "/tmp/bench-set.json",
            "--print-percentiles",
            "50,90,99,99.9,99.99",
            "--key-pattern=P:P",
            "--key-maximum",
            "10000000",
            "-S",
            "/tmp/cachebench.sock",
        ];
        assert_eq!(argv(Pass::Sets, Protocol::Resp), want);
    }

    // The warmup is the SET pass run twice, not a lighter version of it. A warmup that touched fewer keys would leave the measured pass paying for the rest.
    #[test]
    fn the_warmup_is_the_set_pass() {
        assert_eq!(
            argv(Pass::Warmup, Protocol::Resp),
            argv(Pass::Sets, Protocol::Resp)
        );
    }

    #[test]
    fn the_get_pass_differs_only_in_the_ratio() {
        let sets = argv(Pass::Sets, Protocol::Resp);
        let gets = argv(Pass::Gets, Protocol::Resp);
        let differ: Vec<usize> = (0..sets.len()).filter(|&i| sets[i] != gets[i]).collect();
        assert_eq!(differ.len(), 1, "{sets:?} against {gets:?}");
        assert_eq!(sets[differ[0]], "1:0");
        assert_eq!(gets[differ[0]], "0:1");
    }

    #[test]
    fn memcached_gets_the_text_protocol_and_nothing_else_does() {
        let text = argv(Pass::Sets, Protocol::MemcacheText);
        assert_eq!(&text[text.len() - 2..], ["--protocol", "memcache_text"]);
        assert!(!argv(Pass::Sets, Protocol::Resp).contains(&"--protocol".to_owned()));
    }

    // The original leaves this at memtier's default, which is only safe at its own memory limit.
    #[test]
    fn the_key_space_is_stated_rather_than_defaulted() {
        assert!(argv(Pass::Sets, Protocol::Resp).contains(&"--key-maximum".to_owned()));
    }

    // An empty argument that disappears from a log line is a log line that reproduces a different run.
    #[test]
    fn the_logged_line_keeps_the_empty_key_prefix() {
        let line = Invocation {
            profile: &profile(),
            pipeline: 10,
            pass: Pass::Gets,
            protocol: Protocol::Resp,
            socket: Path::new("/tmp/cachebench.sock"),
            json_out: Path::new("/tmp/bench-get.json"),
        }
        .line(Path::new("/usr/bin/memtier_benchmark"));
        assert!(line.contains("--key-prefix \"\""), "{line}");
        assert!(
            line.starts_with("/usr/bin/memtier_benchmark -c 16"),
            "{line}"
        );
    }
}
