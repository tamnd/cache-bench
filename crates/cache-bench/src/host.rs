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
        load: load_average(),
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

/// What the machine's one minute load average is right now.
///
/// Asked again per run by the sweep rather than once by the probe, because the whole point of recording it is that it changes: a cell measured in an hour where somebody else was on the machine is a cell that can be found afterwards rather than guessed at.
pub(crate) fn load_average() -> Option<f64> {
    file("/proc/loadavg").as_deref().and_then(load)
}

/// The one minute load average, which is the first of the three.
fn load(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

/// How much of this machine somebody else is using, in cores, sampled over a window.
///
/// The load average cannot answer this and it is worth being clear about why, because the sweep asked it for months and got a useless answer. A load average is a decaying average over the last minute, and a run drives every pinned core flat out for minutes. Ask it between two runs and most of what comes back is the run that just ended, so an idle machine reads as eight busy cores and there is no threshold anywhere that separates that from eight busy cores belonging to somebody else.
///
/// The counters in `/proc/stat` are cumulative rather than averaged, so a difference across a window that begins after the last server was killed is what happened during that window and nothing before it. The window is the caller's, and the caller is expected to leave a moment first for the teardown of its own last run.
///
/// In cores rather than as a fraction, because cores are the unit the profiles are written in: half a core on a box whose cache half is four cores is an eighth of the thing being measured.
pub(crate) fn busy_cores(sample: std::time::Duration, cpus: u32) -> Option<f64> {
    let before = cpu_times(&file("/proc/stat")?)?;
    std::thread::sleep(sample);
    let after = cpu_times(&file("/proc/stat")?)?;
    share(before, after, cpus)
}

/// Busy and total jiffies out of the summary line of `/proc/stat`.
///
/// The first line is the whole machine and the ones under it are the individual cores, which are the same numbers again and would double everything if they were added in.
///
/// Only the first eight counters are read. Those are the ones every kernel since 2.6.11 has, and the two after them, guest and guest nice, are already counted inside user and nice, so adding them would inflate the total and quietly report a busy machine as a quieter one. Steal counts as busy, because a core the hypervisor took is a core this is not getting, which is exactly what is being looked for.
fn cpu_times(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().next()?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let (mut total, mut spare) = (0_u64, 0_u64);
    let mut counters = 0;
    for (at, field) in fields.take(8).enumerate() {
        let value: u64 = field.parse().ok()?;
        total = total.checked_add(value)?;
        // Idle is the fourth counter and iowait the fifth, and neither of them is somebody using the machine.
        if at == 3 || at == 4 {
            spare = spare.checked_add(value)?;
        }
        counters += 1;
    }
    // Fewer than five and there is no idle counter to subtract, so whatever this file is, it is not one this knows how to read.
    if counters < 5 {
        return None;
    }
    Some((total.checked_sub(spare)?, total))
}

/// What share of the machine was busy between two readings, in cores.
///
/// Nothing is assumed about the tick rate, because the busy jiffies are divided by the total jiffies rather than by the wall clock. A window so long that the difference does not fit in a `u32` is not a window this takes, and it answers nothing rather than a wrapped number.
fn share(before: (u64, u64), after: (u64, u64), cpus: u32) -> Option<f64> {
    let busy = u32::try_from(after.0.checked_sub(before.0)?).ok()?;
    let total = u32::try_from(after.1.checked_sub(before.1)?).ok()?;
    (total > 0).then(|| f64::from(busy) / f64::from(total) * f64::from(cpus))
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
    use super::{cpu_times, cpuinfo, distro, load, lscpu, meminfo, probe, share, summarise};

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

    // The summary line is the whole machine and the lines under it are the same numbers again, one core at a time.
    #[test]
    fn the_cpu_counters_come_off_the_summary_line() {
        let text = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\n";
        assert_eq!(cpu_times(text).unwrap(), (150, 1000));
    }

    // Guest and guest nice are already inside user and nice, so adding them would inflate the total and report a busy machine as a quieter one.
    #[test]
    fn guest_time_is_not_counted_twice() {
        let with_guest = "cpu  100 0 50 800 50 0 0 0 400 0\n";
        assert_eq!(cpu_times(with_guest).unwrap(), (150, 1000));
    }

    // A file this does not recognise answers nothing rather than a number, because the number decides whether a sweep runs.
    #[test]
    fn a_stat_file_this_does_not_understand_answers_nothing() {
        assert_eq!(cpu_times("intr 1 2 3\n"), None);
        assert_eq!(cpu_times("cpu  100 0 50\n"), None);
        assert_eq!(cpu_times("cpu  100 not_a_number 50 800 50\n"), None);
        assert_eq!(cpu_times(""), None);
    }

    // Cores rather than a fraction, because cores are the unit the profiles are written in.
    #[test]
    fn the_busy_share_is_counted_in_cores() {
        // A quarter of a sixteen core machine busy is four cores busy.
        assert_eq!(share((0, 0), (400, 1600), 16).unwrap(), 4.0);
        // Nothing at all, which is what a machine with only this sweep on it looks like between two runs.
        assert_eq!(share((100, 1000), (100, 2000), 8).unwrap(), 0.0);
    }

    // Two readings with no time between them divide by zero, and a counter that went backwards is a machine that was rebooted mid sweep.
    #[test]
    fn a_window_with_nothing_in_it_answers_nothing() {
        assert_eq!(share((100, 1000), (100, 1000), 8), None);
        assert_eq!(share((100, 1000), (50, 900), 8), None);
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
