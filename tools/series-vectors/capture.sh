#!/usr/bin/env bash
#
# Produces testdata/golden/series.json from a checkout of the original.
#
# The original's graph tool pastes its numbers into a Python script, runs python3 on it and deletes the script.
# Nothing else ever sees what went on a chart, so the way to get the original's own series is to stand in for python3, keep the script and throw away the drawing.
# The fake python3 below copies the script it was handed and exits, which the Go side treats as a successful render.
#
# Usage: tools/series-vectors/capture.sh /path/to/cache-benchmarks

set -euo pipefail

upstream=${1:?usage: capture.sh /path/to/cache-benchmarks}
upstream=$(cd "$upstream" && pwd)
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
out="$here/../../testdata/golden/series.json"

if [[ ! -f "$upstream/results/output.json" ]]; then
    echo "no results/output.json under $upstream" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/scripts"
ln -s "$upstream/results" "$work/results"

cat > "$work/bin/python3" <<'EOF'
#!/usr/bin/env bash
# Stands in for the real python3. Keeps the script and draws nothing.
set -eu
name=$(grep -m1 '^filename = ' "$1" | sed -e 's/^filename = "//' -e 's/"$//')
cp "$1" "$CAPTURE/$(basename "$name" .png).py"
EOF
chmod +x "$work/bin/python3"

(cd "$upstream/cmd" && go build -o "$work/graph" ./graph)

# The order here is bench-all.sh's order, so that a chart missing from the capture is a chart the original does not draw either.
# --force is the one flag bench-all.sh does not pass, and it is needed because the original skips any chart whose PNG is already on disk.
draw() {
    CAPTURE="$work/scripts" PATH="$work/bin:$PATH" \
        "$work/graph" --dir=results --force "$@"
}

cd "$work"
for scale in logarithmic linear; do
    for pipeline in 1 10 25 50; do
        for op in get set; do
            draw --bench=throughput --pipeline=$pipeline --which=$op --scale=$scale
        done
    done
    for pipeline in 1 10 25 50; do
        for percentile in 50 90 99 999 9999 min max avg; do
            for op in get set; do
                draw --bench=latency --pipeline=$pipeline --percentile=$percentile --which=$op --scale=$scale
            done
        done
    done
    for pipeline in 1 10 25 50; do
        draw --bench=cpucycles --pipeline=$pipeline --scale=$scale
    done
done

# The two the original carves out by hand, after the loop, linear only.
draw --bench=latency --pipeline=1 --percentile=99 --which=set --scale=linear --scase=1
draw --bench=latency --pipeline=1 --percentile=99 --which=get --scale=linear --scase=1

count=$(ls "$work/scripts" | wc -l | tr -d ' ')
if [[ "$count" != "154" ]]; then
    echo "captured $count scripts, expected 154" >&2
    exit 1
fi

python3 "$here/assemble.py" "$work/scripts" > "$out"
echo "wrote $out from $count charts"
