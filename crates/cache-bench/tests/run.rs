//! One whole run, against a server and a load generator that are not real.
//!
//! Every piece of the runner has unit tests around it already. What those cannot check is the sequence: that the server is started before it is waited for, that the socket in the argv is the socket the readiness probe connects to, that the file memtier is told to write is the file that gets read back, and that what comes out the far end is a result file of the shape the rest of the harness reads.
//!
//! So this runs the real binary against a fake server that speaks just enough RESP to answer a `PING`, and a fake memtier that writes the JSON a real one writes. Both are Python, because a shell script cannot bind a unix socket and the alternative is a second Rust binary built only for this.
//!
//! It is not a benchmark and it measures nothing. It finishes in about a second, and if it breaks, the wiring is wrong.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(unix)]
mod unix {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A fake cache server. Binds whatever `--unixsocket` it was given and answers every line with `+PONG`.
    const SERVER: &str = r#"
import socket, sys, threading
sock = None
for i, a in enumerate(sys.argv):
    if a == "--unixsocket" and i + 1 < len(sys.argv):
        sock = sys.argv[i + 1]
if sock is None:
    print("fake-server 9.9.9", flush=True)
    sys.exit(0)
print("fake-server 9.9.9 listening", flush=True)
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(sock)
s.listen(64)
def serve(c):
    with c:
        while True:
            d = c.recv(4096)
            if not d:
                return
            c.sendall(b"+PONG\r\n" * d.count(b"\n"))
while True:
    c, _ = s.accept()
    threading.Thread(target=serve, args=(c,), daemon=True).start()
"#;

    /// A fake memtier. Reads its own argv for where to write and how many operations it was asked for, and reports exactly that many.
    ///
    /// `-n` is per connection and `-c` is per thread, so the total a real memtier reports is all three multiplied together. Getting that wrong here is what this fake is for: the runner checks the count it gets back against the count it asked for, and the two have to be the same kind of number.
    const MEMTIER: &str = r#"
import json, sys
out = None
n = c = t = 0
for i, a in enumerate(sys.argv):
    if a == "--json-out-file": out = sys.argv[i + 1]
    if a == "-n": n = int(sys.argv[i + 1])
    if a == "-c": c = int(sys.argv[i + 1])
    if a == "-t": t = int(sys.argv[i + 1])
stats = {"Ops/sec": 1234.5, "KB/sec": 2048.0, "Count": n * c * t, "Latency": 0.5,
         "Min Latency": 0.1, "Max Latency": 9.9, "Average Latency": 0.5,
         "Percentile Latencies": {"p50.00": 0.4, "p90.00": 0.8, "p99.00": 1.5,
                                  "p99.90": 3.0, "p99.99": 7.0}}
which = "Gets" if "0:1" in sys.argv else "Sets"
json.dump({"ALL STATS": {which: stats}}, open(out, "w"))
print("fake memtier ran the", which, "pass")
"#;

    /// A profile that fits on any machine this test will ever run on.
    ///
    /// The profiles in the tree pin to cores 0 to 15 and 16 to 31, and a CI runner has four. Pinning to a core that is not there is refused by the kernel rather than ignored, so the real profiles cannot be used here. The numbers are as small as the checks in `Profile::check` allow, because nothing in this test is measuring anything.
    const PROFILES: &str = r#"
[profiles.fake]
description = "Two cores and nothing real, for the test that runs the whole sequence against fakes."
cores = 2
cache_pin = "0"
bench_pin = "1"
threads = [1]
bench_threads = 1
connections_per_thread = 1
operations = 10
size_range = "1-1024"
key_maximum = 100
maxmemory = "64mb"
pipelines = [1]
runs = 1
perf = ["no"]

[profiles.sweepy]
description = "The same two cores, with four cells in it, for the test that sweeps and then restarts."
cores = 2
cache_pin = "0"
bench_pin = "1"
threads = [1]
bench_threads = 1
connections_per_thread = 1
operations = 10
size_range = "1-1024"
key_maximum = 100
maxmemory = "64mb"
pipelines = [1, 10]
runs = 2
perf = ["no"]
"#;

    /// Whether there is a python3 to run the fakes with.
    ///
    /// Every machine this project is developed or tested on has one. A machine that does not gets told the test was skipped rather than told it failed, because what would have failed is the test's own scaffolding.
    fn have_python() -> bool {
        Command::new("python3")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    }

    /// A working directory of this test's own, emptied first.
    fn workspace(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cache-bench-run-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("makes the working directory");
        dir
    }

    /// Write one of the fakes and make it executable.
    fn fake(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;

        let path = dir.join(name);
        std::fs::write(&path, format!("#!/usr/bin/env python3{body}")).expect("writes the fake");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    /// The whole scaffolding: two fakes, a config that points at them, a profile that fits, and somewhere to put the results.
    fn scaffold(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = workspace(tag);
        let server = fake(&dir, "fake-server", SERVER);
        let memtier = fake(&dir, "fake-memtier", MEMTIER);
        let config = dir.join("config.jsonc");
        // Valkey is named and its binary is not there, which is what a broken engine looks like from here. Nothing that sweeps only redis touches it.
        std::fs::write(
            &config,
            format!(
                "{{ \"paths\": {{ \"memtier\": {:?}, \"redis\": {:?}, \"valkey\": {:?} }} }}",
                memtier.display().to_string(),
                server.display().to_string(),
                dir.join("no-such-valkey").display().to_string()
            ),
        )
        .expect("writes the config");
        std::fs::write(dir.join("profiles.toml"), PROFILES).expect("writes the profiles");
        let results = dir.join("results");
        (dir, config, results)
    }

    /// `cache-bench run` with the arguments every case here shares.
    fn cache_bench(
        dir: &Path,
        config: &Path,
        results: &Path,
        extra: &[&str],
    ) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_cache-bench"))
            .args(["run", "redis", "--threads", "1", "--profile", "fake"])
            .arg("--config")
            .arg(config)
            .arg("--dir")
            .arg(results)
            .arg("--socket")
            .arg(dir.join("cb.sock"))
            .arg("--profiles")
            .arg(dir.join("profiles.toml"))
            .args(extra)
            .output()
            .expect("runs cache-bench")
    }

    /// `cache-bench sweep` over the four cell profile, against the same fakes. Which engines it sweeps is up to the caller.
    fn sweep(dir: &Path, config: &Path, results: &Path, extra: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_cache-bench"))
            .args(["sweep", "--profile", "sweepy"])
            .arg("--config")
            .arg(config)
            .arg("--dir")
            .arg(results)
            .arg("--socket")
            .arg(dir.join("cb.sock"))
            .arg("--profiles")
            .arg(dir.join("profiles.toml"))
            .args(extra)
            .output()
            .expect("runs cache-bench")
    }

    // The whole sequence, and the file it produces.
    #[test]
    fn a_run_starts_a_server_drives_it_and_writes_a_result() {
        if !have_python() {
            eprintln!("skipped: this machine has no python3 to run the fake server with");
            return;
        }
        let (dir, config, results) = scaffold("whole");
        let out = cache_bench(&dir, &config, &results, &[]);
        let said = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{said}");

        let path = results
            .join("runs")
            .join("bench_redis-threads_1-pipeline_1-perf_no-run_1.json");
        let text = std::fs::read_to_string(&path).expect("wrote the run file");
        // The version is the server's own first line, and the counters are absent rather than zero.
        assert!(text.contains("\"version\":\"fake-server 9.9.9\""), "{text}");
        assert!(text.contains("\"opsec\":1234.500"), "{text}");
        assert!(text.contains("\"perf\": {}"), "{text}");
        // Ours, and both present because this ran in corrected mode.
        assert!(text.contains("\"profile\":\"fake\""), "{text}");
        assert!(text.contains("\"run_started\""), "{text}");

        // Three passes ran, and each one wrote where it was told to.
        for pass in ["warmup", "sets", "gets"] {
            let log = results.join("logs").join(format!(
                "bench_redis-threads_1-pipeline_1-perf_no-run_1-{pass}.log"
            ));
            assert!(log.exists(), "the {pass} pass left no log");
        }
        // The directory was given back, so the next run is not refused by a process that has exited.
        assert!(!results.join(".lock").exists(), "the lock was not released");

        // A cell that is already on disk is skipped rather than measured again, which is the whole of how a sweep is restartable.
        let again = cache_bench(&dir, &config, &results, &[]);
        assert!(again.status.success());
        assert!(
            String::from_utf8_lossy(&again.stdout).contains("already measured"),
            "a run that was already on disk was measured again"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Upstream mode writes the original's file, which does not have this project's two extra keys in it.
    #[test]
    fn upstream_mode_writes_the_originals_fields_and_no_others() {
        if !have_python() {
            eprintln!("skipped: this machine has no python3 to run the fake server with");
            return;
        }
        let (dir, config, results) = scaffold("upstream");
        let out = cache_bench(&dir, &config, &results, &["--compat", "upstream"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let text = std::fs::read_to_string(
            results
                .join("runs")
                .join("bench_redis-threads_1-pipeline_1-perf_no-run_1.json"),
        )
        .expect("wrote the run file");
        assert!(!text.contains("\"profile\""), "{text}");
        assert!(!text.contains("\"run_started\""), "{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // The failure that produces plausible numbers for the wrong engine. A server left over from an earlier cell would answer this whole run.
    #[test]
    fn a_server_already_on_the_socket_stops_the_run() {
        if !have_python() {
            eprintln!("skipped: this machine has no python3 to run the fake server with");
            return;
        }
        let (dir, config, results) = scaffold("stray");
        let socket = dir.join("cb.sock");
        // The supervisor is the right way to start a server, and this is a test that starts one the wrong way on purpose, because a stray is by definition a process this harness did not start and is not holding a handle to.
        #[allow(
            clippy::disallowed_methods,
            reason = "this process stands in for one the harness did not start"
        )]
        let mut stray = Command::new(dir.join("fake-server"))
            .arg("--unixsocket")
            .arg(&socket)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("starts the stray server");
        // Give it long enough to bind, since the point of the test is that the socket is answering.
        for _ in 0..100 {
            if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let out = cache_bench(&dir, &config, &results, &[]);
        let why = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(!out.status.success(), "the stray server was measured");
        assert!(why.contains("already answering"), "{why}");

        let _ = stray.kill();
        let _ = stray.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // The gate this milestone is measured against, in miniature. A sweep completes, a second one repeats nothing, and a file that was left half written is measured again instead of being counted as a run.
    #[test]
    fn a_sweep_measures_every_cell_once_and_a_restart_repeats_none_of_them() {
        if !have_python() {
            eprintln!("skipped: this machine has no python3 to run the fake server with");
            return;
        }
        let (dir, config, results) = scaffold("sweep");
        let out = sweep(&dir, &config, &results, &["--cache", "redis"]);
        let said = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{said}");

        // Two pipeline depths and two runs each, in the order the original measures them.
        let names = [
            "bench_redis-threads_1-pipeline_1-perf_no-run_1.json",
            "bench_redis-threads_1-pipeline_1-perf_no-run_2.json",
            "bench_redis-threads_1-pipeline_10-perf_no-run_1.json",
            "bench_redis-threads_1-pipeline_10-perf_no-run_2.json",
        ];
        for name in names {
            assert!(
                results.join("runs").join(name).exists(),
                "{name} is missing"
            );
        }
        let at = |name: &str| said.find(name).expect("every cell was named as it was run");
        assert!(at(names[0]) < at(names[1]), "{said}");
        assert!(at(names[1]) < at(names[2]), "{said}");
        assert!(at(names[2]) < at(names[3]), "{said}");
        assert!(!results.join(".lock").exists(), "the lock was not released");

        // One line per attempt, and nothing missing, which is what an empty failure file says.
        let log = std::fs::read_to_string(results.join("logs").join("sweep.jsonl"))
            .expect("wrote the journal");
        assert_eq!(log.lines().count(), 4, "{log}");
        assert!(log.contains("\"outcome\":\"measured\""), "{log}");
        let failures =
            std::fs::read_to_string(results.join("failures.json")).expect("wrote the failure file");
        assert!(failures.contains("\"failures\": []"), "{failures}");

        // Started again, it measures nothing, because everything it was going to measure is on disk.
        let again = sweep(&dir, &config, &results, &["--cache", "redis"]);
        assert!(again.status.success());
        assert!(
            String::from_utf8_lossy(&again.stdout).contains("0 measured here, 4 already on disk"),
            "{}",
            String::from_utf8_lossy(&again.stdout)
        );

        // A file of the right name holding half a run is what a machine that lost power partway through a write leaves behind.
        let cut = results.join("runs").join(names[2]);
        let whole = std::fs::read_to_string(&cut).expect("reads the run file");
        std::fs::write(&cut, &whole[..whole.len() / 2]).expect("truncates it");
        let third = sweep(&dir, &config, &results, &["--cache", "redis"]);
        let told = String::from_utf8_lossy(&third.stdout).into_owned();
        assert!(third.status.success(), "{told}");
        assert!(told.contains("being measured again"), "{told}");
        assert!(
            told.contains("1 measured here, 3 already on disk"),
            "{told}"
        );
        // Whole again. Not byte for byte the same as before, because the run it now holds started a second later than the one it used to hold.
        let back = std::fs::read_to_string(&cut).expect("reads it back");
        assert_eq!(back.len(), whole.len(), "{back}");
        assert!(back.ends_with("\"perf\": {}\n}\n"), "{back}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // What a sweep leaves behind besides the runs: a line per attempt, and a file naming what is missing.
    #[test]
    fn a_sweep_writes_down_what_it_did_and_what_it_could_not_do() {
        if !have_python() {
            eprintln!("skipped: this machine has no python3 to run the fake server with");
            return;
        }
        let (dir, config, results) = scaffold("record");
        // Valkey is named in the config and its binary is not there, so all four of its cells fail. Valkey is swept before redis, so this also checks that a broken engine does not stop the ones behind it.
        let out = sweep(&dir, &config, &results, &["--cache", "valkey"]);
        let said = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "a sweep that measured nothing said it was fine: {said}"
        );

        // Three failures, and then the rest of that engine left alone rather than failing one cell at a time for a day.
        let failures =
            std::fs::read_to_string(results.join("failures.json")).expect("wrote the failure file");
        assert_eq!(failures.matches("\"cell\"").count(), 3, "{failures}");
        assert!(failures.contains("\"attempts\": 1"), "{failures}");
        assert!(failures.contains("\"cache\": \"valkey\""), "{failures}");
        assert!(said.contains("failed 3 times in a row"), "{said}");
        assert!(said.contains("1 left alone"), "{said}");

        // One line per attempt, and every one of them says what happened.
        let log = std::fs::read_to_string(results.join("logs").join("sweep.jsonl"))
            .expect("wrote the journal");
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 3, "{log}");
        for line in &lines {
            assert!(line.contains("\"outcome\":\"failed\""), "{line}");
            assert!(line.contains("\"why\":"), "{line}");
            assert!(line.contains("\"seconds\":"), "{line}");
        }
        // An engine the config never named stops the sweep before it starts, rather than failing a thousand cells one at a time on day three.
        let unnamed = sweep(&dir, &config, &results, &["--cache", "yo"]);
        let why = String::from_utf8_lossy(&unnamed.stderr).into_owned();
        assert!(!unnamed.status.success(), "{why}");
        assert!(why.contains("yo"), "{why}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // A dry run says what it would do and touches nothing, which is how somebody checks the shape of a sweep before giving up a machine for a week.
    #[test]
    fn a_dry_run_names_the_cells_and_measures_none_of_them() {
        let (dir, config, results) = scaffold("dry");
        let out = sweep(&dir, &config, &results, &["--cache", "redis", "--dry-run"]);
        let said = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{said}");
        assert!(said.contains("4 cells over redis"), "{said}");
        assert!(
            said.contains("bench_redis-threads_1-pipeline_10-perf_no-run_2.json"),
            "{said}"
        );
        assert!(!results.join("runs").exists(), "a dry run wrote results");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
