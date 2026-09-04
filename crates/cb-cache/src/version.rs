//! Asking a server binary what it is.
//!
//! The string this returns goes into every result file that server produces, and it is the only record of what was actually measured. The README's version table is written from it rather than typed, so a build that was swapped halfway through a sweep shows up as two different strings in the results instead of as a footnote nobody wrote.
//!
//! It is taken the way the original takes it: run the binary with `--version`, keep the first line, strip the colour escapes out of it and trim. Nothing here tries to parse a version number out of the text, because the seven of them do not agree on a format and a number that was parsed wrong is worse than the line the server printed.

use std::path::Path;
use std::time::Duration;

use crate::supervise::{BadProcess, Supervisor};

/// How long a binary gets to answer `--version`.
///
/// Five seconds. This is not a benchmark, it is a process that prints one line and exits, and the only reason it takes any measurable time is Garnet starting a .NET runtime first. A binary that has not answered by now is not going to, and finding that out in five seconds beats finding it out when a sweep that was meant to run overnight is still on its first server in the morning.
const GRACE: Duration = Duration::from_secs(5);

/// Run `<binary> --version` and keep the first line.
///
/// `capture` is where the output goes, and it is a file in the run directory rather than a pipe, so that a binary which refused to answer leaves behind whatever it did say.
///
/// # Errors
///
/// If the binary will not start, if it does not answer within a few seconds, or if it exits without printing anything.
pub fn version(binary: &Path, capture: &Path) -> Result<String, NoVersion> {
    let mut asking = Supervisor::new(binary)
        .args(["--version"])
        .log(capture)
        .start()
        .map_err(NoVersion::NotStarted)?;
    let answered = asking.settle(GRACE).map_err(NoVersion::NotStarted)?;
    if !answered {
        // Stopping it is the destructor's job, and the whole group goes with it.
        return Err(NoVersion::Silent {
            binary: binary.display().to_string(),
        });
    }
    let text = std::fs::read_to_string(capture).map_err(|why| NoVersion::NoCapture {
        capture: capture.display().to_string(),
        why: why.to_string(),
    })?;
    first_line(&text).ok_or_else(|| NoVersion::NothingPrinted {
        binary: binary.display().to_string(),
    })
}

/// The first line worth keeping, stripped and trimmed.
///
/// Leading blank lines are skipped rather than returned as an empty version, because at least one of these binaries prints a newline before its banner and an empty string in a result file looks like a harness that never asked.
#[must_use]
pub fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(|line| strip_colour(line).trim().to_owned())
        .find(|line| !line.is_empty())
}

/// Take the ANSI colour escapes out of a line.
///
/// One of these binaries colours its banner, and a version string with `\u{1b}[0m` in the middle of it ends up in a JSON file, in a README table and eventually on a chart. This drops the escape sequences and keeps everything else, including any text that merely looks like one, because a version is text and guessing at its meaning is not this function's job.
#[must_use]
pub fn strip_colour(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line.chars();
    while let Some(c) = rest.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // An escape sequence here is `ESC [` then digits and semicolons then one letter. Anything else is not one, and is kept.
        let mut ahead = rest.clone();
        if ahead.next() != Some('[') {
            out.push(c);
            continue;
        }
        let mut ended = false;
        for c in ahead.by_ref() {
            if c.is_ascii_digit() || c == ';' {
                continue;
            }
            ended = c.is_ascii_alphabetic();
            break;
        }
        if ended {
            rest = ahead;
        } else {
            out.push(c);
        }
    }
    out
}

/// A binary that would not say what it is.
#[derive(Debug, thiserror::Error)]
pub enum NoVersion {
    /// It could not be run, which is nearly always a path in `config.jsonc` that points at nothing.
    #[error("{0}")]
    NotStarted(#[from] BadProcess),
    /// It ran and never finished.
    #[error(
        "{binary} did not answer --version within {GRACE:?}, which is a binary that wants an argument this harness does not pass"
    )]
    Silent {
        /// Which binary.
        binary: String,
    },
    /// What it printed could not be read back.
    #[error("cannot read {capture}, which is where the version output went: {why}")]
    NoCapture {
        /// Where the output was meant to be.
        capture: String,
        /// Why it could not be read.
        why: String,
    },
    /// It exited quietly.
    #[error(
        "{binary} printed nothing for --version, so there is no version to record against its numbers"
    )]
    NothingPrinted {
        /// Which binary.
        binary: String,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{first_line, strip_colour};

    #[test]
    fn a_plain_banner_comes_back_as_it_was_printed() {
        assert_eq!(
            first_line("valkey-server v=8.1.1 sha=00000000:0 malloc=jemalloc\n").unwrap(),
            "valkey-server v=8.1.1 sha=00000000:0 malloc=jemalloc"
        );
    }

    #[test]
    fn only_the_first_line_is_kept() {
        let text = "pogocache 1.2.0\nbuilt with clang\ncopyright somebody\n";
        assert_eq!(first_line(text).unwrap(), "pogocache 1.2.0");
    }

    #[test]
    fn a_banner_that_starts_with_a_blank_line_is_not_recorded_as_an_empty_version() {
        assert_eq!(
            first_line("\n\n  Garnet 1.0.61\n").unwrap(),
            "Garnet 1.0.61"
        );
    }

    #[test]
    fn a_binary_that_printed_nothing_has_no_version() {
        assert_eq!(first_line(""), None);
        assert_eq!(first_line("   \n\n"), None);
    }

    #[test]
    fn colour_escapes_are_dropped_and_the_words_are_not() {
        assert_eq!(strip_colour("\u{1b}[1;32myo\u{1b}[0m 0.4.0"), "yo 0.4.0");
    }

    // A version string is text, and text that looks like the start of an escape sequence but is not one belongs to the server rather than to the terminal.
    #[test]
    fn something_that_is_not_an_escape_sequence_is_left_alone() {
        assert_eq!(strip_colour("redis [1.2.3]"), "redis [1.2.3]");
        assert_eq!(strip_colour("ends here \u{1b}[1;"), "ends here \u{1b}[1;");
    }

    // The real thing, against a binary that prints a banner and exits.
    #[cfg(unix)]
    #[test]
    fn a_binary_is_asked_and_its_first_line_is_kept() {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let dir = std::env::temp_dir();
        let fake = dir.join(format!("cb-fake-version-{}", std::process::id()));
        let capture = dir.join(format!("cb-version-{}.txt", std::process::id()));
        let mut file = std::fs::File::create(&fake).expect("writes the fake server");
        file.write_all(
            b"#!/bin/sh\nprintf '\\033[32mfake-server 9.9.9\\033[0m\\nbuilt today\\n'\n",
        )
        .expect("writes");
        drop(file);
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let said = super::version(&fake, &capture).expect("answers");
        assert_eq!(said, "fake-server 9.9.9");

        std::fs::remove_file(&fake).ok();
        std::fs::remove_file(&capture).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_binary_that_is_not_there_says_so_rather_than_recording_an_empty_version() {
        let capture = std::env::temp_dir().join(format!("cb-missing-{}.txt", std::process::id()));
        let why = super::version(std::path::Path::new("/nonexistent/cache-server"), &capture)
            .unwrap_err();
        assert!(why.to_string().contains("cannot start"), "{why}");
        std::fs::remove_file(&capture).ok();
    }
}
