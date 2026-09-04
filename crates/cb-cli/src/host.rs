//! What this machine is, read off the machine itself.
//!
//! Two jobs. It fills in the `host.json` that goes next to a results directory, and it answers the questions `doctor` refuses a sweep over. Both want the same facts, so they are gathered once.
//!
//! Everything the kernel publishes is a file, and every file here is parsed by a function that takes a string. That is not indirection for its own sake: it is the only way these can be tested anywhere other than on a Linux box with the right hardware, and a parser that has only ever run on the machine it was written on is a parser nobody has checked.
//!
//! A fact this machine does not publish comes back as absent rather than as a guess. `doctor` says which ones were missing, and refuses to write a `host.json` that cannot say what produced the numbers, because a results directory that does not know what measured it is the defect this file exists to fix.

use std::path::Path;
use std::process::Command;

/// What the machine says about itself.
#[derive(Debug, Default, Clone)]
pub(crate) struct Host {
    /// The kernel, as `uname` gives it.
    pub(crate) kernel: Option<String>,
    /// The distribution, for the userland the servers were built against.
    pub(crate) distro: Option<String>,
    /// The CPU, as the machine describes itself.
    pub(crate) cpu_model: Option<String>,
    /// How many logical CPUs it has.
    pub(crate) cpus: Option<u32>,
    /// How much memory it has.
    pub(crate) memory: Option<u64>,
    /// How much of that is available right now, which is a different question from how much is free.
    pub(crate) available: Option<u64>,
    /// What the frequency governor is set to.
    pub(crate) governor: Option<String>,
    /// Which mitigations are on, summarised.
    pub(crate) mitigations: Option<String>,
    /// The one minute load average.
    pub(crate) load: Option<f64>,
}

/// Ask the machine everything at once.
pub(crate) fn probe() -> Host {
    Host {
        kernel: kernel(),
        distro: file("/etc/os-release").as_deref().and_then(distro),
        cpu_model: cpu_model(),
        cpus: std::thread::available_parallelism()
            .ok()
            .and_then(|n| u32::try_from(n.get()).ok()),
        memory: file("/proc/meminfo")
            .as_deref()
            .and_then(|text| meminfo(text, "MemTotal")),
        available: file("/proc/meminfo")
            .as_deref()
            .and_then(|text| meminfo(text, "MemAvailable")),
        governor: file("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty()),
        mitigations: mitigations(),
        load: file("/proc/loadavg").as_deref().and_then(load),
    }
}

/// `uname -srm`, which is the same three fields the original prints and the shortest line that says what kernel this is.
fn kernel() -> Option<String> {
    let out = Command::new("uname").args(["-srm"]).output().ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// The distribution's own name for itself.
fn distro(os_release: &str) -> Option<String> {
    for line in os_release.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            let value = value.trim().trim_matches('"').trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

/// What the CPU calls itself.
///
/// `lscpu` first, because on ARM `/proc/cpuinfo` has no model name at all and lscpu is the thing that turns the implementer and part numbers into `Neoverse-V2`. The file is the fallback for a machine without lscpu installed, which is most containers.
fn cpu_model() -> Option<String> {
    if let Ok(out) = Command::new("lscpu").output()
        && out.status.success()
        && let Some(model) = lscpu(&String::from_utf8_lossy(&out.stdout))
    {
        return Some(model);
    }
    file("/proc/cpuinfo").as_deref().and_then(cpuinfo)
}

/// The model name out of `lscpu`.
fn lscpu(text: &str) -> Option<String> {
    field(text, "Model name")
}

/// The model name out of `/proc/cpuinfo`.
///
/// `model name` on x86, `Model` on some ARM boards, and neither on most of them.
fn cpuinfo(text: &str) -> Option<String> {
    field(text, "model name").or_else(|| field(text, "Model"))
}

/// The first `key: value` line with this key, trimmed.
fn field(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == key {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

/// One of `/proc/meminfo`'s counters, in bytes.
///
/// The file is in kibibytes and says so on every line. Anything that ever appears there in another unit is skipped rather than multiplied by a thousand and a bit.
fn meminfo(text: &str, key: &str) -> Option<u64> {
    let value = field(text, key)?;
    let (number, unit) = value.split_once(char::is_whitespace)?;
    (unit.trim() == "kB")
        .then(|| number.parse::<u64>().ok())
        .flatten()?
        .checked_mul(1024)
}

/// The one minute load average, which is the first of the three.
fn load(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

/// Which CPU vulnerabilities this kernel says are still open.
///
/// The mitigations matter here because several of them are worth double digit percentages on a workload that is mostly syscalls, so two results directories with different answers in this field are not comparable. The full text of all twenty files would be a paragraph nobody reads, so this is the count and then the names of the ones the kernel calls vulnerable, which is the part that differs between machines.
fn mitigations() -> Option<String> {
    let dir = Path::new("/sys/devices/system/cpu/vulnerabilities");
    let mut names = Vec::new();
    let mut open = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let Ok(said) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if said.trim_start().starts_with("Vulnerable") {
            open.push(name.clone());
        }
        names.push(name);
    }
    if names.is_empty() {
        return None;
    }
    names.sort();
    open.sort();
    Some(summarise(names.len(), &open))
}

/// The mitigations line, written out.
fn summarise(checked: usize, open: &[String]) -> String {
    if open.is_empty() {
        return format!("{checked} known, all of them mitigated");
    }
    format!(
        "{checked} known, {} left open: {}",
        open.len(),
        open.join(", ")
    )
}

/// Read a file the kernel publishes, or nothing.
fn file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::{cpuinfo, distro, load, lscpu, meminfo, probe, summarise};

    #[test]
    fn the_distribution_is_the_name_it_gives_itself() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04.3 LTS\"\n";
        assert_eq!(distro(text).unwrap(), "Ubuntu 24.04.3 LTS");
        assert_eq!(distro("NAME=\"Ubuntu\"\n"), None);
    }

    // The reference host is ARM, where /proc/cpuinfo has no model name and lscpu is the only thing that will say what the CPU is.
    #[test]
    fn an_arm_cpu_is_named_by_lscpu_and_not_by_cpuinfo() {
        let arm =
            "processor\t: 0\nBogoMIPS\t: 2100.00\nCPU implementer\t: 0x41\nCPU part\t: 0xd4f\n";
        assert_eq!(cpuinfo(arm), None);
        let listed = "Architecture:  aarch64\nCPU(s):        32\nModel name:    Neoverse-V2\n";
        assert_eq!(lscpu(listed).unwrap(), "Neoverse-V2");
    }

    #[test]
    fn an_x86_cpu_is_named_by_cpuinfo() {
        let text = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD EPYC 7302P 16-Core Processor\n";
        assert_eq!(cpuinfo(text).unwrap(), "AMD EPYC 7302P 16-Core Processor");
    }

    // The file is in kibibytes and a byte count that is off by a factor of 1024 would pass every check in doctor.
    #[test]
    fn memory_is_read_in_kibibytes_and_kept_in_bytes() {
        let text =
            "MemTotal:       65809436 kB\nMemFree:         1234 kB\nMemAvailable:   60000000 kB\n";
        assert_eq!(meminfo(text, "MemTotal").unwrap(), 65_809_436 * 1024);
        assert_eq!(meminfo(text, "MemAvailable").unwrap(), 60_000_000 * 1024);
        assert_eq!(meminfo(text, "Hugepagesize"), None);
    }

    #[test]
    fn a_counter_in_a_unit_this_does_not_know_is_not_guessed_at() {
        assert_eq!(meminfo("MemTotal:       64 GB\n", "MemTotal"), None);
    }

    #[test]
    fn the_load_average_is_the_one_minute_figure() {
        assert_eq!(load("0.42 1.10 2.00 1/1234 5678\n").unwrap(), 0.42);
        assert_eq!(load("not a number\n"), None);
    }

    #[test]
    fn the_mitigations_line_names_what_is_still_open() {
        assert_eq!(summarise(20, &[]), "20 known, all of them mitigated");
        assert_eq!(
            summarise(20, &["mds".to_owned(), "srbds".to_owned()]),
            "20 known, 2 left open: mds, srbds"
        );
    }

    // Whatever this machine is, asking it has to come back rather than fail, because doctor prints what it found and what it did not.
    #[test]
    fn asking_this_machine_answers() {
        let host = probe();
        assert!(host.cpus.unwrap_or(0) > 0);
    }
}
