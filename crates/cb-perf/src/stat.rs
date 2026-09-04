//! Running `perf stat` against a server that is already up.
//!
//! Counters are taken over the server process, not over memtier, and not over the whole machine. Over memtier they would measure the load generator, which is not what any of the cycles charts claim to show. Over the machine they would fold in every other process on the box, and the number would drift with whatever else happened to be running.
//!
//! perf is attached with `-p` for the length of the measured passes and then interrupted. `SIGINT` is the signal it answers by printing its counters and leaving, and anything stronger kills it before it writes, which loses the whole capture.
//!
//! Its output goes to a file rather than to a pipe. perf writes the counters to stderr at the very end, and a file means the capture survives a harness that fell over between the interrupt and the read, which is exactly when somebody wants to look at it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cb_cache::{Running, Supervisor};
use cb_core::Perf;

use crate::csv::{self, BadPerf, event_list};

/// How long perf is given to print its counters after being interrupted.
///
/// Two seconds. It has six counters to format and nothing to flush, so this is not a budget, it is the point at which something has gone wrong enough to be worth saying so.
const GRACE: Duration = Duration::from_secs(2);

/// How `perf stat` is invoked, kept apart from the starting so it can be checked without a machine that has perf on it.
#[must_use]
pub fn argv(pid: u32) -> Vec<OsString> {
    [
        "stat".to_owned(),
        // The machine readable form. Fields are comma separated and in a fixed order, so nothing here depends on column widths.
        "-x,".to_owned(),
        "-e".to_owned(),
        event_list(),
        "-p".to_owned(),
        pid.to_string(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

/// A perf attached to a running server.
///
/// Held across the measured passes and read back at the end.
#[derive(Debug)]
pub struct Session {
    /// perf itself, in its own process group like everything else this harness starts.
    perf: Running,
    /// Where its counters are being written.
    capture: PathBuf,
    /// When it was attached, which is what `cpu_utilized` is computed against.
    started: Instant,
}

impl Session {
    /// Attach to a server that is already running.
    ///
    /// `capture` is where perf's output goes, which is a file in the run directory rather than a pipe.
    ///
    /// # Errors
    ///
    /// If perf could not be started at all, which is nearly always that it is not installed.
    pub fn attach(binary: &Path, pid: u32, capture: &Path) -> Result<Self, BadPerf> {
        let perf = Supervisor::new(binary)
            .args(argv(pid))
            .log(capture)
            .start()
            .map_err(|why| BadPerf::NotStarted(why.to_string()))?;
        Ok(Self {
            perf,
            capture: capture.to_path_buf(),
            started: Instant::now(),
        })
    }

    /// The command as it was started, for the run log.
    #[must_use]
    pub fn line(binary: &Path, pid: u32) -> String {
        Supervisor::new(binary).args(argv(pid)).line()
    }

    /// Interrupt perf, wait for it to write, and read what it counted.
    ///
    /// # Errors
    ///
    /// If perf could not be signalled, if it did not write anything before the grace period ran out, or if what it wrote holds no counters.
    pub fn finish(mut self) -> Result<Perf, BadPerf> {
        let wall = self.started.elapsed().as_secs_f64();
        self.perf
            .interrupt()
            .map_err(|why| BadPerf::NotStopped(why.to_string()))?;
        let left = self
            .perf
            .settle(GRACE)
            .map_err(|why| BadPerf::NotStopped(why.to_string()))?;
        if !left {
            // Killing it here would be pointless. It has not written its counters and it is not going to.
            return Err(BadPerf::NotStopped(format!(
                "perf did not print its counters within {GRACE:?} of being interrupted"
            )));
        }
        let text = std::fs::read_to_string(&self.capture).map_err(|why| {
            BadPerf::NoCapture(self.capture.display().to_string(), why.to_string())
        })?;
        csv::read(&text, wall)
    }
}

/// So that a failed run does not leave a perf attached to a pid that is about to be reused.
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.perf.interrupt();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::path::Path;

    use super::{Session, argv};

    #[test]
    fn perf_is_attached_to_the_server_rather_than_to_the_load_generator() {
        let args: Vec<String> = argv(4271)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = args.iter().position(|a| a == "-p").expect("attaches");
        assert_eq!(args[at + 1], "4271");
    }

    #[test]
    fn the_machine_readable_form_is_asked_for() {
        let args = argv(1);
        assert!(args.iter().any(|a| a == "-x,"));
    }

    #[test]
    fn the_events_are_the_six_the_original_asks_for() {
        assert_eq!(
            Session::line(Path::new("/usr/bin/perf"), 4271),
            "/usr/bin/perf stat -x, -e cycles,instructions,branches,branch-misses,page-faults,task-clock -p 4271"
        );
    }

    #[test]
    fn a_path_with_a_space_in_it_is_quoted_for_the_log() {
        let text = Session::line(Path::new("/opt/linux tools/perf"), 9);
        assert!(text.starts_with("'/opt/linux tools/perf' stat"), "{text}");
    }

    // The whole sequence, against something that behaves the way perf does: it sits there until it is interrupted, and only then does it print what it counted.
    #[cfg(unix)]
    #[test]
    fn a_session_interrupts_and_reads_back_what_was_counted() {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;
        use std::time::Duration;

        let dir = std::env::temp_dir();
        let fake = dir.join(format!("cb-fake-perf-{}", std::process::id()));
        let capture = dir.join(format!("cb-capture-{}.csv", std::process::id()));
        let script = "#!/bin/sh\n\
             trap 'echo \"642245372237,,cycles,60002144000,100.00,,\"; echo \"59998.63,msec,task-clock,59998634000,100.00,3.000,CPUs utilized\"; exit 0' INT\n\
             echo attached\n\
             while true; do sleep 0.05; done\n";
        let mut file = std::fs::File::create(&fake).expect("writes the fake perf");
        file.write_all(script.as_bytes()).expect("writes");
        drop(file);
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let session = Session::attach(&fake, 4271, &capture).expect("attaches");
        // Interrupting before the trap is installed would kill it the ordinary way, and this would then be a test about how fast the shell got going.
        for _ in 0..500 {
            if std::fs::read_to_string(&capture).is_ok_and(|text| text.contains("attached")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let perf = session.finish().expect("reads the capture");
        assert_eq!(
            perf.cycles.expect("counted cycles"),
            cb_core::Counter::Text("642245372237".to_owned())
        );
        assert!(perf.cpu_utilized.is_some());

        std::fs::remove_file(&fake).ok();
        std::fs::remove_file(&capture).ok();
    }
}
