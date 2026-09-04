//! Whether this machine can count cycles.
//!
//! Two checks, because either one alone gives the wrong answer on a machine somebody will actually run this on.
//!
//! The directory check alone says yes inside a virtual machine that exposes a `cpu` event source and then answers `<not supported>` for every hardware event, which is the common case and is exactly the case that matters. The live check alone says no on a machine with a perfectly good PMU where `perf_event_paranoid` is turned up, which is a permissions problem with a fix rather than a property of the hardware.
//!
//! Running both means the answer distinguishes the three states that lead to three different actions: measure cycles, do not measure cycles and say why, or lower the paranoid setting and try again.

use std::path::Path;
use std::process::Command;

use crate::csv;

/// What a probe found.
///
/// The reason is carried along with the answer because a host that cannot count cycles ends up saying so in its generated README, and "no" on its own is not something a reader can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Whether cycles can be counted on this host, which is whether the cycles half of the matrix is worth running.
    pub counted: bool,
    /// Why, in a sentence, for `doctor` and for the generated README.
    pub reason: String,
}

/// Where the kernel lists the event sources it has.
const SOURCES: &str = "/sys/bus/event_source/devices";

/// What `perf_event_paranoid` has to be at or below for per process counting on a process we started.
///
/// Two, which is what most distributions ship and which forbids kernel and CPU wide measurements while still allowing user space counting on a process of our own. Three and above forbid that as well, and a host set that way needs the setting lowered rather than different hardware.
const PARANOID: i32 = 2;

/// Ask the machine.
///
/// Runs `perf stat` once over a command that does nothing, which is the only way to find out whether the counters answer rather than whether they exist.
#[must_use]
pub fn probe() -> Probe {
    if !Path::new(SOURCES).join("cpu").exists() && !Path::new(SOURCES).join("cpu_core").exists() {
        return Probe {
            counted: false,
            reason: format!(
                "the kernel lists no cpu event source under {SOURCES}, which is what a virtual machine without a passed through PMU looks like"
            ),
        };
    }
    if let Some(level) = paranoid()
        && level > PARANOID
    {
        return Probe {
            counted: false,
            reason: format!(
                "perf_event_paranoid is {level}, and counting on a process needs {PARANOID} or lower, so this is a setting rather than the hardware"
            ),
        };
    }
    live()
}

/// What the kernel will let an unprivileged process count.
fn paranoid() -> Option<i32> {
    std::fs::read_to_string("/proc/sys/kernel/perf_event_paranoid")
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Count cycles over a command that does nothing, and see what comes back.
///
/// This is the one process the harness runs that does not go through the supervisor, because it is not a server. It exits on its own after counting a command that does nothing, there is nothing to pin it to and nothing to leave behind, and waiting for it is the whole point.
fn live() -> Probe {
    let out = Command::new("perf")
        .args(["stat", "-x,", "-e", "cycles", "true"])
        .output();
    let Ok(out) = out else {
        return Probe {
            counted: false,
            reason: "perf is not on the path, so nothing here can count anything".to_owned(),
        };
    };
    // perf stat writes its counters to stderr, and the command it ran writes to stdout.
    let text = String::from_utf8_lossy(&out.stderr);
    match csv::read(&text, 0.0) {
        Ok(perf) if csv::counted(&perf) => Probe {
            counted: true,
            reason: "perf counted cycles on a live process, so the counters are real".to_owned(),
        },
        Ok(_) => Probe {
            counted: false,
            reason: "the kernel lists a cpu event source but answers <not supported> for cycles, which is a virtual machine without a passed through PMU".to_owned(),
        },
        Err(why) => Probe {
            counted: false,
            reason: format!("perf ran and produced nothing usable: {why}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{PARANOID, probe};

    // The probe has to answer on any machine rather than failing on one without perf, because it is what decides whether the cycles half of the matrix runs at all.
    #[test]
    fn the_probe_answers_with_a_reason_on_whatever_this_is() {
        let found = probe();
        assert!(!found.reason.is_empty());
    }

    // Two forbids kernel measurements and still allows counting our own process, which is all this needs.
    #[test]
    fn the_paranoid_ceiling_still_allows_counting_our_own_process() {
        assert_eq!(PARANOID, 2);
    }
}
