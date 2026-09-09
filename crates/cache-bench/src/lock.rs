//! One writer at a time in a results directory.
//!
//! Two runs sharing a results directory is not a race over a file, it is a race over the machine. Both start a server, both pin to the same cores, both bind the same socket, and both write result files that look exactly like the ones a healthy sweep writes. The numbers come out low and there is nothing in them that says why.
//!
//! The original has no equivalent, because the original is a shell script that somebody watches. A sweep here runs for days unattended, and the way that goes wrong is somebody starting a second one on Wednesday having forgotten about Monday's.
//!
//! This is a lock file rather than an advisory lock on a descriptor, because it has to be readable. A run that stops because the directory is busy should say which pid has it, and a flock does not leave anything behind for a person to look at.
//!
//! # When the writer is not there any more
//!
//! A lock that only a clean exit takes away is a lock that a reboot turns into a wall. The machines these sweeps run on are not all ours to keep up, and one of them reboots whenever its owner wants it back. A sweep that has to be restarted by hand afterwards is not a sweep that runs for days unattended, so a lock whose writer is gone gets taken over instead of obeyed.
//!
//! Telling the difference is the whole of the care here, because taking over a lock that somebody is holding is the exact accident this module exists to prevent. So the file says enough about its writer to answer the question, and the answer has to be yes on every part of it before the lock is called stale. The boot has to be the one the lock was written on, because a pid from an earlier boot names nothing now. The pid has to be gone from `/proc`. And if it is there, it has to have started at the moment the lock says it did, because pids are reused. Anything that cannot be read is a no, so a machine without `/proc` obeys every lock it finds, which is what every machine did before this.
//!
//! Reading `/proc` and giving up elsewhere is the same trade `cb-mem` makes for the resident set, and for the same reason: the hosts these numbers come from are Linux, and a guess dressed up as a portable answer would be worse than a refusal.

use std::path::{Path, PathBuf};

/// The lock file's name inside a results directory.
const NAME: &str = ".lock";

/// A held results directory, released when this goes out of scope.
#[derive(Debug)]
pub(crate) struct Lock {
    /// The file to take away.
    path: PathBuf,
}

impl Lock {
    /// Take the directory, or say who has it.
    ///
    /// A lock left behind by a writer that is provably gone is taken over, and the takeover is said out loud rather than done quietly, because the one thing worse than a stuck sweep is a sweep that steals a directory and does not mention it.
    ///
    /// # Errors
    ///
    /// If something else holds it, or if the file cannot be written at all.
    pub(crate) fn take(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir)
            .map_err(|why| format!("cannot make {}: {why}", dir.display()))?;
        let path = dir.join(NAME);
        // Exclusive creation is the whole of the lock. It is one syscall on every filesystem this will run on, and it is atomic on all of them.
        match std::fs::File::create_new(&path) {
            Ok(_) => {
                let _ = std::fs::write(&path, Held::mine().text());
                Ok(Self { path })
            }
            Err(why) if why.kind() == std::io::ErrorKind::AlreadyExists => {
                let held = std::fs::read_to_string(&path)
                    .ok()
                    .as_deref()
                    .and_then(Held::read);
                if let Some(held) = held.filter(Held::gone) {
                    eprintln!(
                        "cache-bench: taking over {}, which pid {} left behind when it stopped without giving it back",
                        path.display(),
                        held.pid
                    );
                    // Not create_new, because the file is the one being taken over and it is already there.
                    std::fs::write(&path, Held::mine().text())
                        .map_err(|why| format!("cannot take {}: {why}", path.display()))?;
                    return Ok(Self { path });
                }
                Err(format!(
                    "{} is held by {}, so something is already writing to {}. Stop it, or remove that file if nothing is.",
                    path.display(),
                    held_by(&path),
                    dir.display()
                ))
            }
            Err(why) => Err(format!("cannot take {}: {why}", path.display())),
        }
    }
}

/// Whoever wrote the lock file, for the message.
fn held_by(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => match Held::read(&text) {
            Some(held) => format!("pid {}", held.pid),
            // A file written by a version of this that only put a pid in it, or by a hand.
            None if !text.trim().is_empty() => format!("pid {}", text.trim()),
            None => "a process that did not say which".to_owned(),
        },
        _ => "a process that did not say which".to_owned(),
    }
}

/// Released on the way out, including on the way out of a failed run.
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// What a lock file says about the run that wrote it.
///
/// Three fields, one a line, in the order a person would want to read them. A pid on its own would be enough to name the writer and is not enough to tell whether it is still there, which is what the other two are for.
#[derive(Debug, PartialEq, Eq)]
struct Held {
    /// The writing process.
    pid: u32,
    /// The boot it was running on, out of `/proc/sys/kernel/random/boot_id`, because a pid outlives nothing and a boot id outlives the machine's memory of it.
    boot: String,
    /// When it started, in the ticks since boot that `/proc/<pid>/stat` counts in, so that a reused pid is not mistaken for the original.
    started: u64,
}

impl Held {
    /// This process, as the lock file would describe it.
    ///
    /// The boot and the start time are empty and zero on a machine that has no `/proc`, which is a record that [`Self::gone`] will never call stale. That is the right answer there: a machine that cannot be asked should be obeyed.
    fn mine() -> Self {
        let pid = std::process::id();
        Self {
            pid,
            boot: boot_id().unwrap_or_default(),
            started: started(pid).unwrap_or_default(),
        }
    }

    /// The lock file's contents.
    fn text(&self) -> String {
        format!(
            "pid {}\nboot {}\nstarted {}\n",
            self.pid, self.boot, self.started
        )
    }

    /// A record back out of a lock file, or nothing if it is not one.
    fn read(text: &str) -> Option<Self> {
        let field = |name: &str| {
            text.lines()
                .filter_map(|line| line.split_once(' '))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.trim().to_owned())
        };
        Some(Self {
            pid: field("pid")?.parse().ok()?,
            boot: field("boot")?,
            started: field("started")?.parse().ok()?,
        })
    }

    /// Whether the process that wrote this is provably not running.
    ///
    /// Every unanswerable question is a no. Being unable to prove the writer is gone leaves the lock exactly as strong as it was before any of this existed, and that is the side to be wrong on.
    fn gone(&self) -> bool {
        let Some(boot) = boot_id() else {
            // No `/proc`, so nothing here can be checked and the lock stands.
            return false;
        };
        if self.boot != boot {
            // Written on an earlier boot, so whatever holds that pid now, it is not this.
            return true;
        }
        let Some(started) = started(self.pid) else {
            // Same boot and no such process, which is the plain case a reboot does not even reach.
            return true;
        };
        // There, but started at another moment, so this is a different process wearing a pid the old one gave back.
        started != self.started
    }
}

/// The current boot, which is a fresh value after every restart.
fn boot_id() -> Option<String> {
    let id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// When a pid started, in ticks since boot, or nothing if there is no such process.
fn started(pid: u32) -> Option<u64> {
    starttime(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The start time out of the text of a `/proc/<pid>/stat`.
///
/// Field twenty two, one based, in a line whose second field is the executable name in brackets and may contain spaces and brackets of its own. Everything before the last `)` is skipped for exactly that reason, the same way `cb-mem` reads the group id out of the same line.
fn starttime(stat: &str) -> Option<u64> {
    let after = stat.rsplit_once(')')?.1;
    // After the closing bracket the fields are numbered from three, so field twenty two is the twentieth of them.
    after.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{Held, Lock, boot_id, started, starttime};

    fn dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cache-bench-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_second_run_in_the_same_directory_is_refused_and_told_which_pid_has_it() {
        let dir = dir("held");
        let held = Lock::take(&dir).expect("takes it");
        let why = Lock::take(&dir).unwrap_err();
        assert!(
            why.contains(&format!("pid {}", std::process::id())),
            "{why}"
        );
        drop(held);
    }

    // A failed run has to give the directory back, or the next attempt is refused by a process that is not running any more.
    #[test]
    fn the_directory_is_given_back_when_the_run_ends() {
        let dir = dir("released");
        drop(Lock::take(&dir).expect("takes it"));
        Lock::take(&dir).expect("takes it again");
    }

    // The reboot case, which is the one that stopped a sweep on a real host for three hours. The lock names a boot that is not this one, so nothing that boot was running exists now, whatever its pid says.
    #[test]
    fn a_lock_left_behind_by_an_earlier_boot_is_taken_over() {
        let dir = dir("rebooted");
        drop(Lock::take(&dir).expect("takes it"));
        let stale = Held {
            pid: std::process::id(),
            boot: "not-the-boot-this-machine-is-on".to_owned(),
            started: 1,
        };
        std::fs::write(dir.join(".lock"), stale.text()).unwrap();
        if boot_id().is_none() {
            // No `/proc`, so the lock stands and that is the documented answer here.
            assert!(!stale.gone());
            return;
        }
        let took = Lock::take(&dir).expect("takes it over");
        let now = std::fs::read_to_string(dir.join(".lock")).unwrap();
        assert_eq!(Held::read(&now).unwrap(), Held::mine());
        drop(took);
    }

    // The other half of the same question, and the more important half. A lock held by a process that is genuinely running is obeyed, and this one is held by the process asking.
    #[test]
    fn a_lock_held_by_a_process_that_is_running_is_not_taken_over() {
        assert!(!Held::mine().gone(), "this process is running");
    }

    // A pid that is there but started at another moment is a pid that was given back and handed out again, and that is a different process.
    #[test]
    fn a_reused_pid_does_not_count_as_the_process_that_took_the_lock() {
        if boot_id().is_none() {
            return;
        }
        let mut mine = Held::mine();
        mine.started += 1;
        assert!(mine.gone(), "a start time that is not ours is not us");
    }

    #[test]
    fn a_record_survives_being_written_and_read_back() {
        let held = Held {
            pid: 2735,
            boot: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".to_owned(),
            started: 987_654,
        };
        assert_eq!(Held::read(&held.text()).unwrap(), held);
        // A file from before this had a bare pid in it, and a record that cannot be read is a lock that stands.
        assert_eq!(Held::read("2735\n"), None);
        assert_eq!(Held::read(""), None);
    }

    // The executable name is the caller's, and a server called `redis (test)` would shift every field after it if the brackets were not skipped.
    #[test]
    fn the_start_time_is_read_past_a_name_with_brackets_in_it() {
        // Field three is the state, which is the `R` below, so the rest start at four.
        let mut line = "2735 (redis (test)) R".to_owned();
        for n in 4..=22u64 {
            let value = if n == 22 { 987_654 } else { n };
            line.push(' ');
            line.push_str(&value.to_string());
        }
        assert_eq!(starttime(&line), Some(987_654));
        assert_eq!(starttime("nothing that looks like a stat line"), None);
    }

    #[test]
    fn a_pid_that_is_not_there_has_no_start_time() {
        // Pid zero is the scheduler and never appears in `/proc`, so there is no race in asking about it.
        assert_eq!(started(0), None);
    }
}
