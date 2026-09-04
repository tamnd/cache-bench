# Provisioning a host

`install.sh` turns a fresh Ubuntu box into one that can run a sweep. It installs the build dependencies, a Rust toolchain, a .NET SDK for Garnet, `memtier_benchmark` and the seven cache servers, all at the versions pinned in `versions.env`, into directories beside this checkout, which is where `config.jsonc` already looks for them.

```
tools/provision/install.sh
```

That is the whole of it on a machine with passwordless sudo. It takes about half an hour the first time, most of it building Redis, Valkey and Garnet, and a few seconds every time after that, because a binary already built at the pinned commit is left alone. Name parts to do one at a time, and pass `--force` to build something again anyway:

```
tools/provision/install.sh redis valkey
tools/provision/install.sh --force garnet
```

Two environment variables move things. `WORK` is where the sibling checkouts go and defaults to the directory this repository sits in, which is what `config.jsonc` means by `../`. `PREFIX` is where `memtier_benchmark` is linked so that the name on PATH is the build that was just pinned, and it defaults to `~/.local`.

## Versions are results, not setup

Every version in `versions.env` is written into every run file this harness produces. Two sweeps on two machines are comparable only if these agree, so they are pinned here rather than being whatever each box happened to have on the day, and bumping one is a commit of its own so that a chart that moved can be traced to the thing that moved it.

Dragonfly is the one that is downloaded rather than built. Its own instructions say to take the release binary, its build wants more memory than some of these machines have, and the release binary is what the original measured.

## What this does not do

It does not decide whether the box is fit to measure anything. That is `cache-bench doctor`, which reads the cores, the core split, the memory against the profile's limit, the counters and the load average, and refuses a profile the host cannot honestly run:

```
cargo run --release -p cache-bench -- doctor --profile <profile> --deep
```

`--deep` starts each of the seven servers in turn, waits for it to answer and stops it again, which is the check no file can make and the one that catches a binary of the wrong architecture, a server built without unix socket support, or a Garnet whose runtime is not installed.

It also does not touch anything outside `WORK`, `PREFIX/bin` and the packages apt installs. It never removes anything, and running it twice is the normal case rather than the recovery path.
