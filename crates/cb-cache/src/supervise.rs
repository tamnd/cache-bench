//! Starting a server, pinning it, and making sure it is gone afterwards.
//!
//! Everything in this tree that starts a process goes through here, and clippy is configured to fail the build on a bare `Command::spawn` anywhere else. That is not tidiness. A benchmark that leaves a server running has not failed in a way anybody notices; the next run starts its own server, the two of them share the cores, and the numbers come out low and plausible.
//!
//! Three things are done that a plain spawn does not do.
//!
//! The child is put in its own process group before exec, so that stopping it stops everything it started. Several of these servers fork workers, and signalling the pid alone leaves those workers holding the socket.
//!
//! The CPU pin is applied before exec as well, rather than by wrapping the command in `taskset`. Wrapping means the pid that comes back is taskset's, which is the pid perf would then be attached to and the pid the stray check would then be looking for. Setting the affinity in the child means the pid is the server's.
//!
//! And stopping is checked rather than requested. `SIGTERM` goes to the group, and if the group is still there when the grace period runs out it gets `SIGKILL`, and either way the group is confirmed gone before the run is called finished.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cb_core::CpuSet;

/// How often the group is checked while waiting for it to go away.
///
/// Twenty milliseconds. Long enough that a server which stops immediately is not polled a thousand times, short enough that it does not add up over a sweep of several hundred runs.
const TICK: Duration = Duration::from_millis(20);

/// How long to keep checking that a group is gone after the last process in it was reaped.
///
/// A process that has been signalled is not necessarily gone the instant `wait` returns, because the group can still hold something that was reparented, and a stray check that runs too early reports a stray that is on its way out.
const SETTLE: Duration = Duration::from_millis(500);

/// A server about to be started.
#[derive(Debug, Clone)]
pub struct Supervisor {
    /// What to run.
    program: PathBuf,
    /// Its arguments, already built by the adapter.
    args: Vec<OsString>,
    /// Which CPUs it may run on, if it is pinned.
    pin: Option<CpuSet>,
    /// Where its output goes, which is a file per run rather than this process's stderr.
    log: Option<PathBuf>,
}

impl Supervisor {
    /// Name the program.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            pin: None,
            log: None,
        }
    }

    /// Give it its arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Pin it to a set of CPUs.
    #[must_use]
    pub fn pin(mut self, cpus: &CpuSet) -> Self {
        self.pin = Some(cpus.clone());
        self
    }

    /// Send its output to a file.
    ///
    /// Both streams go to the same file, because what these servers write is a startup banner and the occasional warning, and the order the two streams interleave in is the useful part when one of them refuses to start.
    #[must_use]
    pub fn log(mut self, path: impl Into<PathBuf>) -> Self {
        self.log = Some(path.into());
        self
    }

    /// The command as it would be typed, for the run log.
    ///
    /// The pin is not in it, because the pin is not a wrapper command here. It is written alongside instead, by whoever records the run.
    #[must_use]
    pub fn line(&self) -> String {
        let mut out = quote(self.program.as_os_str());
        for arg in &self.args {
            out.push(' ');
            out.push_str(&quote(arg));
        }
        out
    }

    /// Start it.
    ///
    /// # Errors
    ///
    /// If the log file cannot be created, if the pin asks for a CPU this platform cannot express, or if the program will not start.
    pub fn start(&self) -> Result<Running, BadProcess> {
        let mut command = Command::new(&self.program);
        command.args(&self.args).stdin(Stdio::null());
        match &self.log {
            Some(path) => {
                let file = File::create(path).map_err(|why| {
                    BadProcess::NoLog(path.display().to_string(), why.to_string())
                })?;
                let other = file.try_clone().map_err(|why| {
                    BadProcess::NoLog(path.display().to_string(), why.to_string())
                })?;
                command.stdout(Stdio::from(file)).stderr(Stdio::from(other));
            }
            None => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        platform::prepare(&mut command, self.pin.as_ref())?;
        // This is the one spawn in the tree, and clippy is configured to fail the build on any other.
        #[allow(
            clippy::disallowed_methods,
            reason = "this is the supervisor the rest of the tree is required to go through"
        )]
        let child = command.spawn().map_err(|why| {
            BadProcess::NotStarted(self.program.display().to_string(), why.to_string())
        })?;
        let pid = child.id();
        Ok(Running {
            child: Some(child),
            pid,
        })
    }
}

/// Wrap an argument in single quotes if it holds anything a shell would act on.
fn quote(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:,=".contains(c))
    {
        return text.into_owned();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A started server, and the group it leads.
///
/// The pid is also the process group id, because the child called `setpgid(0, 0)` before exec.
#[derive(Debug)]
pub struct Running {
    /// `None` once it has been reaped.
    child: Option<std::process::Child>,
    /// Kept separately, because the handle is gone after the wait and the stray check still needs the number.
    pid: u32,
}

/// How a server went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// It stopped when it was asked to, which is what should happen.
    Asked,
    /// It had to be killed, which is worth recording next to the run.
    Killed,
}

impl Running {
    /// The server's pid, which is what perf attaches to.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether it is still up.
    ///
    /// A server that has exited before the run finished is a failed run rather than a slow one, and this is what notices.
    ///
    /// # Errors
    ///
    /// If the child could not be checked on at all.
    pub fn alive(&mut self) -> Result<bool, BadProcess> {
        let Some(child) = self.child.as_mut() else {
            return Ok(false);
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.child = None;
                Ok(false)
            }
            Ok(None) => Ok(true),
            Err(why) => Err(BadProcess::NotStopped(why.to_string())),
        }
    }

    /// Ask the whole group to stop, kill it if it will not, and confirm it is gone.
    ///
    /// # Errors
    ///
    /// If the group could not be signalled, if the child could not be waited on, or if something in the group is still there after the kill, which is the stray this whole module exists to catch.
    pub fn stop(&mut self, grace: Duration) -> Result<Stopped, BadProcess> {
        if self.child.is_none() {
            return Ok(Stopped::Asked);
        }
        platform::term(self.pid)?;
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if !self.alive()? {
                return self.settled(Stopped::Asked);
            }
            std::thread::sleep(TICK);
        }
        platform::kill(self.pid)?;
        if let Some(child) = self.child.as_mut() {
            child
                .wait()
                .map_err(|why| BadProcess::NotStopped(why.to_string()))?;
            self.child = None;
        }
        self.settled(Stopped::Killed)
    }

    /// Interrupt it, the way pressing control C would.
    ///
    /// This exists for perf, which handles `SIGINT` by printing what it counted and leaving. Anything stronger kills it before it writes and the whole capture is lost, so a perf is never stopped the ordinary way.
    ///
    /// # Errors
    ///
    /// If the group could not be signalled at all. A group that has already gone is not an error, because that is the state being asked for.
    pub fn interrupt(&self) -> Result<(), BadProcess> {
        platform::interrupt(self.pid)
    }

    /// Wait for it to exit on its own.
    ///
    /// Returns whether it did. A false here is a process that ignored whatever it was asked to do, and the caller is the one that decides whether that is worth killing over.
    ///
    /// # Errors
    ///
    /// If the child could not be waited on.
    pub fn settle(&mut self, patience: Duration) -> Result<bool, BadProcess> {
        let deadline = Instant::now() + patience;
        loop {
            if !self.alive()? {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(TICK);
        }
    }

    /// Wait for the group to disappear, and complain if it does not.
    fn settled(&self, how: Stopped) -> Result<Stopped, BadProcess> {
        let deadline = Instant::now() + SETTLE;
        loop {
            if !platform::group_exists(self.pid) {
                return Ok(how);
            }
            if Instant::now() >= deadline {
                return Err(BadProcess::Stray(self.pid));
            }
            std::thread::sleep(TICK);
        }
    }
}

impl Drop for Running {
    /// Kill anything still running, because a panic partway through a run must not leave a server holding the cores.
    ///
    /// Nothing here can report a failure, so nothing here tries. The stray check on the next start is what catches a group this could not clear.
    fn drop(&mut self) {
        if self.child.is_none() {
            return;
        }
        let _ = platform::kill(self.pid);
        if let Some(child) = self.child.as_mut() {
            let _ = child.wait();
        }
    }
}

/// Anything that stops a server being started or stopped.
#[derive(Debug, thiserror::Error)]
pub enum BadProcess {
    /// The log file could not be created, which is usually a results directory that is not there yet.
    #[error("cannot open {0} to log the server's output: {1}")]
    NoLog(String, String),
    /// The program would not start, which is usually that it is not installed.
    #[error("cannot start {0}: {1}")]
    NotStarted(String, String),
    /// The child could not be signalled or waited on.
    #[error("cannot stop the server: {0}")]
    NotStopped(String),
    /// Something in the group outlived the kill.
    #[error(
        "process group {0} is still there after being killed, so something from this run is still holding the cores and the next run would be measured against it"
    )]
    Stray(u32),
    /// A CPU number this platform cannot put in an affinity mask.
    #[error("cpu {0} is past the largest one an affinity mask can hold")]
    NoSuchCpu(u32),
    /// Pinning was asked for on a platform that has no way to do it.
    #[error(
        "this platform cannot pin a process to a set of CPUs, and a run that is not pinned is not comparable to one that is"
    )]
    NoPinning,
    /// Everything here needs a Unix.
    #[error("starting servers needs a Unix, and this is not one")]
    Unsupported,
}

#[cfg(unix)]
mod platform {
    //! The Unix half. Process groups everywhere, pinning on Linux only.

    use std::io;
    use std::os::unix::process::CommandExt as _;
    use std::process::Command;

    use cb_core::CpuSet;

    use super::BadProcess;

    /// Put the child in its own process group, and pin it, both before exec.
    ///
    /// The closure runs between fork and exec, where the only safe thing to do is call async signal safe functions. Both of these are, and the mask is built out here beforehand so that nothing in the closure allocates.
    pub(super) fn prepare(command: &mut Command, pin: Option<&CpuSet>) -> Result<(), BadProcess> {
        let mask = mask(pin)?;
        // SAFETY: the closure calls setpgid and sched_setaffinity and nothing else. Both are async signal safe, so both are allowed between fork and exec, and neither allocates, takes a lock or touches anything this process owns.
        #[allow(
            unsafe_code,
            reason = "there is no safe way to run code between fork and exec, and the pin has to be applied to the server itself rather than to a taskset wrapper whose pid is the one perf would attach to"
        )]
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                apply(mask.as_ref())
            });
        }
        Ok(())
    }

    /// Ask the group to stop.
    pub(super) fn term(pgid: u32) -> Result<(), BadProcess> {
        signal(pgid, libc::SIGTERM)
    }

    /// Interrupt the group, which is what perf wants.
    pub(super) fn interrupt(pgid: u32) -> Result<(), BadProcess> {
        signal(pgid, libc::SIGINT)
    }

    /// Make the group stop.
    pub(super) fn kill(pgid: u32) -> Result<(), BadProcess> {
        signal(pgid, libc::SIGKILL)
    }

    /// Whether anything is still in the group.
    ///
    /// Signal zero delivers nothing and reports whether there would have been somebody to deliver it to.
    pub(super) fn group_exists(pgid: u32) -> bool {
        let Ok(pgid) = i32::try_from(pgid) else {
            return false;
        };
        // SAFETY: kill with signal zero delivers no signal. It only reports whether the group exists and whether we may signal it.
        #[allow(
            unsafe_code,
            reason = "the standard library cannot ask whether a process group still exists, and that question is the stray check"
        )]
        let rc = unsafe { libc::kill(-pgid, 0) };
        rc == 0
    }

    /// Send one signal to a whole group.
    ///
    /// A group that has already gone is not an error here. It is the thing being asked for.
    fn signal(pgid: u32, sig: i32) -> Result<(), BadProcess> {
        let pgid = i32::try_from(pgid)
            .map_err(|_| BadProcess::NotStopped("the pid does not fit in a pid_t".to_owned()))?;
        // SAFETY: a negative pid means the process group, which is the whole point, and signalling touches nothing in this process.
        #[allow(
            unsafe_code,
            reason = "the standard library can only signal one child, and a server that forked workers needs the whole group"
        )]
        let rc = unsafe { libc::kill(-pgid, sig) };
        if rc == 0 {
            return Ok(());
        }
        let why = io::Error::last_os_error();
        if why.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(BadProcess::NotStopped(why.to_string()))
    }

    #[cfg(target_os = "linux")]
    pub(super) use linux::{apply, mask};

    #[cfg(target_os = "linux")]
    mod linux {
        use std::io;

        use cb_core::CpuSet;

        use super::BadProcess;

        /// An affinity mask, built before the fork so the `pre_exec` closure does not have to.
        pub(crate) type Mask = libc::cpu_set_t;

        /// Turn a CPU set into a mask.
        pub(crate) fn mask(pin: Option<&CpuSet>) -> Result<Option<Mask>, BadProcess> {
            let Some(pin) = pin else { return Ok(None) };
            // SAFETY: cpu_set_t is a plain array of integers with no padding that matters and no invariant to uphold, and all zeroes is the empty set, which is what CPU_ZERO writes.
            #[allow(
                unsafe_code,
                reason = "cpu_set_t has no constructor in libc and all zeroes is its empty value"
            )]
            let mut set: Mask = unsafe { std::mem::zeroed() };
            for cpu in pin.cpus() {
                let slot = usize::try_from(cpu).map_err(|_| BadProcess::NoSuchCpu(cpu))?;
                if slot >= libc::CPU_SETSIZE as usize {
                    return Err(BadProcess::NoSuchCpu(cpu));
                }
                // SAFETY: the slot has been checked against CPU_SETSIZE, which is the bound CPU_SET has.
                #[allow(unsafe_code, reason = "CPU_SET is a macro that libc exposes as unsafe")]
                unsafe {
                    libc::CPU_SET(slot, &mut set);
                }
            }
            Ok(Some(set))
        }

        /// Apply it to the calling thread, which between fork and exec is the child.
        pub(crate) fn apply(mask: Option<&Mask>) -> io::Result<()> {
            let Some(mask) = mask else { return Ok(()) };
            // SAFETY: pid zero is the calling thread, the size is the size of the type being passed, and the pointer is to a mask that outlives the call.
            #[allow(
                unsafe_code,
                reason = "sched_setaffinity is the syscall, and doing this any other way means wrapping the server in taskset"
            )]
            let rc =
                unsafe { libc::sched_setaffinity(0, size_of::<Mask>(), std::ptr::from_ref(mask)) };
            if rc == 0 {
                return Ok(());
            }
            Err(io::Error::last_os_error())
        }
    }

    /// A Unix that is not Linux has process groups and no `sched_setaffinity`.
    ///
    /// Asking for a pin here is refused rather than ignored, because a run that was meant to be pinned and quietly was not is a measurement of something else.
    #[cfg(not(target_os = "linux"))]
    pub(super) fn mask(pin: Option<&CpuSet>) -> Result<Option<()>, BadProcess> {
        if pin.is_some() {
            return Err(BadProcess::NoPinning);
        }
        Ok(None)
    }

    /// Nothing to apply, because nothing was accepted.
    ///
    /// The result is what `pre_exec` takes, so it stays even though this can never fail.
    #[cfg(not(target_os = "linux"))]
    #[allow(
        clippy::unnecessary_wraps,
        reason = "the signature is the one pre_exec requires, and the Linux version does fail"
    )]
    pub(super) fn apply(_mask: Option<&()>) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(not(unix))]
mod platform {
    //! Windows, where none of this is possible and none of it is needed, because the sweep runs on Linux.

    use std::process::Command;

    use cb_core::CpuSet;

    use super::BadProcess;

    pub(super) fn prepare(_command: &mut Command, _pin: Option<&CpuSet>) -> Result<(), BadProcess> {
        Err(BadProcess::Unsupported)
    }

    pub(super) fn term(_pgid: u32) -> Result<(), BadProcess> {
        Err(BadProcess::Unsupported)
    }

    pub(super) fn interrupt(_pgid: u32) -> Result<(), BadProcess> {
        Err(BadProcess::Unsupported)
    }

    pub(super) fn kill(_pgid: u32) -> Result<(), BadProcess> {
        Err(BadProcess::Unsupported)
    }

    pub(super) const fn group_exists(_pgid: u32) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::time::Duration;

    use super::Supervisor;

    #[test]
    fn the_command_line_is_written_the_way_it_would_be_typed() {
        let line = Supervisor::new("/usr/bin/valkey-server")
            .args(["--io-threads", "8", "--save", ""])
            .line();
        assert_eq!(line, "/usr/bin/valkey-server --io-threads 8 --save ''");
    }

    #[test]
    fn a_path_with_a_space_in_it_is_quoted() {
        let line = Supervisor::new("/opt/cache servers/redis-server").line();
        assert_eq!(line, "'/opt/cache servers/redis-server'");
    }

    #[cfg(unix)]
    #[test]
    fn a_server_starts_stops_when_asked_and_leaves_nothing_behind() {
        let mut running = Supervisor::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .start()
            .expect("starts");
        assert!(running.alive().unwrap());
        assert_eq!(
            running.stop(Duration::from_secs(5)).unwrap(),
            super::Stopped::Asked
        );
        assert!(!running.alive().unwrap());
    }

    // The reason the group exists. Several of these servers fork workers, and signalling the pid alone leaves them holding the socket.
    #[cfg(unix)]
    #[test]
    fn stopping_takes_the_children_with_it() {
        let mut running = Supervisor::new("/bin/sh")
            .args(["-c", "sleep 30 & sleep 30"])
            .start()
            .expect("starts");
        // The stop only returns once the whole group is confirmed gone, so the grandchild is covered by this succeeding.
        running.stop(Duration::from_secs(5)).expect("stops cleanly");
    }

    // A server that ignores SIGTERM is not a hypothetical. Stopping still has to be something that finishes.
    #[cfg(unix)]
    #[test]
    fn a_server_that_will_not_stop_is_killed_rather_than_waited_on_forever() {
        let log = scratch("deaf");
        let mut running = Supervisor::new("/bin/sh")
            // The trap covers the shell, and the loop restarts the sleep that the group signal does kill, so the group as a whole outlives the SIGTERM.
            .args([
                "-c",
                "trap '' TERM; echo up; while true; do sleep 0.2; done",
            ])
            .log(&log)
            .start()
            .expect("starts");
        // Signalling before the trap is installed would kill it the ordinary way, and the test would pass or fail on how quickly the shell got going.
        wait_for(&log, "up");
        assert_eq!(
            running.stop(Duration::from_millis(200)).unwrap(),
            super::Stopped::Killed
        );
        std::fs::remove_file(&log).ok();
    }

    /// A scratch path for a test that needs the server to say something before it is signalled.
    #[cfg(unix)]
    fn scratch(what: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cb-supervise-{}-{what}.log", std::process::id()))
    }

    /// Wait until the server has written the word that means it is up.
    #[cfg(unix)]
    fn wait_for(log: &Path, word: &str) {
        for _ in 0..500 {
            if std::fs::read_to_string(log).is_ok_and(|text| text.contains(word)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the test server never wrote {word:?} to {}", log.display());
    }

    // perf is the reason this exists. It answers SIGINT by printing its counters and leaving, and anything stronger loses the capture.
    #[cfg(unix)]
    #[test]
    fn an_interrupt_is_handled_like_control_c_rather_than_like_a_kill() {
        let log = scratch("interrupt");
        let mut running = Supervisor::new("/bin/sh")
            .args([
                "-c",
                "trap 'echo counters; exit 0' INT; echo up; while true; do sleep 0.05; done",
            ])
            .log(&log)
            .start()
            .expect("starts");
        wait_for(&log, "up");
        running.interrupt().expect("signals");
        assert!(running.settle(Duration::from_secs(5)).expect("waits"));
        let text = std::fs::read_to_string(&log).expect("logged");
        // The handler ran, which is what perf's does, so what it wrote on the way out is there to read.
        assert!(text.contains("counters"), "{text}");
        std::fs::remove_file(&log).ok();
    }

    // The whole reason spawning goes through here rather than through Command.
    #[cfg(unix)]
    #[test]
    fn dropping_a_run_partway_through_does_not_leave_a_server_up() {
        let running = Supervisor::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .start()
            .expect("starts");
        let pid = running.pid();
        drop(running);
        assert!(!super::platform::group_exists(pid));
    }

    #[cfg(unix)]
    #[test]
    fn the_servers_output_goes_to_its_own_file() {
        let log = scratch("output");
        let mut running = Supervisor::new("/bin/sh")
            .args(["-c", "echo listening; echo warning >&2"])
            .log(&log)
            .start()
            .expect("starts");
        // Waited for rather than stopped, because stopping a process that has not reached its first write yet would kill it before it wrote anything, and the test would then be about the race rather than about the log.
        while running.alive().unwrap() {
            std::thread::sleep(Duration::from_millis(10));
        }
        let text = std::fs::read_to_string(&log).expect("logged");
        assert!(text.contains("listening"), "{text}");
        assert!(text.contains("warning"), "{text}");
        std::fs::remove_file(&log).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_is_not_there_says_which_one() {
        let why = Supervisor::new(Path::new("/nowhere/valkey-server"))
            .start()
            .unwrap_err();
        assert!(why.to_string().contains("/nowhere/valkey-server"), "{why}");
    }
}
