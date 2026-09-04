//! One cache server, from started to stopped.
//!
//! The argv tables live in `cb-core` because they are data, the process handling lives in `supervise` because it is the only place in this tree that needs unsafe, and this is the thin piece that puts the two together. It is the same six steps for all seven servers, which is the point: an engine that got special handling here would be an engine measured differently from the rest, and the fairness rules say a flag added for one has to be considered for the other six or not added at all.
//!
//! Clear the socket, start the binary pinned to the cache half of the cores with its output going to a file, wait for it to answer, hand it over. Later, stop the group, confirm nothing survived, and take the socket away again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cb_core::{CacheKind, CpuSet, Endpoint, Launch};

use crate::ready::{NotReady, wait};
use crate::supervise::{BadProcess, Running, Stopped, Supervisor};

/// A server that is up and answering.
#[derive(Debug)]
pub struct Server {
    /// Which of the seven.
    kind: CacheKind,
    /// The process, and its group.
    running: Running,
    /// The command line, kept for the run record.
    line: String,
    /// How long it took to answer, which goes in the run record too.
    ready: Duration,
    /// The socket to take away afterwards, when it is listening on one.
    socket: Option<PathBuf>,
}

impl Server {
    /// Start it and wait until it answers.
    ///
    /// `on` is the half of the machine the server gets. It is optional only so that the tests can run on a machine with no affinity call at all, and a real run always pins, because a server and a load generator sharing cores measures the two of them fighting.
    ///
    /// # Errors
    ///
    /// If a stale socket is in the way and cannot be removed, if the binary will not start, or if it never answers.
    pub fn start(
        kind: CacheKind,
        launch: &Launch<'_>,
        on: Option<&CpuSet>,
        log: &Path,
        patience: Duration,
    ) -> Result<Self, NotRunning> {
        let socket = match launch.endpoint {
            Endpoint::Unix(path) => {
                clear(path)?;
                Some(path.to_path_buf())
            }
            Endpoint::Tcp(_) => None,
        };

        // The adapter builds the whole command line including the binary, and the supervisor takes the two apart, so the first word is dropped here rather than being built twice.
        let mut argv = kind.argv(launch);
        let program = if argv.is_empty() {
            launch.binary.to_path_buf()
        } else {
            PathBuf::from(argv.remove(0))
        };

        let mut starting = Supervisor::new(program).args(argv).log(log);
        if let Some(cpus) = on {
            starting = starting.pin(cpus);
        }
        let line = starting.line();
        let mut running = starting.start()?;
        let ready = wait(kind, launch.endpoint, &mut running, patience)?;

        Ok(Self {
            kind,
            running,
            line,
            ready,
            socket,
        })
    }

    /// Which server this is.
    #[must_use]
    pub const fn kind(&self) -> CacheKind {
        self.kind
    }

    /// Its pid, which is what perf attaches to.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.running.pid()
    }

    /// The command line it was started with, for the run record.
    ///
    /// The pin is not in it. It is not a wrapper command here, so it is recorded on its own by whoever writes the run.
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }

    /// How long it took to answer.
    ///
    /// Worth recording. A server that suddenly takes ten times longer to come up than it did yesterday is the kind of thing that explains a run that looks wrong.
    #[must_use]
    pub const fn ready(&self) -> Duration {
        self.ready
    }

    /// Whether it is still up.
    ///
    /// Checked after the measured passes, because a server that died halfway through produced numbers for half a run and those are not a measurement.
    ///
    /// # Errors
    ///
    /// If the process could not be checked on at all.
    pub fn alive(&mut self) -> Result<bool, BadProcess> {
        self.running.alive()
    }

    /// Stop it, confirm the group is gone, and take the socket away.
    ///
    /// # Errors
    ///
    /// If the group could not be signalled, or if something in it outlived the kill, which is the stray that would otherwise sit on the cores the next run is measured on.
    pub fn stop(mut self, grace: Duration) -> Result<Stopped, BadProcess> {
        let how = self.running.stop(grace)?;
        // Only after the group is gone. A socket removed while the server still holds it leaves the file descriptor open and the next server binding a path nothing is listening on.
        if let Some(path) = &self.socket {
            let _ = std::fs::remove_file(path);
        }
        Ok(how)
    }
}

/// Take a leftover socket out of the way.
///
/// A run whose server was killed leaves its socket file behind, and several of these servers refuse to bind a path that already exists. Left alone it turns into a failure hundreds of runs into a sweep, in a server that is fine, for a reason that has nothing to do with the server.
///
/// Only a socket is removed. Anything else at that path is somebody's file and a benchmark has no business deleting it, so it is reported instead.
fn clear(path: &Path) -> Result<(), NotRunning> {
    let Ok(found) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !is_socket(&found) {
        return Err(NotRunning::InTheWay {
            path: path.display().to_string(),
        });
    }
    std::fs::remove_file(path).map_err(|why| NotRunning::Socket {
        path: path.display().to_string(),
        why: why.to_string(),
    })
}

/// Whether what is at the socket path is in fact a socket.
#[cfg(unix)]
fn is_socket(found: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt as _;
    found.file_type().is_socket()
}

/// Elsewhere there is no way to ask, and nothing is removed.
#[cfg(not(unix))]
const fn is_socket(_found: &std::fs::Metadata) -> bool {
    false
}

/// A server that did not get as far as answering.
#[derive(Debug, thiserror::Error)]
pub enum NotRunning {
    /// Something that is not a socket is sitting on the socket path.
    #[error(
        "{path} already exists and is not a socket, so it is somebody's file rather than a leftover from a run"
    )]
    InTheWay {
        /// The path in question.
        path: String,
    },
    /// A leftover socket could not be removed.
    #[error("cannot remove the leftover socket at {path}: {why}")]
    Socket {
        /// The path in question.
        path: String,
        /// Why it would not go.
        why: String,
    },
    /// The binary would not start.
    #[error("{0}")]
    NotStarted(#[from] BadProcess),
    /// It started and never answered.
    #[error("{0}")]
    NotReady(#[from] NotReady),
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use cb_core::{CacheKind, Compat, Endpoint, Launch, size::Bytes};

    #[cfg(unix)]
    fn launch<'a>(binary: &'a Path, socket: &'a Path) -> Launch<'a> {
        Launch {
            binary,
            threads: 2,
            maxmemory: Bytes(32 * 1024 * 1024 * 1024),
            endpoint: Endpoint::Unix(socket),
            compat: Compat::Corrected,
            as_root: false,
        }
    }

    /// Whether there is a python3 to write a fake server in.
    ///
    /// A unix socket listener cannot be written in the shell, and the one test that needs a real one needs a real process rather than a thread, because what it is checking is that something was started, waited for over the socket and stopped as a group. Every host this harness runs on has python3, and on a machine that does not the test says why it did nothing rather than failing over a missing interpreter.
    #[cfg(unix)]
    fn have_python() -> bool {
        std::process::Command::new("python3")
            .args(["-c", "pass"])
            .output()
            .is_ok_and(|out| out.status.success())
    }

    /// A server that answers `PING` on a unix socket, which is all this layer asks of one.
    ///
    /// The adapter's flags land in `sys.argv` and are ignored, the same way a real server ignores nothing and this fake ignores everything. What matters is that it binds the socket it was told about.
    #[cfg(unix)]
    fn fake(socket: &Path) -> std::path::PathBuf {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;

        let path = std::env::temp_dir().join(format!("cb-fake-cache-{}.py", std::process::id()));
        let script = format!(
            "#!/usr/bin/env python3\nimport socket\ns = socket.socket(socket.AF_UNIX)\ns.bind({:?})\ns.listen(8)\nwhile True:\n    c, _ = s.accept()\n    c.recv(64)\n    c.sendall(b'+PONG\\r\\n')\n    c.close()\n",
            socket.display().to_string()
        );
        let mut file = std::fs::File::create(&path).expect("writes the fake server");
        file.write_all(script.as_bytes()).expect("writes");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    // The whole sequence: started with the adapter's argv, waited for over the protocol, stopped as a group, socket taken away.
    #[cfg(unix)]
    #[test]
    fn a_server_is_started_waited_for_and_stopped() {
        if !have_python() {
            eprintln!("no python3 here, so there is nothing to bind a socket with");
            return;
        }
        let socket = std::env::temp_dir().join(format!("cb-server-{}.sock", std::process::id()));
        std::fs::remove_file(&socket).ok();
        let binary = fake(&socket);
        let log = std::env::temp_dir().join(format!("cb-server-{}.log", std::process::id()));

        let mut server = super::Server::start(
            CacheKind::Valkey,
            &launch(&binary, &socket),
            None,
            &log,
            Duration::from_secs(20),
        )
        .expect("starts and answers");

        assert_eq!(server.kind(), CacheKind::Valkey);
        assert!(server.pid() > 0);
        assert!(server.alive().expect("checks"));
        // The recorded line is the adapter's, with the binary in front of it and no pin wrapped around it.
        assert!(
            server.line().contains("--appendonly no"),
            "{}",
            server.line()
        );
        assert!(server.line().starts_with(&binary.display().to_string()));

        server.stop(Duration::from_secs(5)).expect("stops");
        assert!(!socket.exists(), "the socket was left behind");

        std::fs::remove_file(&binary).ok();
        std::fs::remove_file(&log).ok();
    }

    // The failure this exists to prevent, which shows up hundreds of runs into a sweep and looks like the wrong server's fault.
    #[cfg(unix)]
    #[test]
    fn a_leftover_socket_is_cleared_rather_than_left_to_fail_the_next_bind() {
        let socket = std::env::temp_dir().join(format!("cb-stale-{}.sock", std::process::id()));
        std::fs::remove_file(&socket).ok();
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("binds");
        drop(listener);
        assert!(socket.exists());

        super::clear(&socket).expect("clears it");
        assert!(!socket.exists());
    }

    // Somebody's file is not a leftover, and a benchmark deleting one would be a bug worth remembering.
    #[cfg(unix)]
    #[test]
    fn a_file_that_is_not_a_socket_is_reported_rather_than_deleted() {
        let path = std::env::temp_dir().join(format!("cb-notasocket-{}", std::process::id()));
        std::fs::write(&path, b"somebody's file").expect("writes");

        let why = super::clear(&path).unwrap_err();
        assert!(why.to_string().contains("is not a socket"), "{why}");
        assert!(path.exists(), "the file was deleted");

        std::fs::remove_file(&path).ok();
    }

    // A binary that is not there has to say so, because a sweep that reports this as a timeout sends somebody looking at the server instead of at config.jsonc.
    #[cfg(unix)]
    #[test]
    fn a_binary_that_is_not_there_fails_to_start_rather_than_failing_to_answer() {
        let socket = std::env::temp_dir().join(format!("cb-nobinary-{}.sock", std::process::id()));
        let log = std::env::temp_dir().join(format!("cb-nobinary-{}.log", std::process::id()));
        let binary = Path::new("/nonexistent/valkey-server");

        let why = super::Server::start(
            CacheKind::Valkey,
            &launch(binary, &socket),
            None,
            &log,
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(why.to_string().contains("cannot start"), "{why}");

        std::fs::remove_file(&log).ok();
    }
}
