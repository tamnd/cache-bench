//! `host.json`, the record of what a results directory was measured on.
//!
//! One per results directory, written by `doctor` before a sweep starts and read by everything that has to describe the numbers afterwards. The original has no equivalent, and the absence is why its published charts cannot say what produced them.
//!
//! What is in here is what a reader needs in order to decide whether two numbers are comparable: the kernel, the CPU, whether the machine has a hardware PMU, what the governor was doing and the load generator's version. The engine versions are not in here, because every run in `output.json` already carries the version line its server printed, and a second copy of a list is a list that will eventually disagree with the first. What is deliberately not in here is anything naming the machine. No hostname, no address, no user. A results directory is published and a machine name is not something to publish, so a host is described by what it is rather than by what it is called, and there is a test in here that says so.

use serde::{Deserialize, Serialize};

/// What a results directory was measured on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    /// Which hardware profile the sweep ran, which is the name in `profiles.toml`.
    pub profile: String,
    /// The kernel, as `uname` gives it.
    pub kernel: String,
    /// The distribution, for the userland the servers were built against.
    pub distro: String,
    /// The CPU, as the machine describes itself.
    pub cpu_model: String,
    /// How many logical CPUs it has.
    pub cpus: u32,
    /// How much memory it has, in bytes.
    pub memory_bytes: u64,
    /// Whether a hardware PMU was there to be counted with, which decides whether there are any cycles charts at all.
    pub pmu: Pmu,
    /// What the frequency governor was set to, since a chart measured under a governor that ramps is a chart of the governor.
    pub governor: String,
    /// Which CPU mitigations were on, since several of them are worth double digit percentages on a syscall heavy workload.
    pub mitigations: String,
    /// The load generator's own version line, which is the one version that is not in `output.json` because memtier is not one of the things being measured.
    pub memtier: String,
    /// What produced the results directory.
    pub cache_bench: Tool,
    /// The compiler that built the engines that are built from source, which is `yo` and nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rustc: Option<String>,
    /// When the sweep started, RFC 3339 in UTC.
    pub started: String,
    /// When it finished, absent while it is still running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<String>,
}

/// Whether the machine can count cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pmu {
    /// Counters are real and the cycles charts mean something.
    Present,
    /// No counters, which is the normal case in a virtual machine and means this results directory has no cycles charts in it.
    Absent,
}

impl Pmu {
    /// How it is written in a generated document.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Present => "yes, cycles per operation was measured",
            Self::Absent => "no, this host exposes no hardware PMU",
        }
    }
}

/// What wrote the results directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    /// The `cache-bench` version.
    pub version: String,
    /// The commit it was built from, so that a results directory can be traced to the code that made it.
    pub git: String,
}

impl Machine {
    /// Read a `host.json`.
    ///
    /// # Errors
    ///
    /// If the file is not JSON of the right shape.
    pub fn parse(text: &str) -> Result<Self, BadMachine> {
        serde_json::from_str(text).map_err(|e| BadMachine::Shape(e.to_string()))
    }

    /// Write one back.
    ///
    /// Pretty printed with a trailing newline, because this file is read by people rather than by the chart engine and it lands in a diff every time a sweep is rerun.
    #[must_use]
    pub fn emit(&self) -> String {
        // Falls back to something that will not parse rather than panicking, on a shape that cannot occur.
        let mut text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "null".to_owned());
        text.push('\n');
        text
    }

    /// Whether anything in here looks like it names the machine rather than describes it.
    ///
    /// Checked at write time and again in a test. The rule is section 02's and it is absolute: nothing under `results/` carries a hostname.
    ///
    /// # Errors
    ///
    /// If any named string appears anywhere in the record.
    pub fn check_anonymous(&self, names: &[&str]) -> Result<(), BadMachine> {
        let text = self.emit().to_lowercase();
        for name in names {
            if name.is_empty() {
                continue;
            }
            if text.contains(&name.to_lowercase()) {
                return Err(BadMachine::Named {
                    name: (*name).to_owned(),
                });
            }
        }
        Ok(())
    }
}

/// Anything that stops a host record being usable.
#[derive(Debug, thiserror::Error)]
pub enum BadMachine {
    /// The file is not JSON of the right shape.
    #[error("host.json is not readable: {0}")]
    Shape(String),
    /// It names the machine.
    #[error("host.json contains {name:?}, and a results directory does not carry machine names")]
    Named {
        /// What was found.
        name: String,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Machine, Pmu, Tool};

    fn machine() -> Machine {
        Machine {
            profile: "wsl32".to_owned(),
            kernel: "Linux 6.18.33.2-microsoft-standard-WSL2 x86_64".to_owned(),
            distro: "Ubuntu 26.04 LTS".to_owned(),
            cpu_model: "AMD Ryzen 9 7950X 16-Core Processor".to_owned(),
            cpus: 32,
            memory_bytes: 55_834_574_848,
            pmu: Pmu::Absent,
            governor: "performance".to_owned(),
            mitigations: "mitigations=on".to_owned(),
            memtier: "memtier_benchmark 2.4.4".to_owned(),
            cache_bench: Tool {
                version: "0.4.0".to_owned(),
                git: "4b18e6b".to_owned(),
            },
            rustc: Some("rustc 1.98.0".to_owned()),
            started: "2026-09-01T09:12:03Z".to_owned(),
            finished: None,
        }
    }

    #[test]
    fn a_record_reads_back_as_what_was_written() {
        let before = machine();
        let after = Machine::parse(&before.emit()).unwrap();
        assert_eq!(before, after);
    }

    // A sweep that is still running has no finish time, and the key is left out rather than written as null.
    #[test]
    fn an_unfinished_sweep_omits_the_finish_time() {
        let text = machine().emit();
        assert!(!text.contains("finished"), "{text}");
        let mut done = machine();
        done.finished = Some("2026-09-14T04:41:19Z".to_owned());
        assert!(done.emit().contains("2026-09-14T04:41:19Z"));
    }

    #[test]
    fn the_pmu_is_a_word_rather_than_a_flag() {
        let text = machine().emit();
        assert!(text.contains("\"pmu\": \"absent\""), "{text}");
    }

    // The rule that matters most about this file: it describes a machine and never names one.
    #[test]
    fn a_record_that_names_the_machine_is_caught() {
        let mut named = machine();
        assert!(named.check_anonymous(&["server3", "gamingpc"]).is_ok());
        named.kernel = "Linux server3 6.18.33.2 x86_64".to_owned();
        let why = named
            .check_anonymous(&["server3", "gamingpc"])
            .unwrap_err()
            .to_string();
        assert!(why.contains("server3"), "{why}");
    }

    #[test]
    fn the_check_is_case_insensitive_and_ignores_empty_names() {
        let mut named = machine();
        named.distro = "Ubuntu 26.04 LTS on GamingPC".to_owned();
        assert!(named.check_anonymous(&["gamingpc"]).is_err());
        assert!(named.check_anonymous(&[""]).is_ok());
    }
}
