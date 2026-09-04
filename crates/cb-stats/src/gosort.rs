//! Go's `sort.Slice`, ported.
//!
//! Upstream mode needs this and nothing else does. The original sorts its SET results with a comparator that reads a different slice, so the order that comes out is not a property of the values being sorted. It is a property of the exact sequence of comparisons and swaps the sort performs, which means reproducing the original's published SET numbers means reproducing Go's sort itself rather than merely sorting the same data.
//!
//! This is `pdqsort` as it appears in `src/sort/zsortfunc.go`, function for function, down to the constants and the two places the Go code is arguably wrong. `partialInsertionSort` walks below the start of its own range, and `breakPatterns` uses a seed derived from the length. Both are load bearing here.
//!
//! The comparator takes positions rather than elements, exactly as `sort.Slice` does, which is the only way a comparator can read a slice it is not sorting.
//!
//! Verified against the real thing. `testdata/golden/gosort.json` holds 142 cases produced by Go, covering every length to 64, the shapes that drive a quicksort into its slow paths, and the aliased comparator at the four lengths the original's mutated run count produces.

/// A slice being sorted, and how to compare and swap two positions in it.
///
/// This is Go's `lessSwap` pair. Keeping the comparison and the swap on the same object is what lets the comparison read something other than the data being permuted, which is the whole reason this module exists.
pub trait LessSwap {
    /// Whether the element at `i` sorts before the element at `j`.
    fn less(&mut self, i: usize, j: usize) -> bool;
    /// Exchange two positions.
    fn swap(&mut self, i: usize, j: usize);
}

/// Sort by key, which is the ordinary case.
pub struct ByKey<'a, T, F> {
    data: &'a mut [T],
    key: F,
}

impl<'a, T, F: Fn(&T) -> f64> ByKey<'a, T, F> {
    /// Compare on a key read out of each element.
    pub fn new(data: &'a mut [T], key: F) -> Self {
        Self { data, key }
    }
}

impl<T, F: Fn(&T) -> f64> LessSwap for ByKey<'_, T, F> {
    fn less(&mut self, i: usize, j: usize) -> bool {
        (self.key)(&self.data[i]) < (self.key)(&self.data[j])
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.data.swap(i, j);
    }
}

/// Sort one slice while comparing another, which is the original's third sort.
///
/// The comparator reads `other` at the positions it is handed, and those positions index a slice that is being permuted underneath it. There is no ordering this converges on. Which element ends up where depends on the order the algorithm asks its questions in, and that is the point.
pub struct Aliased<'a, T, U, F> {
    data: &'a mut [T],
    other: &'a [U],
    key: F,
}

impl<'a, T, U, F: Fn(&U) -> i64> Aliased<'a, T, U, F> {
    /// Permute `data`, compare `other`, descending, which is what `choose` does.
    pub fn new(data: &'a mut [T], other: &'a [U], key: F) -> Self {
        Self { data, other, key }
    }
}

impl<T, U, F: Fn(&U) -> i64> LessSwap for Aliased<'_, T, U, F> {
    fn less(&mut self, i: usize, j: usize) -> bool {
        (self.key)(&self.other[i]) > (self.key)(&self.other[j])
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.data.swap(i, j);
    }
}

/// `sort.Slice`.
///
/// The entry point Go exposes, including its choice of recursion limit.
pub fn slice(data: &mut impl LessSwap, length: usize) {
    let limit = usize::BITS - length.leading_zeros();
    pdqsort(data, 0, length, limit as usize);
}

/// What `choosePivot` learned about the range on its way past.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hint {
    Unknown,
    Increasing,
    Decreasing,
}

/// `pdqsort`, sorting `data[a..b]`.
///
/// `limit` is how many badly unbalanced pivots are tolerated before giving up and heapsorting, which is what keeps the worst case out of quadratic time.
fn pdqsort(data: &mut impl LessSwap, mut a: usize, mut b: usize, mut limit: usize) {
    const MAX_INSERTION: usize = 12;

    let mut was_balanced = true;
    let mut was_partitioned = true;

    loop {
        let length = b - a;

        if length <= MAX_INSERTION {
            insertion_sort(data, a, b);
            return;
        }

        if limit == 0 {
            heap_sort(data, a, b);
            return;
        }

        if !was_balanced {
            break_patterns(data, a, b);
            limit -= 1;
        }

        let (mut pivot, mut hint) = choose_pivot(data, a, b);
        if hint == Hint::Decreasing {
            reverse_range(data, a, b);
            // The pivot was that far after the start, so after reversing it is that far before the end.
            pivot = (b - 1) - (pivot - a);
            hint = Hint::Increasing;
        }

        // Likely already sorted, so try to finish it off cheaply.
        if was_balanced
            && was_partitioned
            && hint == Hint::Increasing
            && partial_insertion_sort(data, a, b)
        {
            return;
        }

        // Probably many duplicates, so partition into equal and greater instead.
        if a > 0 && !data.less(a - 1, pivot) {
            a = partition_equal(data, a, b, pivot);
            continue;
        }

        let (mid, already_partitioned) = partition(data, a, b, pivot);
        was_partitioned = already_partitioned;

        let (left_len, right_len) = (mid - a, b - mid);
        let balance_threshold = length / 8;
        if left_len < right_len {
            was_balanced = left_len >= balance_threshold;
            pdqsort(data, a, mid, limit);
            a = mid + 1;
        } else {
            was_balanced = right_len >= balance_threshold;
            pdqsort(data, mid + 1, b, limit);
            b = mid;
        }
    }
}

/// `insertionSort`.
fn insertion_sort(data: &mut impl LessSwap, a: usize, b: usize) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && data.less(j, j - 1) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}

/// `siftDown`, keeping the heap property on `data[lo..hi]` with the root at `first`.
fn sift_down(data: &mut impl LessSwap, lo: usize, hi: usize, first: usize) {
    let mut root = lo;
    loop {
        let mut child = 2 * root + 1;
        if child >= hi {
            break;
        }
        if child + 1 < hi && data.less(first + child, first + child + 1) {
            child += 1;
        }
        if !data.less(first + root, first + child) {
            return;
        }
        data.swap(first + root, first + child);
        root = child;
    }
}

/// `heapSort`, the fallback when too many pivots went badly.
fn heap_sort(data: &mut impl LessSwap, a: usize, b: usize) {
    let first = a;
    let lo = 0;
    let hi = b - a;
    if hi == 0 {
        return;
    }

    // Build the heap with the greatest element on top.
    // Go counts down through zero on a signed int, so both loops here run once more after reaching zero and then stop.
    let mut i = (hi - 1) / 2;
    loop {
        sift_down(data, i, hi, first);
        if i == 0 {
            break;
        }
        i -= 1;
    }

    // Pop them off, largest first, into the end.
    let mut i = hi - 1;
    loop {
        data.swap(first, first + i);
        sift_down(data, lo, i, first);
        if i == 0 {
            break;
        }
        i -= 1;
    }
}

/// `partition`, one quicksort partition around `data[pivot]`.
fn partition(data: &mut impl LessSwap, a: usize, b: usize, pivot: usize) -> (usize, bool) {
    data.swap(a, pivot);
    // Both ends are inclusive of what is left to partition. `j` never falls below `a`, because it only moves while `i <= j` and `i` starts one past `a`.
    let (mut i, mut j) = (a + 1, b - 1);

    while i <= j && data.less(i, a) {
        i += 1;
    }
    while i <= j && !data.less(j, a) {
        j -= 1;
    }
    if i > j {
        data.swap(j, a);
        return (j, true);
    }
    data.swap(i, j);
    i += 1;
    j -= 1;

    loop {
        while i <= j && data.less(i, a) {
            i += 1;
        }
        while i <= j && !data.less(j, a) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i, j);
        i += 1;
        j -= 1;
    }
    data.swap(j, a);
    (j, false)
}

/// `partitionEqual`, splitting into elements equal to the pivot and elements above it.
fn partition_equal(data: &mut impl LessSwap, a: usize, b: usize, pivot: usize) -> usize {
    data.swap(a, pivot);
    let (mut i, mut j) = (a + 1, b - 1);

    loop {
        while i <= j && !data.less(a, i) {
            i += 1;
        }
        while i <= j && data.less(a, j) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i, j);
        i += 1;
        j -= 1;
    }
    i
}

/// `partialInsertionSort`, which returns whether the range came out sorted.
///
/// The inner loops stop at index 1 rather than at `a`, so on a subrange this shifts elements that are not in the range it was asked about. That is Go's, not a transcription slip, and it moves data in cases this port has to match.
fn partial_insertion_sort(data: &mut impl LessSwap, a: usize, b: usize) -> bool {
    const MAX_STEPS: usize = 5;
    const SHORTEST_SHIFTING: usize = 50;

    let mut i = a + 1;
    for _ in 0..MAX_STEPS {
        while i < b && !data.less(i, i - 1) {
            i += 1;
        }

        if i == b {
            return true;
        }

        if b - a < SHORTEST_SHIFTING {
            return false;
        }

        data.swap(i, i - 1);

        // Shift the smaller one left.
        if i - a >= 2 {
            let mut j = i - 1;
            while j >= 1 {
                if !data.less(j, j - 1) {
                    break;
                }
                data.swap(j, j - 1);
                j -= 1;
            }
        }
        // Shift the greater one right.
        if b - i >= 2 {
            let mut j = i + 1;
            while j < b {
                if !data.less(j, j - 1) {
                    break;
                }
                data.swap(j, j - 1);
                j += 1;
            }
        }
    }
    false
}

/// `breakPatterns`, scattering three elements to spoil an input built to be slow.
///
/// The seed is the length, so this is deterministic. A sort that reached for a real random number here would not be reproducible and neither would the original's output.
fn break_patterns(data: &mut impl LessSwap, a: usize, b: usize) {
    let length = b - a;
    if length >= 8 {
        let mut random = Xorshift(length as u64);
        let modulus = next_power_of_two(length);

        for idx in a + (length / 4) * 2 - 1..=a + (length / 4) * 2 + 1 {
            // The mask is one below a power of two that is at most twice the length, so the result always fits.
            let mut other = usize::try_from(random.next() & (modulus - 1)).unwrap_or(0);
            if other >= length {
                other -= length;
            }
            data.swap(idx, a + other);
        }
    }
}

/// `choosePivot`.
///
/// Under 8 elements it takes a fixed position, under 50 the median of three, and above that the median of three medians of three. The swap count on the way through is what the hints are read from.
fn choose_pivot(data: &mut impl LessSwap, a: usize, b: usize) -> (usize, Hint) {
    const SHORTEST_NINTHER: usize = 50;
    const MAX_SWAPS: usize = 4 * 3;

    let len = b - a;

    // Three samples at the quarter points. Go calls them i, j and k, and the middle one is the one that comes back.
    let mut swaps = 0;
    let mut first = a + len / 4;
    let mut mid = a + (len / 4) * 2;
    let mut last = a + (len / 4) * 3;

    if len >= 8 {
        if len >= SHORTEST_NINTHER {
            first = median_adjacent(data, first, &mut swaps);
            mid = median_adjacent(data, mid, &mut swaps);
            last = median_adjacent(data, last, &mut swaps);
        }
        mid = median(data, first, mid, last, &mut swaps);
    }

    match swaps {
        0 => (mid, Hint::Increasing),
        MAX_SWAPS => (mid, Hint::Decreasing),
        _ => (mid, Hint::Unknown),
    }
}

/// `order2`, returning the pair in order and counting the exchange.
fn order2(data: &mut impl LessSwap, a: usize, b: usize, swaps: &mut usize) -> (usize, usize) {
    if data.less(b, a) {
        *swaps += 1;
        return (b, a);
    }
    (a, b)
}

/// `median`, the middle of three by position.
fn median(data: &mut impl LessSwap, a: usize, b: usize, c: usize, swaps: &mut usize) -> usize {
    let (a, b) = order2(data, a, b, swaps);
    let (b, c) = order2(data, b, c, swaps);
    let (_, b) = order2(data, a, b, swaps);
    let _ = c;
    b
}

/// `medianAdjacent`, the median of an element and its two neighbours.
fn median_adjacent(data: &mut impl LessSwap, a: usize, swaps: &mut usize) -> usize {
    median(data, a - 1, a, a + 1, swaps)
}

/// `reverseRange`.
fn reverse_range(data: &mut impl LessSwap, a: usize, b: usize) {
    let (mut i, mut j) = (a, b - 1);
    while i < j {
        data.swap(i, j);
        i += 1;
        j -= 1;
    }
}

/// Go's `xorshift`, seeded from the length.
struct Xorshift(u64);

impl Xorshift {
    /// The next value, wrapping exactly as the Go original does.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Go's `nextPowerOfTwo`, which returns twice the length when the length is already a power of two.
fn next_power_of_two(length: usize) -> u64 {
    1 << (usize::BITS - length.leading_zeros())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde::Deserialize;

    use super::{Aliased, ByKey, slice};

    /// One case, as Go produced it.
    ///
    /// The lists are optional because Go writes an empty slice as `null`, and the zero length case is one worth keeping.
    #[derive(Deserialize)]
    struct Case {
        name: String,
        input: Option<Vec<i64>>,
        #[serde(default)]
        alias: Option<Vec<i64>>,
        desc: bool,
        want: Option<Vec<i64>>,
    }

    #[allow(clippy::expect_used)]
    fn cases() -> Vec<Case> {
        serde_json::from_str(include_str!("../../../testdata/golden/gosort.json"))
            .expect("the committed sort vectors parse")
    }

    // The whole of the point. Every one of these came out of Go's sort.Slice, and any difference in the algorithm shows up as a different permutation rather than as a different set of values, so this catches transcription slips that a sortedness check would sail past.
    #[test]
    fn every_case_matches_go_exactly() {
        let cases = cases();
        assert_eq!(cases.len(), 142);
        for case in cases {
            let mut got = case.input.clone().unwrap_or_default();
            let n = got.len();
            if let Some(alias) = &case.alias {
                assert!(case.desc, "{}: the aliased cases are descending", case.name);
                let mut data = Aliased::new(&mut got, alias.as_slice(), |v: &i64| *v);
                slice(&mut data, n);
            } else if case.desc {
                #[allow(clippy::cast_precision_loss)]
                let mut data = ByKey::new(&mut got, |v: &i64| -(*v as f64));
                slice(&mut data, n);
            } else {
                #[allow(clippy::cast_precision_loss)]
                let mut data = ByKey::new(&mut got, |v: &i64| *v as f64);
                slice(&mut data, n);
            }
            assert_eq!(got, case.want.unwrap_or_default(), "{}", case.name);
        }
    }

    // A comparator that reads a slice it is not sorting does not produce a sorted result, and if these came out sorted the fixtures would be proving nothing.
    #[test]
    fn the_aliased_cases_do_not_come_out_sorted() {
        let scrambled = cases()
            .iter()
            .filter(|c| c.alias.is_some() && c.input.as_ref().is_some_and(|i| i.len() > 12))
            .filter_map(|c| c.want.as_ref())
            .filter(|want| !want.is_sorted() && !want.iter().rev().is_sorted())
            .count();
        assert!(scrambled >= 15, "only {scrambled} were scrambled");
    }

    // A flat alias makes the comparator false for every pair, which is the cell measured without perf.
    // Go leaves an order it cannot distinguish alone, and that is why one half of the original's matrix escapes D2.
    #[test]
    fn a_comparator_that_is_always_false_leaves_the_order_alone() {
        for case in cases()
            .iter()
            .filter(|c| c.name.starts_with("aliased_flat"))
        {
            assert_eq!(case.want, case.input, "{}", case.name);
        }
    }
}
