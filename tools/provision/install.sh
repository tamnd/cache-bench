#!/usr/bin/env bash
#
# Turn an Ubuntu box into one that can run a sweep.
#
# It installs the build dependencies, a Rust toolchain, a .NET SDK for Garnet, memtier_benchmark, and the seven cache servers, all at the versions pinned in versions.env, and it puts them where config.jsonc already looks for them: in directories beside this checkout, which is the layout the original uses.
#
# Everything here is safe to run again. A checkout that is already there is fetched rather than cloned, a binary that is already built at the pinned ref is left alone, and nothing is removed. Run it with no arguments to do all of it, or name the parts to do one at a time:
#
#   tools/provision/install.sh
#   tools/provision/install.sh redis valkey
#   tools/provision/install.sh --force garnet
#
# It does not measure anything and it does not start anything. What says whether a box is ready is `cache-bench doctor --deep`, which starts each server in turn and stops it again, and that is the command to run when this one finishes.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)

# Where the sibling checkouts go. config.jsonc names every binary as ../something, relative to this repository, so the default is the directory this repository sits in.
: "${WORK:=$(dirname "$root")}"
# Where memtier is linked so that `memtier_benchmark` on PATH is the one that was just built.
: "${PREFIX:=$HOME/.local}"

# shellcheck source=versions.env disable=SC1091
. "$here/versions.env"

force=no
if [ "${1:-}" = "--force" ]; then
    force=yes
    shift
fi
want=("$@")

ALL="deps rust dotnet memtier memcached redis valkey dragonfly garnet pogocache yo"

# A part name that is not one of those is a typo, and a typo that is silently ignored looks exactly like a part that finished in no time at all.
for one in "${want[@]}"; do
    case " $ALL " in
        *" $one "*) ;;
        *)
            echo "no part named $one. There is: $ALL" >&2
            exit 2
            ;;
    esac
done

# Whether this run was asked for a given part. No arguments means all of them.
wanted() {
    if [ ${#want[@]} -eq 0 ]; then
        return 0
    fi
    for one in "${want[@]}"; do
        if [ "$one" = "$1" ]; then
            return 0
        fi
    done
    return 1
}

say() {
    printf '\n== %s\n' "$*"
}

have() {
    command -v "$1" >/dev/null 2>&1
}

# Clone a repository if it is not there, and put it on the pinned ref either way.
#
# Detached rather than on a branch, on purpose. A checkout sitting on main is a checkout that changes under a sweep the next time somebody runs this, and the version of the server is part of the measurement.
fetch() {
    url=$1
    ref=$2
    dir=$3
    if [ ! -d "$dir/.git" ]; then
        git clone --recurse-submodules "$url" "$dir"
    fi
    git -C "$dir" fetch --tags --force origin
    git -C "$dir" checkout --detach "$ref"
    git -C "$dir" submodule update --init --recursive
}

# Whether a binary needs building: missing, or older than the checkout it came from, or the caller said so.
stale() {
    binary=$1
    dir=$2
    if [ "$force" = "yes" ] || [ ! -x "$binary" ]; then
        return 0
    fi
    head=$(git -C "$dir" rev-parse HEAD)
    stamp="$binary.built-from"
    if [ ! -f "$stamp" ] || [ "$(cat "$stamp")" != "$head" ]; then
        return 0
    fi
    echo "already built at $head"
    return 1
}

# Record which commit a binary was built from, which is what makes the next run of this script cheap.
built() {
    git -C "$2" rev-parse HEAD > "$1.built-from"
}

jobs=$(nproc)
mkdir -p "$WORK" "$PREFIX/bin"

if wanted deps; then
    say "build dependencies"
    # A machine that has been used for anything else has third party apt sources on it, and one of those being unreachable or unsigned fails the whole update even though every package below comes from the distribution. What matters is whether the install works, so the update is allowed to complain and the install is the thing that has to succeed.
    sudo apt-get update || echo "some apt sources did not refresh, carrying on with what is cached"
    # What every build below needs, and the run fails here if any of it is missing rather than in the middle of a compile an hour later.
    sudo apt-get install -y --no-install-recommends \
        autoconf automake build-essential ca-certificates cmake curl git libevent-dev \
        libssl-dev libtool pkg-config python3 zlib1g-dev
    # Packages that are named differently, or gone, depending on how old or new the release is. Ubuntu 24.04 has libpcre3-dev and 26.04 has only libpcre2-dev, and linux-tools-common is not on a WSL kernel at all. One name that has no candidate would fail the whole line, so these go one at a time and a miss is reported rather than fatal.
    for extra in libpcre2-dev libpcre3-dev linux-tools-common; do
        sudo apt-get install -y --no-install-recommends "$extra" >/dev/null 2>&1 \
            || echo "no $extra on this release, which is only a problem if a build below asks for it"
    done
    # perf comes from a package named after the running kernel, and a virtual machine can be running a kernel its distribution has no package for. That is not fatal here: doctor decides whether this box can measure cycles, and a profile with no perf in it does not need it.
    sudo apt-get install -y "linux-tools-$(uname -r)" || echo "no perf package for $(uname -r), so this box measures no cycles"
fi

if wanted rust; then
    say "rust"
    if have cargo; then
        cargo --version
    else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal
        # shellcheck disable=SC1091
        . "$HOME/.cargo/env"
    fi
fi

if wanted dotnet; then
    say "dotnet sdk $DOTNET_CHANNEL, for garnet"
    if [ -x "$HOME/.dotnet/dotnet" ] && "$HOME/.dotnet/dotnet" --list-sdks | grep -q "^$DOTNET_CHANNEL"; then
        "$HOME/.dotnet/dotnet" --list-sdks
    else
        # The distribution's package is a major version behind more often than not, and Garnet pins its SDK in a global.json, so this uses Microsoft's own installer into a directory of its own rather than fighting apt.
        curl -sSL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh
        bash /tmp/dotnet-install.sh --channel "$DOTNET_CHANNEL" --install-dir "$HOME/.dotnet"
    fi
fi

if wanted memtier; then
    say "memtier_benchmark $MEMTIER_REF"
    dir="$WORK/memtier_benchmark"
    fetch https://github.com/RedisLabs/memtier_benchmark.git "$MEMTIER_REF" "$dir"
    if stale "$dir/memtier_benchmark" "$dir"; then
        (cd "$dir" && autoreconf -ivf && ./configure && make -j"$jobs")
        built "$dir/memtier_benchmark" "$dir"
    fi
    ln -sf "$dir/memtier_benchmark" "$PREFIX/bin/memtier_benchmark"
    # The load generator is the one tool config.jsonc looks for on PATH rather than by path, so a second copy earlier on PATH would be the one that ran, and the version in every run file would be that one.
    found=$(command -v memtier_benchmark || true)
    if [ -n "$found" ] && [ "$found" != "$PREFIX/bin/memtier_benchmark" ]; then
        echo "warning: $found comes first on PATH, so put $PREFIX/bin in front of it or name the built one in config.jsonc"
    fi
fi

if wanted memcached; then
    say "memcached $MEMCACHED_REF"
    dir="$WORK/memcached"
    fetch https://github.com/memcached/memcached.git "$MEMCACHED_REF" "$dir"
    if stale "$dir/memcached" "$dir"; then
        (cd "$dir" && ./autogen.sh && ./configure && make -j"$jobs")
        built "$dir/memcached" "$dir"
    fi
fi

if wanted redis; then
    say "redis $REDIS_REF"
    dir="$WORK/redis"
    fetch https://github.com/redis/redis.git "$REDIS_REF" "$dir"
    if stale "$dir/src/redis-server" "$dir"; then
        (cd "$dir" && make -j"$jobs")
        built "$dir/src/redis-server" "$dir"
    fi
fi

if wanted valkey; then
    say "valkey $VALKEY_REF"
    dir="$WORK/valkey"
    fetch https://github.com/valkey-io/valkey.git "$VALKEY_REF" "$dir"
    if stale "$dir/src/valkey-server" "$dir"; then
        (cd "$dir" && make -j"$jobs")
        built "$dir/src/valkey-server" "$dir"
    fi
fi

if wanted dragonfly; then
    say "dragonfly $DRAGONFLY_REF"
    # The only one that is downloaded rather than built. Dragonfly's own instructions are to take the release binary, its build needs more memory than some of these boxes have, and the release is what the original measured.
    arch=$(uname -m)
    dir="$WORK/dragonfly"
    mkdir -p "$dir"
    if [ "$force" = "yes" ] || [ ! -x "$dir/dragonfly-$arch" ]; then
        curl -sSL -o /tmp/dragonfly.tar.gz \
            "https://github.com/dragonflydb/dragonfly/releases/download/$DRAGONFLY_REF/dragonfly-$arch.tar.gz"
        tar -xzf /tmp/dragonfly.tar.gz -C "$dir"
        chmod +x "$dir/dragonfly-$arch"
    fi
    "$dir/dragonfly-$arch" --version | head -1
fi

if wanted garnet; then
    say "garnet $GARNET_REF"
    dir="$WORK/garnet"
    fetch https://github.com/microsoft/garnet.git "$GARNET_REF" "$dir"
    out="$dir/main/GarnetServer/bin/Release/$GARNET_FRAMEWORK/GarnetServer"
    if stale "$out" "$dir"; then
        (cd "$dir" && "$HOME/.dotnet/dotnet" build main/GarnetServer/GarnetServer.csproj \
            -c Release -f "$GARNET_FRAMEWORK")
        built "$out" "$dir"
    fi
fi

if wanted pogocache; then
    say "pogocache $POGOCACHE_REF"
    dir="$WORK/pogocache"
    fetch https://github.com/tidwall/pogocache.git "$POGOCACHE_REF" "$dir"
    if stale "$dir/pogocache" "$dir"; then
        # Pogocache builds with -Werror, and a compiler newer than the one it was released against finds things in it that stop the build. GCC 15 on Ubuntu 26.04 does exactly that. This demotes those back to warnings and changes nothing else: it goes in through the environment rather than on the command line, because the Makefile appends the environment's CFLAGS to its own and a command line assignment would replace them, which would quietly take -O3 off a server we are about to measure.
        (cd "$dir" && CFLAGS="-Wno-error" make -j"$jobs")
        built "$dir/pogocache" "$dir"
    fi
fi

if wanted yo; then
    say "yo $YO_REF"
    dir="$WORK/yo"
    fetch https://github.com/tamnd/yo.git "$YO_REF" "$dir"
    if stale "$dir/target/release/yodb" "$dir"; then
        (cd "$dir" && cargo build --release --locked)
        built "$dir/target/release/yodb" "$dir"
    fi
fi

say "what is where"
# The same list config.jsonc holds, so that a line with nothing after it is a line to fix before starting a sweep.
for pair in \
    "memtier_benchmark:$PREFIX/bin/memtier_benchmark" \
    "memcache:$WORK/memcached/memcached" \
    "redis:$WORK/redis/src/redis-server" \
    "valkey:$WORK/valkey/src/valkey-server" \
    "dragonfly:$WORK/dragonfly/dragonfly-$(uname -m)" \
    "garnet:$WORK/garnet/main/GarnetServer/bin/Release/$GARNET_FRAMEWORK/GarnetServer" \
    "pogocache:$WORK/pogocache/pogocache" \
    "yo:$WORK/yo/target/release/yodb"; do
    name=${pair%%:*}
    path=${pair#*:}
    if [ -x "$path" ]; then
        printf '%-10s %s\n' "$name" "$path"
    else
        printf '%-10s missing\n' "$name"
    fi
done

printf '\nNow run: cargo run --release -p cache-bench -- doctor --profile <profile> --deep\n'
