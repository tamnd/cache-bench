//! Reading the resident set of a running server.
//!
//! Linux only, and it says so rather than returning a zero. `/proc/<pid>/status` is where `VmHWM` lives and there is no portable equivalent: macOS has `mach_task_basic_info`, which reports a high water mark for a task the caller can already inspect and needs an entitlement otherwise, and Windows has `GetProcessMemoryInfo`. Neither of those machines is one this project publishes numbers from, so the honest thing is a refusal on them and no `#[cfg]` maze.
//!
//! Every engine measured so far is a single process with threads in it, so the sum below is over one entry. It is a sum anyway, because an engine that forks workers would otherwise be reported at the size of whichever process happened to be its leader, and that number is small, plausible and wrong.

use std::fs;
use std::path::Path;

use crate::status::{self, Resident};

/// What a process group is holding, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// The sum of every member's resident set right now.
    pub now: u64,
    /// The sum of every member's high water mark.
    ///
    /// A sum of peaks rather than a peak of sums, which is an upper bound when the peaks did not happen at the same moment. For one process they are the same thing.
    pub peak: u64,
    /// How many processes were in the group.
    pub processes: u32,
}

/// Read what the group led by this pid is holding.
///
/// # Errors
///
/// On a machine with no `/proc`, on a group with nothing in it, or on a member whose status file does not say what it holds. A memory measurement that cannot read memory is a failed measurement, not a nought.
pub fn group(pid: u32) -> Result<Sample, NoSample> {
    let proc = Path::new("/proc");
    if !proc.is_dir() {
        return Err(NoSample::NotLinux);
    }
    let mut sample = Sample {
        now: 0,
        peak: 0,
        processes: 0,
    };
    for member in members(proc, pid)? {
        let text = match fs::read_to_string(proc.join(member.to_string()).join("status")) {
            Ok(text) => text,
            // Between listing the group and reading this member, it exited. Not the leader's problem and not a failure: the leader is checked for separately by the caller, which is what says whether the server survived its own measurement.
            Err(_) if member != pid => continue,
            Err(why) => return Err(NoSample::Unreadable(member, why.to_string())),
        };
        let Resident { now, peak } =
            status::parse(&text).map_err(|why| NoSample::Unreadable(member, why.to_string()))?;
        sample.now = sample.now.saturating_add(now);
        sample.peak = sample.peak.saturating_add(peak);
        sample.processes = sample.processes.saturating_add(1);
    }
    if sample.processes == 0 {
        return Err(NoSample::Gone(pid));
    }
    Ok(sample)
}

/// Every pid whose process group is this one.
///
/// Read out of each `stat` file rather than by asking for the group directly, because there is no call that lists a group and the alternative is a `getpgid` per pid, which is the same walk with a syscall in it.
fn members(proc: &Path, pid: u32) -> Result<Vec<u32>, NoSample> {
    let entries = fs::read_dir(proc).map_err(|why| NoSample::Unreadable(pid, why.to_string()))?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(candidate) = name.parse::<u32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if pgid(&stat) == Some(pid) {
            out.push(candidate);
        }
    }
    Ok(out)
}

/// The process group id out of the text of a `/proc/<pid>/stat`.
///
/// Field five, one based, in a line whose second field is the executable name in brackets and may contain spaces and brackets of its own. Everything before the last `)` is skipped for exactly that reason: a server called `redis (test)` would otherwise shift every field after it and the group id read would be a number belonging to something else.
fn pgid(stat: &str) -> Option<u32> {
    let after = stat.rsplit_once(')')?.1;
    // After the closing bracket the fields are state, ppid, pgrp, so the group is the third.
    after.split_whitespace().nth(2)?.parse().ok()
}

/// A resident set that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoSample {
    /// No `/proc` on this machine.
    #[error(
        "memory is measured by reading /proc, which this machine does not have, so run this on the Linux host the numbers are meant to come from"
    )]
    NotLinux,
    /// The group had nothing in it by the time it was read.
    #[error(
        "nothing is left of the process group led by {0}, so it exited during its own measurement"
    )]
    Gone(u32),
    /// A member is there and will not say what it holds.
    #[error("cannot read what process {0} is holding: {1}")]
    Unreadable(u32, String),
}

#[cfg(test)]
mod tests {
    use super::pgid;

    #[test]
    fn the_group_is_the_third_field_after_the_name() {
        assert_eq!(
            pgid("3311 (redis-server) S 1 3311 3311 0 -1").as_ref(),
            Some(&3311)
        );
    }

    // A process name with a space and a bracket in it shifts every field after it for anything that splits the whole line on whitespace.
    #[test]
    fn a_name_with_brackets_in_it_does_not_move_the_fields() {
        assert_eq!(pgid("42 (redis (test)) S 1 99 99 0 -1"), Some(99));
        assert_eq!(pgid("42 (a b c) S 1 77 77 0 -1"), Some(77));
    }

    #[test]
    fn a_line_that_is_not_a_stat_is_none_rather_than_a_number() {
        assert_eq!(pgid(""), None);
        assert_eq!(pgid("3311 (redis-server"), None);
        assert_eq!(pgid("3311 (redis-server) S"), None);
    }
}
