package main

import (
	"encoding/json"
	"fmt"
	"math/rand"
	"os"
	"sort"
)

type Case struct {
	Name  string  `json:"name"`
	Input []int64 `json:"input"`
	// When set, the comparator reads this slice at the two positions it is
	// given instead of the slice it is permuting. That is the original's D2.
	Alias []int64 `json:"alias,omitempty"`
	Desc  bool    `json:"desc"`
	Want  []int64 `json:"want"`
}

func run(name string, in []int64, alias []int64, desc bool) Case {
	work := append([]int64(nil), in...)
	if alias != nil {
		sort.Slice(work, func(i, j int) bool {
			if desc {
				return alias[i] > alias[j]
			}
			return alias[i] < alias[j]
		})
	} else {
		sort.Slice(work, func(i, j int) bool {
			if desc {
				return work[i] > work[j]
			}
			return work[i] < work[j]
		})
	}
	return Case{Name: name, Input: in, Alias: alias, Desc: desc, Want: work}
}

func main() {
	r := rand.New(rand.NewSource(20260904))
	var cases []Case

	seq := func(n int, f func(i int) int64) []int64 {
		out := make([]int64, n)
		for i := range out {
			out[i] = f(i)
		}
		return out
	}

	// Every length up to 64, random values, which walks insertion sort, median
	// of three, the ninther and the partition paths.
	for n := 0; n <= 64; n++ {
		cases = append(cases, run(fmt.Sprintf("random_%d", n), seq(n, func(int) int64 { return int64(r.Intn(1000)) }), nil, false))
	}

	for _, n := range []int{13, 31, 50, 51, 100, 257, 400} {
		cases = append(cases,
			run(fmt.Sprintf("sorted_%d", n), seq(n, func(i int) int64 { return int64(i) }), nil, false),
			run(fmt.Sprintf("reversed_%d", n), seq(n, func(i int) int64 { return int64(n - i) }), nil, false),
			run(fmt.Sprintf("equal_%d", n), seq(n, func(int) int64 { return 7 }), nil, false),
			run(fmt.Sprintf("fewdistinct_%d", n), seq(n, func(int) int64 { return int64(r.Intn(3)) }), nil, false),
			run(fmt.Sprintf("organpipe_%d", n), seq(n, func(i int) int64 {
				if i < n/2 {
					return int64(i)
				}
				return int64(n - i)
			}), nil, false),
			run(fmt.Sprintf("sawtooth_%d", n), seq(n, func(i int) int64 { return int64(i % 17) }), nil, false),
			run(fmt.Sprintf("desc_%d", n), seq(n, func(int) int64 { return int64(r.Intn(1000)) }), nil, true),
		)
	}

	// The aliased comparator, at the four lengths the original's mutated run
	// count produces, plus a few more for good measure.
	for _, n := range []int{17, 21, 25, 31, 13, 64, 200} {
		in := seq(n, func(i int) int64 { return int64(i) * 100 })
		for k := 0; k < 3; k++ {
			alias := seq(n, func(int) int64 { return int64(r.Intn(1000000)) })
			cases = append(cases, run(fmt.Sprintf("aliased_%d_%d", n, k), in, alias, true))
		}
		flat := seq(n, func(int) int64 { return 0 })
		cases = append(cases, run(fmt.Sprintf("aliased_flat_%d", n), in, flat, true))
	}

	out, _ := json.Marshal(cases)
	os.Stdout.Write(out)
}
