//! What the kernel says a process is holding.
//!
//! `/proc/<pid>/status` carries `VmRSS`, the resident set right now, and `VmHWM`, the largest it has ever been. Both are in kilobytes, both are absent on a process that has already gone, and `VmHWM` is absent on kernels built without it.
//!
//! The peak is the one that matters. A cache server that allocated a table, filled it, and then had a chunk of it swapped or reclaimed reports a smaller `VmRSS` than the machine actually had to have, and the question this measurement answers is what the machine had to have.
//!
//! The parsing is a function over text rather than over a path, so the awkward cases have tests on a machine with no `/proc` on it.

use std::fmt;

/// What one process is holding, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resident {
    /// The resident set at the moment the file was read.
    pub now: u64,
    /// The largest resident set this process has ever had.
    pub peak: u64,
}

/// Read `VmRSS` and `VmHWM` out of the text of a `/proc/<pid>/status`.
///
/// # Errors
///
/// If either line is missing or does not read as kilobytes. A process that exited between being listed and being read has neither, which is a failed measurement rather than a zero, because a zero here divides into a bytes-per-entry figure that looks like a very good result.
pub fn parse(text: &str) -> Result<Resident, BadStatus> {
    let field = |name: &str| -> Result<u64, BadStatus> {
        let line = text
            .lines()
            .find_map(|line| {
                line.strip_prefix(name)
                    .and_then(|rest| rest.strip_prefix(':'))
            })
            .ok_or_else(|| BadStatus::Missing(name.to_owned()))?;
        // `VmRSS:	    5432 kB`, with the unit always kB whatever the size, which is why this is not a units parser.
        let value = line
            .trim()
            .strip_suffix(" kB")
            .ok_or_else(|| BadStatus::NotKilobytes(name.to_owned(), line.trim().to_owned()))?;
        let kb: u64 = value
            .trim()
            .parse()
            .map_err(|_| BadStatus::NotKilobytes(name.to_owned(), line.trim().to_owned()))?;
        kb.checked_mul(1024)
            .ok_or_else(|| BadStatus::NotKilobytes(name.to_owned(), line.trim().to_owned()))
    };
    Ok(Resident {
        now: field("VmRSS")?,
        peak: field("VmHWM")?,
    })
}

/// A status file that does not say what a memory measurement needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadStatus {
    /// The line is not there, which on a live process means a kernel that does not publish it.
    Missing(String),
    /// The line is there and is not a number of kilobytes.
    NotKilobytes(String, String),
}

impl fmt::Display for BadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(
                f,
                "the process status has no {name} line, so how much memory it held cannot be read"
            ),
            Self::NotKilobytes(name, saw) => write!(
                f,
                "the {name} line reads {saw:?} rather than a number of kilobytes"
            ),
        }
    }
}

impl std::error::Error for BadStatus {}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{BadStatus, parse};

    const REAL: &str = "\
Name:\tredis-server
Umask:\t0022
State:\tS (sleeping)
Tgid:\t3311
Pid:\t3311
VmPeak:\t 6291456 kB
VmSize:\t 6291456 kB
VmLck:\t       0 kB
VmHWM:\t 4194304 kB
VmRSS:\t 4194304 kB
Threads:\t9
";

    #[test]
    fn kilobytes_come_back_as_bytes() {
        let seen = parse(REAL).unwrap();
        assert_eq!(seen.peak, 4_194_304 * 1024);
        assert_eq!(seen.now, 4_194_304 * 1024);
    }

    // VmPeak is virtual and sits directly above VmHWM in the file, so a prefix match that did not anchor on the colon would find the wrong line and report six gigabytes for a server holding four.
    #[test]
    fn the_peak_is_the_resident_one_and_not_the_virtual_one() {
        assert_eq!(parse(REAL).unwrap().peak, 4_194_304 * 1024);
        assert_ne!(parse(REAL).unwrap().peak, 6_291_456 * 1024);
    }

    // A process that exited between being listed and being read: the kernel keeps the file and drops the memory lines from it.
    #[test]
    fn a_process_that_has_gone_is_an_error_rather_than_nought() {
        let gone = "Name:\tredis-server\nState:\tZ (zombie)\nThreads:\t1\n";
        assert_eq!(
            parse(gone),
            Err(BadStatus::Missing("VmRSS".to_owned())),
            "a zombie divides into a very good bytes-per-entry number"
        );
    }

    #[test]
    fn a_line_that_is_not_kilobytes_says_what_it_was() {
        let odd = "VmHWM:\t 100 MB\nVmRSS:\t 100 kB\n";
        let err = parse(odd).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the VmHWM line reads \"100 MB\" rather than a number of kilobytes"
        );
    }

    #[test]
    fn nothing_at_all_is_an_error() {
        assert!(parse("").is_err());
    }
}
