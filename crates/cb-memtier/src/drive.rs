//! Running memtier and reading what it measured.
//!
//! Three invocations make up a run: a warmup SET pass, a measured SET pass and a measured GET pass. All three go through here, and the only difference between them is the ratio in the argv and whether the caller keeps the answer.
//!
//! memtier is pinned to the load generator half of the cores, the half the server was kept off. It writes its own JSON to a file this hands it, and that file is the result. The text memtier prints while it works goes to a log next to the run, because it is where a refused connection says so, and it is the first thing worth reading when a pass came back wrong.

use std::path::Path;
use std::time::{Duration, Instant};

use cb_cache::{BadProcess, Supervisor};
use cb_core::{CpuSet, Op};

use crate::argv::{Invocation, Pass};
use crate::parse::{self, BadOutput};

/// What one pass produced.
#[derive(Debug)]
pub struct Load {
    /// The numbers, already checked.
    pub op: Op,
    /// How long the pass took from start to exit, which is what perf's counters are divided against.
    pub took: Duration,
    /// The command line, for the run record.
    pub line: String,
}

/// Run one pass and read its result.
///
/// `on` is the load generator's half of the machine. It is optional only so the tests can run where there is no affinity call at all, and a real run always pins, because a load generator sharing cores with the server measures the two of them fighting.
///
/// `patience` is how long the pass may take. It is a ceiling on a hang rather than an expectation: a pass that has not finished by then is one where memtier is waiting on a server that stopped answering, and every later run in the sweep is behind it.
///
/// The warmup is checked exactly like the other two. Its numbers are thrown away, but a warmup that did not run means the measured SET pass is partly a measurement of hash table growth, which is the one thing the warmup exists to prevent, so it is a failed run here rather than a quiet one.
///
/// # Errors
///
/// If memtier will not start, if it does not finish in time, if it wrote no result file, or if what it wrote does not pass the checks in [`parse::read`].
pub fn run(
    binary: &Path,
    invocation: &Invocation<'_>,
    on: Option<&CpuSet>,
    log: &Path,
    patience: Duration,
) -> Result<Load, NotMeasured> {
    let out = invocation.json_out;
    // A result file left by an earlier pass would be read as this one's if memtier died before writing, and the numbers in it are real numbers from a real run, which is exactly what makes that failure impossible to spot later.
    if let Err(why) = remove(out) {
        return Err(NotMeasured::InTheWay {
            path: out.display().to_string(),
            why: why.to_string(),
        });
    }

    let mut load = Supervisor::new(binary).args(invocation.argv()).log(log);
    if let Some(cpus) = on {
        load = load.pin(cpus);
    }
    let line = load.line();
    let started = Instant::now();
    let mut running = load.start()?;
    if !running.settle(patience)? {
        // The destructor takes the group with it, so nothing is left holding the cores.
        return Err(NotMeasured::TooLong {
            pass: invocation.pass,
            patience,
        });
    }
    let took = started.elapsed();

    let text = std::fs::read_to_string(out).map_err(|why| NotMeasured::NoOutput {
        pass: invocation.pass,
        path: out.display().to_string(),
        why: why.to_string(),
    })?;
    let op = parse::read(
        &text,
        invocation.pass,
        invocation.profile.total_operations(),
    )?;

    Ok(Load { op, took, line })
}

/// Remove a file, and say nothing about one that was not there.
fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(why) if why.kind() != std::io::ErrorKind::NotFound => Err(why),
        _ => Ok(()),
    }
}

/// Anything that stops a pass producing a number.
#[derive(Debug, thiserror::Error)]
pub enum NotMeasured {
    /// A result file from an earlier pass could not be cleared.
    #[error("cannot remove {path}, which is where this pass writes its result: {why}")]
    InTheWay {
        /// The file in question.
        path: String,
        /// Why it would not go.
        why: String,
    },
    /// memtier would not start or would not be waited on.
    #[error("{0}")]
    NotStarted(#[from] BadProcess),
    /// It ran past its ceiling, which is a server that stopped answering rather than a slow pass.
    #[error(
        "the {pass} pass was still running after {patience:?}, which is memtier waiting on a server that stopped answering"
    )]
    TooLong {
        /// Which pass.
        pass: Pass,
        /// How long it was given.
        patience: Duration,
    },
    /// It exited without writing a result.
    #[error(
        "the {pass} pass wrote no result to {path}, so memtier exited before it measured anything: {why}"
    )]
    NoOutput {
        /// Which pass.
        pass: Pass,
        /// Where the result was meant to be.
        path: String,
        /// Why it could not be read.
        why: String,
    },
    /// It wrote a result that does not hold up.
    #[error("{0}")]
    Refused(#[from] BadOutput),
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    #[cfg(unix)]
    use std::path::{Path, PathBuf};
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use cb_core::{Profile, Profiles, Protocol};

    #[cfg(unix)]
    use crate::argv::{Invocation, Pass};

    /// The profile the reference numbers were measured with, which is the one in the tree.
    #[cfg(unix)]
    fn profile() -> Profile {
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../profiles.toml"))
                .expect("reads profiles.toml");
        Profiles::parse(&text)
            .expect("parses")
            .get("reference")
            .expect("has the reference profile")
            .clone()
    }

    /// A stand in for memtier, written as a shell script so that a real process is really started.
    ///
    /// Named after the test that asked for it, because these run at the same time and a shared path is a test that measures whichever one wrote last.
    #[cfg(unix)]
    fn script(label: &str, body: &str) -> PathBuf {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let path =
            std::env::temp_dir().join(format!("cb-fake-memtier-{}-{label}", std::process::id()));
        let mut file = std::fs::File::create(&path).expect("writes the fake memtier");
        file.write_all(format!("#!/bin/sh\n{body}").as_bytes())
            .expect("writes");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// A memtier that writes the JSON a real one writes, with whatever operation count it was given.
    ///
    /// The count is text rather than a number, because it is going straight into a JSON file and turning it into a float first would only round it.
    #[cfg(unix)]
    fn fake(label: &str, ops: &str) -> PathBuf {
        // It reads the output path off its own argv rather than being told separately, which is also a check that the flag is where the argv builder says it is.
        let body = format!(
            "out=\"\"\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"--json-out-file\" ]; then out=\"$2\"; fi\n  shift\ndone\ncat > \"$out\" <<'JSON'\n{}\nJSON\necho 'the pass ran'\n",
            fake_json(ops)
        );
        script(label, &body)
    }

    /// The shape memtier writes, cut down to the fields that are read.
    #[cfg(unix)]
    fn fake_json(ops: &str) -> String {
        format!(
            r#"{{"ALL STATS": {{"Sets": {{"Ops/sec": 1234.5, "KB/sec": 2048.0, "Count": {ops}, "Latency": 0.5, "Min Latency": 0.1, "Max Latency": 9.9, "Average Latency": 0.5, "Percentile Latencies": {{"p50.00": 0.4, "p90.00": 0.8, "p99.00": 1.5, "p99.90": 3.0, "p99.99": 7.0}}}}}}}}"#
        )
    }

    #[cfg(unix)]
    fn invocation<'a>(profile: &'a Profile, socket: &'a Path, out: &'a Path) -> Invocation<'a> {
        Invocation {
            profile,
            pipeline: 10,
            pass: Pass::Sets,
            protocol: Protocol::Resp,
            socket,
            json_out: out,
        }
    }

    // The pass runs, writes its JSON where the argv said it would, and the numbers come back checked.
    #[cfg(unix)]
    #[test]
    fn a_pass_runs_and_its_result_is_read_back() {
        let profile = profile();
        let binary = fake("whole", &profile.total_operations().to_string());
        let socket = Path::new("/tmp/cb-drive.sock");
        let out = std::env::temp_dir().join(format!("cb-drive-{}.json", std::process::id()));
        let log = std::env::temp_dir().join(format!("cb-drive-{}.log", std::process::id()));

        let load = super::run(
            &binary,
            &invocation(&profile, socket, &out),
            None,
            &log,
            Duration::from_secs(30),
        )
        .expect("measures");

        assert_eq!(load.op.opsec.0.to_string(), "1234.5");
        assert!(load.line.contains("--json-out-file"), "{}", load.line);
        // Whatever memtier said while it worked is kept, because it is where a refused connection says so.
        let said = std::fs::read_to_string(&log).expect("reads the log");
        assert!(said.contains("the pass ran"), "{said}");

        std::fs::remove_file(&binary).ok();
        std::fs::remove_file(&out).ok();
        std::fs::remove_file(&log).ok();
    }

    // The failure that is impossible to spot afterwards. An old result file holds real numbers from a real run.
    #[cfg(unix)]
    #[test]
    fn a_result_file_from_an_earlier_pass_is_never_read_as_this_ones() {
        let profile = profile();
        let out = std::env::temp_dir().join(format!("cb-stale-{}.json", std::process::id()));
        std::fs::write(&out, fake_json(&profile.total_operations().to_string())).expect("writes");
        let log = std::env::temp_dir().join(format!("cb-stale-{}.log", std::process::id()));

        // A memtier that exits without writing anything, which is what a memtier that could not connect does.
        let binary = script("quiet", "exit 0\n");
        let why = super::run(
            &binary,
            &invocation(&profile, Path::new("/tmp/cb-drive.sock"), &out),
            None,
            &log,
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(why.to_string().contains("wrote no result"), "{why}");

        std::fs::remove_file(&binary).ok();
        std::fs::remove_file(&out).ok();
        std::fs::remove_file(&log).ok();
    }

    // A pass that hangs is a server that stopped answering, and the sweep behind it is what is at stake.
    #[cfg(unix)]
    #[test]
    fn a_pass_that_hangs_is_given_up_on_rather_than_waited_on_forever() {
        let profile = profile();
        let out = std::env::temp_dir().join(format!("cb-hang-{}.json", std::process::id()));
        let log = std::env::temp_dir().join(format!("cb-hang-{}.log", std::process::id()));

        let binary = script("hang", "echo waiting\nwhile true; do sleep 0.2; done\n");
        let why = super::run(
            &binary,
            &invocation(&profile, Path::new("/tmp/cb-drive.sock"), &out),
            None,
            &log,
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(why.to_string().contains("still running"), "{why}");

        std::fs::remove_file(&binary).ok();
        std::fs::remove_file(&out).ok();
        std::fs::remove_file(&log).ok();
    }

    // A result with the wrong operation count in it is a run where connections died, and its Ops/sec is a real rate over a workload nobody asked for.
    #[cfg(unix)]
    #[test]
    fn a_pass_that_completed_the_wrong_number_of_operations_is_refused() {
        let profile = profile();
        // A third of what was asked for, which is what a run that lost most of its connections looks like.
        let binary = fake("short", "8533333");
        let out = std::env::temp_dir().join(format!("cb-short-{}.json", std::process::id()));
        let log = std::env::temp_dir().join(format!("cb-short-{}.log", std::process::id()));

        let why = super::run(
            &binary,
            &invocation(&profile, Path::new("/tmp/cb-drive.sock"), &out),
            None,
            &log,
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(why.to_string().contains("operations"), "{why}");

        std::fs::remove_file(&binary).ok();
        std::fs::remove_file(&out).ok();
        std::fs::remove_file(&log).ok();
    }
}
