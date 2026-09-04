//! One writer at a time in a results directory.
//!
//! Two runs sharing a results directory is not a race over a file, it is a race over the machine. Both start a server, both pin to the same cores, both bind the same socket, and both write result files that look exactly like the ones a healthy sweep writes. The numbers come out low and there is nothing in them that says why.
//!
//! The original has no equivalent, because the original is a shell script that somebody watches. A sweep here runs for days unattended, and the way that goes wrong is somebody starting a second one on Wednesday having forgotten about Monday's.
//!
//! This is a lock file rather than an advisory lock on a descriptor, because it has to be readable. A run that stops because the directory is busy should say which pid has it, and a flock does not leave anything behind for a person to look at.

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
                let _ = std::fs::write(&path, format!("{}\n", std::process::id()));
                Ok(Self { path })
            }
            Err(why) if why.kind() == std::io::ErrorKind::AlreadyExists => Err(format!(
                "{} is held by {}, so something is already writing to {}. Stop it, or remove that file if nothing is.",
                path.display(),
                held_by(&path),
                dir.display()
            )),
            Err(why) => Err(format!("cannot take {}: {why}", path.display())),
        }
    }
}

/// Whoever wrote the lock file, for the message.
fn held_by(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => format!("pid {}", text.trim()),
        _ => "a process that did not say which".to_owned(),
    }
}

/// Released on the way out, including on the way out of a failed run.
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::Lock;

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
}
