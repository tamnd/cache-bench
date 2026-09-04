# Methodology

How a number in a chart here got made, from the process that produced it to the
pixel it ended up in. Anyone reading a result should read this first, because
the interesting part of a benchmark is never the number.

## What a run is

One run is one cell of the matrix measured once. It is defined by five things:
which cache server, how many threads that server was given, the pipeline depth,
whether `perf` was attached, and the run index from 1 to 31.

The sequence for a single run is fixed and does not vary by engine.

Start the cache server, pinned to the first half of the cores, with persistence
off, with a memory limit large enough that nothing is ever evicted, listening on
a unix socket in a temporary directory. Wait for it to answer, by connecting and
issuing a trivial command, not by sleeping. Run `memtier_benchmark` once as a
warmup and throw the result away. Run it again, pinned to the second half of the
cores, and keep that result. Stop the server, wait for the process group to be
gone, and check that nothing was left behind. Write one JSON file named for the
five things above.

The warmup is not optional and it is not a courtesy. Several of these engines
allocate their arenas lazily, and a first pass that includes page faults and
lazily grown hash tables measures the allocator rather than the cache.

The socket is a unix socket, so no network stack is in the measurement. That is
the original's choice and it is the right one for a hot path comparison, but it
does mean these numbers say nothing about how any of these servers behave over a
real network, which is how all of them are actually deployed.

`perf` runs are a separate half of the matrix rather than being always on,
because attaching a counter is not free and the throughput numbers should not
carry its cost. That doubles the size of the sweep and it is the only honest way
to have both.

## What the load generator does

`memtier_benchmark`, one instance, pinned to the cores the server is not using.
16 threads of 16 connections each is 256 connections. 100,000 operations per
connection. Values are 1 to 1024 bytes. The key pattern is `P:P`, meaning
parallel sequential rather than random, so every connection walks its own slice
of the key space and the two halves of the test touch the same keys in the same
order.

Each cell is measured twice over, once with `--ratio 1:0` for SET and once with
`--ratio 0:1` for GET, at pipeline depths of 1, 10, 25 and 50.

The number of clients is held at 256 for every point on every chart. The x axis
is the number of threads the *server* was given, not the number of clients. This
is the single most misread thing about these charts: a point at x=1 is one
server thread being hit by 256 connections, not one connection.

## What comes out

Throughput in thousands of operations per second. Latency in microseconds at
MIN, AVG, P50, P90, P99, P99.9, P99.99 and MAX, as memtier reports them. CPU
cycles per operation, from `perf stat` counting cycles on the server process
only for the duration of the measured pass, divided by the operation count that
memtier reports.

Cycles per operation is the number worth caring about most and the one hardest
to get. It is close to architecture independent, it does not move when the box
is busy in a way that throughput does, and it is the only one of the three that
says something about the engine rather than about the machine. It also needs a
real hardware PMU, which a virtual machine usually does not have.

## From 31 runs to one number

Each cell is run 31 times. The runs are sorted, 10 percent is trimmed from each
end, and four numbers are taken from what is left: the minimum, the median, the
maximum, and the mean. The trim is the defence against the one run that got
unlucky with something else on the box, and 31 runs is enough for it to work.

The original's implementation of that selection has four defects, all of which
are corrected here and all of which are reproducible with `--compat=upstream`.
They are listed in [divergences.md](../divergences.md).

Each cell also carries a standard deviation and a coefficient of variation. They
are not plotted. They exist so that a cell which was disturbed while it was being
measured can be seen to have been disturbed, which is impossible once 31 runs
have been reduced to a single number.

## What the charts are

154 charts, drawn in linear and logarithmic scale. Throughput and each of the
eight latency percentiles, for SET and for GET, at each of the four pipeline
depths, plus cycles per operation. Every chart has the same x axis, the server
thread count, and every chart is stamped with the host profile that produced it.

Charts are drawn from `output.json`, which is committed next to them. Redrawing
every chart from that file is one command and needs nothing installed but Rust.
The point of that is that nobody has to trust us. If you think the selection is
wrong, or the scale is misleading, or a colour choice is doing work it should
not, you have the data to redraw it your way.

Chart output is deterministic. The same input produces a byte identical PNG on
Linux, macOS and Windows, the fonts are embedded in the binary rather than taken
from the host, and a hash manifest in `testdata/` fails CI if that stops being
true.

## Hardware, and why it is the hardest part

The original was measured on a 32 core ARM64 AWS c8g.8xlarge, cache pinned to 16
cores, load generator pinned to the other 16. Absolute numbers from any other
machine are not comparable with the published ones, and on a different
architecture the relative ordering of two engines that are close together can
genuinely differ, because memory ordering, atomics cost and cache line behaviour
all differ.

So the rule is that comparisons are valid only within one results directory.
Charts from two profiles never go next to each other, and no chart from this
project should ever be presented beside a chart from the original.

A virtualised host generally has no PMU, which means no cycles charts from it at
all. This is not something to work around, and a cycles number derived from a
paravirtualised counter would be worse than no number. Where cycles charts and
throughput charts come from different hosts, both are stamped, and the stamp is
the only thing standing between a reader and an invalid comparison.

## What these numbers may and may not be used for

They may be used to compare these cache servers against each other, on the
stated architecture, over unix sockets, for GET and SET of 1 to 1024 byte values
at 256 connections, at the stated pipeline depths, with persistence off and no
eviction. That is a narrow claim and it is the one this harness supports.

They may not be used to say one engine is faster than another, full stop. This
workload has no expiry, no eviction, no mixed command set, no large values, no
network, no replication, no persistence and no multi key operations. It is a hot
path measurement of two commands. Wherever a number from here is published, that
sentence goes with it.
