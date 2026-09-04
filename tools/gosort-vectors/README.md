# gosort-vectors

Produces `crates/cb-core/golden/gosort.json`, which is what `cb-stats::gosort` is tested against.

Upstream mode has to reproduce the original's published SET numbers. Those come out of a sort whose comparator reads a slice it is not sorting, so the order that results is a property of the algorithm rather than of the data, and matching it means matching Go's `pdqsort` exactly. The only way to know a port does that is to ask Go.

```
cd tools/gosort-vectors
go run . > ../../crates/cb-core/golden/gosort.json
```

It needs Go and nothing else. There is no `go.mod` here on purpose, because this is not part of the build and nothing in CI runs it. The output is committed, and regenerating it should not be necessary: a fixture that moves is not a fixture.

The cases cover every length up to 64, the shapes that push a quicksort into its slow paths, and the aliased comparator at the four lengths the original's mutated run count produces.
