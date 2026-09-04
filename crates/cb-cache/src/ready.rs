//! Waiting for a server to actually answer.
//!
//! The original sleeps. A fixed sleep is either too long, which adds up over several hundred runs, or too short, which is worse: memtier connects to a server that is still growing its hash table or still faulting in the pages behind its memory limit, and the first seconds of the measured pass are a measurement of startup.
//!
//! Here the wait is a protocol round trip. A connection is opened and a command is sent, and the server is ready when it answers correctly, not when it has accepted a connection. Accepting is not the same thing: the listening socket is up well before several of these servers can serve anything, and a connect that succeeds against a server that then sits there is exactly the case a sleep cannot tell apart from a healthy one.
//!
//! The command is the smallest one each protocol has. `PING` for everything speaking RESP, and `version` for memcached, because the memcache text protocol has no ping and `version` is the only command that neither reads nor writes a key.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use cb_core::{CacheKind, Endpoint, Protocol};

use crate::supervise::Running;

/// How long to wait between attempts.
///
/// Ten milliseconds. A server that is ready is found within one tick, and a server that takes two seconds costs two hundred connects, which is nothing next to a run.
const TICK: Duration = Duration::from_millis(10);

/// How long one attempt may take before it is given up on and retried.
///
/// A connect that hangs is the case this exists for. Without it a single wedged attempt would eat the whole deadline and the error would say the server never came up rather than that it stopped answering.
const ATTEMPT: Duration = Duration::from_secs(2);

/// Wait until the server answers, or give up.
///
/// Returns how long it took, which goes in the run record. A server that suddenly takes ten times longer to come up than it did yesterday is worth seeing.
///
/// # Errors
///
/// If the server exits while being waited for, or if it never answers before the deadline.
pub fn wait(
    kind: CacheKind,
    endpoint: Endpoint<'_>,
    server: &mut Running,
    patience: Duration,
) -> Result<Duration, NotReady> {
    let started = Instant::now();
    let deadline = started + patience;
    loop {
        // A server that has already exited is never going to answer, and waiting the full deadline for it hides why the run failed behind a timeout.
        // A check that could not be made is not the same as a server that is gone, so it is left to the knock below, which answers the only question that matters anyway.
        if matches!(server.alive(), Ok(false)) {
            return Err(NotReady::Exited {
                kind,
                waited: started.elapsed(),
            });
        }
        let last = match knock(endpoint, kind.protocol()) {
            Ok(()) => return Ok(started.elapsed()),
            Err(why) => why,
        };
        if Instant::now() >= deadline {
            return Err(NotReady::Silent {
                kind,
                waited: started.elapsed(),
                last,
            });
        }
        std::thread::sleep(TICK);
    }
}

/// One attempt: connect, send the smallest command there is, and check the answer.
fn knock(endpoint: Endpoint<'_>, protocol: Protocol) -> Result<(), String> {
    let mut stream = open(endpoint)?;
    stream
        .set_read_timeout(Some(ATTEMPT))
        .and_then(|()| stream.set_write_timeout(Some(ATTEMPT)))
        .map_err(|why| why.to_string())?;
    let (ask, want) = greeting(protocol);
    stream.write_all(ask).map_err(|why| why.to_string())?;
    stream.flush().map_err(|why| why.to_string())?;

    let mut answer = [0_u8; 64];
    let read = stream.read(&mut answer).map_err(|why| why.to_string())?;
    let answer = &answer[..read];
    if answer.starts_with(want) {
        return Ok(());
    }
    // A server that answers something else is a real failure and not a slow start, but it is retried anyway, because the one way it happens in practice is a leftover server on the socket that is about to be replaced by the one we started.
    Err(format!(
        "answered {:?} where {:?} was expected",
        String::from_utf8_lossy(answer),
        String::from_utf8_lossy(want)
    ))
}

/// What to send, and what a healthy server sends back.
const fn greeting(protocol: Protocol) -> (&'static [u8], &'static [u8]) {
    match protocol {
        // The inline form, so that this does not have to build a RESP array to ask a one word question.
        Protocol::Resp => (b"PING\r\n", b"+PONG"),
        // memcached has no ping. `version` is the only command it answers that neither reads nor writes a key.
        Protocol::MemcacheText => (b"version\r\n", b"VERSION "),
    }
}

/// A connection, either kind.
///
/// The two socket types have no trait in common in the standard library, so this is where the two paths meet.
fn open(endpoint: Endpoint<'_>) -> Result<Socket, String> {
    match endpoint {
        Endpoint::Unix(path) => unix(path),
        Endpoint::Tcp(port) => TcpStream::connect(("127.0.0.1", port))
            .map(Socket::Tcp)
            .map_err(|why| why.to_string()),
    }
}

/// Unix sockets, which is what every sweep uses.
#[cfg(unix)]
fn unix(path: &Path) -> Result<Socket, String> {
    std::os::unix::net::UnixStream::connect(path)
        .map(Socket::Unix)
        .map_err(|why| format!("{}: {why}", path.display()))
}

/// Windows has unix sockets now, and the standard library does not expose them.
#[cfg(not(unix))]
fn unix(path: &Path) -> Result<Socket, String> {
    Err(format!(
        "{}: unix sockets are not available on this platform",
        path.display()
    ))
}

/// Whichever kind of connection was opened.
enum Socket {
    /// A unix socket.
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    /// A TCP connection to loopback.
    Tcp(TcpStream),
}

/// Both socket types have the same three calls on them and no shared trait, so they are forwarded by hand.
macro_rules! forward {
    ($self:ident, $stream:ident, $body:expr) => {
        match $self {
            #[cfg(unix)]
            Socket::Unix($stream) => $body,
            Socket::Tcp($stream) => $body,
        }
    };
}

impl Socket {
    /// How long to wait for an answer before giving up on this attempt.
    fn set_read_timeout(&self, how_long: Option<Duration>) -> std::io::Result<()> {
        forward!(self, stream, stream.set_read_timeout(how_long))
    }

    /// How long to wait to send the question.
    fn set_write_timeout(&self, how_long: Option<Duration>) -> std::io::Result<()> {
        forward!(self, stream, stream.set_write_timeout(how_long))
    }
}

impl std::io::Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        forward!(self, stream, stream.read(buf))
    }
}

impl std::io::Write for Socket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        forward!(self, stream, stream.write(buf))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        forward!(self, stream, stream.flush())
    }
}

/// A server that never got to the point of answering.
#[derive(Debug, thiserror::Error)]
pub enum NotReady {
    /// It exited while being waited for, which is a bad flag or a port in use.
    #[error(
        "{kind} exited {waited:?} after being started, so check the server log next to the run"
    )]
    Exited {
        /// Which server.
        kind: CacheKind,
        /// How long it lasted.
        waited: Duration,
    },
    /// It stayed up and never answered.
    #[error("{kind} was still not answering after {waited:?}, and the last attempt said: {last}")]
    Silent {
        /// Which server.
        kind: CacheKind,
        /// How long it was given.
        waited: Duration,
        /// What the last attempt reported, which is the useful half.
        last: String,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use cb_core::Endpoint;
    use cb_core::{CacheKind, Protocol};

    use super::greeting;
    #[cfg(unix)]
    use super::wait;
    #[cfg(unix)]
    use crate::supervise::Supervisor;

    #[test]
    fn resp_servers_are_asked_to_ping_and_memcached_is_asked_its_version() {
        assert_eq!(greeting(Protocol::Resp), (&b"PING\r\n"[..], &b"+PONG"[..]));
        assert_eq!(
            greeting(Protocol::MemcacheText),
            (&b"version\r\n"[..], &b"VERSION "[..])
        );
        // The six that speak RESP get the same question, and memcached is the one that does not.
        for kind in CacheKind::ALL {
            let expected = if kind == CacheKind::Memcache {
                Protocol::MemcacheText
            } else {
                Protocol::Resp
            };
            assert_eq!(kind.protocol(), expected, "{kind}");
        }
    }

    // The failure this replaces. A sleep cannot tell a slow server from one that died on its first flag.
    #[cfg(unix)]
    #[test]
    fn a_server_that_exited_is_reported_as_exited_rather_than_as_slow() {
        let socket = std::env::temp_dir().join(format!("cb-ready-{}.sock", std::process::id()));
        let mut server = Supervisor::new("/bin/sh")
            .args(["-c", "exit 1"])
            .start()
            .expect("starts");
        let why = wait(
            CacheKind::Valkey,
            Endpoint::Unix(&socket),
            &mut server,
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(why.to_string().contains("exited"), "{why}");
        // The deadline was thirty seconds and this has to come back long before that, because the point is that it does not wait for a server that is already gone.
    }

    // The happy path, against something that answers the way a server does.
    #[cfg(unix)]
    #[test]
    fn a_server_that_answers_is_ready_and_the_wait_says_how_long_it_took() {
        use std::io::{Read as _, Write as _};
        use std::os::unix::net::UnixListener;

        let socket = std::env::temp_dir().join(format!("cb-pong-{}.sock", std::process::id()));
        std::fs::remove_file(&socket).ok();
        let listener = UnixListener::bind(&socket).expect("binds");
        let answering = std::thread::spawn(move || {
            let (mut client, _) = listener.accept().expect("accepts");
            let mut asked = [0_u8; 16];
            let read = client.read(&mut asked).expect("reads");
            assert_eq!(&asked[..read], b"PING\r\n");
            client.write_all(b"+PONG\r\n").expect("answers");
        });

        let mut server = Supervisor::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .start()
            .expect("starts");
        let took = wait(
            CacheKind::Valkey,
            Endpoint::Unix(&socket),
            &mut server,
            Duration::from_secs(10),
        )
        .expect("is ready");
        assert!(took < Duration::from_secs(10));
        answering.join().expect("the fake server did not fail");
        server.stop(Duration::from_secs(5)).expect("stops");
        std::fs::remove_file(&socket).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_never_answers_says_what_the_last_attempt_saw() {
        let socket = std::env::temp_dir().join(format!("cb-quiet-{}.sock", std::process::id()));
        let mut server = Supervisor::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .start()
            .expect("starts");
        let why = wait(
            CacheKind::Redis,
            Endpoint::Unix(&socket),
            &mut server,
            Duration::from_millis(100),
        )
        .unwrap_err();
        let text = why.to_string();
        assert!(text.contains("still not answering"), "{text}");
        assert!(text.contains(&socket.display().to_string()), "{text}");
        server.stop(Duration::from_secs(5)).expect("stops");
    }
}
