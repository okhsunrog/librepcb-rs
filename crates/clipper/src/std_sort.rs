//! Exact replica of libstdc++'s `std::sort()` (introsort).
//!
//! `std::sort()` is not stable, and the order of elements comparing equal
//! depends on the exact algorithm. Clipper sorts its local minima and
//! intersection lists with `std::sort()` using comparators with many ties
//! (only the Y coordinate is compared), and the result order influences the
//! output (point order, polygon order). To produce bit-identical results to
//! the C++ library as built with GCC/libstdc++ (the reference platform of
//! LibrePCB), this module reproduces the libstdc++ implementation
//! (`bits/stl_algo.h` and `bits/stl_heap.h`) step by step:
//! introsort with a threshold of 16 elements, a recursion depth limit of
//! `2 * floor(log2(n))`, median-of-three pivot selection moved to the first
//! element, unguarded Hoare partitioning, heap sort fallback, and a final
//! insertion sort.
//!
//! Moves through a temporary ("hole") are replaced by swaps, which results in
//! the same element order and performs the same comparisons.

/// Threshold below which insertion sort is used (`_S_threshold`).
const THRESHOLD: usize = 16;

/// Sorts `v` exactly like libstdc++'s `std::sort(first, last, less)`.
///
/// `less(a, b)` must implement a strict weak ordering ("`a` is less than
/// `b`"), like the comparator passed to `std::sort()`. With an invalid
/// comparator the element order is unspecified, but this function never
/// panics or accesses out of bounds (unlike the C++ original).
pub fn sort_by<T>(v: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
    let n = v.len();
    if n == 0 {
        return;
    }
    introsort_loop(v, 0, n, lg(n) * 2, &mut less);
    final_insertion_sort(v, 0, n, &mut less);
}

/// `std::__lg()`: floor(log2(n)) for n > 0.
fn lg(n: usize) -> usize {
    (usize::BITS - 1 - n.leading_zeros()) as usize
}

fn introsort_loop<T>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth_limit: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    while last - first > THRESHOLD {
        if depth_limit == 0 {
            partial_sort(v, first, last, last, less);
            return;
        }
        depth_limit -= 1;
        let cut = unguarded_partition_pivot(v, first, last, less);
        introsort_loop(v, cut, last, depth_limit, less);
        last = cut;
    }
}

fn move_median_to_first<T>(
    v: &mut [T],
    result: usize,
    a: usize,
    b: usize,
    c: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    if less(&v[a], &v[b]) {
        if less(&v[b], &v[c]) {
            v.swap(result, b);
        } else if less(&v[a], &v[c]) {
            v.swap(result, c);
        } else {
            v.swap(result, a);
        }
    } else if less(&v[a], &v[c]) {
        v.swap(result, a);
    } else if less(&v[b], &v[c]) {
        v.swap(result, c);
    } else {
        v.swap(result, b);
    }
}

fn unguarded_partition<T>(
    v: &mut [T],
    mut first: usize,
    mut last: usize,
    pivot: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) -> usize {
    loop {
        // Note: The bounds checks never trigger with a valid comparator
        // (the pivot acts as sentinel), they only protect against invalid
        // comparators.
        while first < v.len() && less(&v[first], &v[pivot]) {
            first += 1;
        }
        last = last.saturating_sub(1);
        while last > 0 && less(&v[pivot], &v[last]) {
            last -= 1;
        }
        if first >= last {
            return first;
        }
        v.swap(first, last);
        first += 1;
    }
}

fn unguarded_partition_pivot<T>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) -> usize {
    let mid = first + (last - first) / 2;
    move_median_to_first(v, first, first + 1, mid, last - 1, less);
    unguarded_partition(v, first + 1, last, first, less)
}

fn partial_sort<T>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    heap_select(v, first, middle, last, less);
    sort_heap(v, first, middle, less);
}

fn heap_select<T>(
    v: &mut [T],
    first: usize,
    middle: usize,
    last: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    make_heap(v, first, middle, less);
    for i in middle..last {
        if less(&v[i], &v[first]) {
            pop_heap(v, first, middle, i, less);
        }
    }
}

fn sort_heap<T>(v: &mut [T], first: usize, mut last: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    while last - first > 1 {
        last -= 1;
        pop_heap(v, first, last, last, less);
    }
}

fn make_heap<T>(v: &mut [T], first: usize, last: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    if last - first < 2 {
        return;
    }
    let len = last - first;
    let mut parent = (len - 2) / 2;
    loop {
        // The value to insert stays at the hole position `parent`.
        adjust_heap(v, first, parent, len, less);
        if parent == 0 {
            return;
        }
        parent -= 1;
    }
}

/// `std::__pop_heap()`: moves `*first` to `*result` and re-inserts the
/// previous `*result` into the heap `[first, last)`.
fn pop_heap<T>(
    v: &mut [T],
    first: usize,
    last: usize,
    result: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    v.swap(first, result);
    adjust_heap(v, first, 0, last - first, less);
}

/// `std::__adjust_heap()` where the value to insert is stored at the hole
/// position `first + hole_index`.
fn adjust_heap<T>(
    v: &mut [T],
    first: usize,
    mut hole_index: usize,
    len: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let top_index = hole_index;
    let mut second_child = hole_index;
    while second_child < len.saturating_sub(1) / 2 {
        second_child = 2 * (second_child + 1);
        if less(&v[first + second_child], &v[first + second_child - 1]) {
            second_child -= 1;
        }
        v.swap(first + hole_index, first + second_child);
        hole_index = second_child;
    }
    if (len & 1) == 0 && second_child == (len - 2) / 2 {
        second_child = 2 * (second_child + 1);
        v.swap(first + hole_index, first + second_child - 1);
        hole_index = second_child - 1;
    }
    push_heap(v, first, hole_index, top_index, less);
}

/// `std::__push_heap()` where the value to insert is stored at the hole
/// position `first + hole_index`.
fn push_heap<T>(
    v: &mut [T],
    first: usize,
    mut hole_index: usize,
    top_index: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    while hole_index > top_index {
        let parent = (hole_index - 1) / 2;
        if !less(&v[first + parent], &v[first + hole_index]) {
            break;
        }
        v.swap(first + hole_index, first + parent);
        hole_index = parent;
    }
}

fn insertion_sort<T>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    if first == last {
        return;
    }
    for i in (first + 1)..last {
        if less(&v[i], &v[first]) {
            v[first..=i].rotate_right(1);
        } else {
            unguarded_linear_insert(v, i, less);
        }
    }
}

fn unguarded_linear_insert<T>(v: &mut [T], mut last: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    // The value to insert moves down with `last` (swaps instead of a hole).
    while last > 0 && less(&v[last], &v[last - 1]) {
        v.swap(last, last - 1);
        last -= 1;
    }
}

fn final_insertion_sort<T>(
    v: &mut [T],
    first: usize,
    last: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    if last - first > THRESHOLD {
        insertion_sort(v, first, first + THRESHOLD, less);
        for i in (first + THRESHOLD)..last {
            unguarded_linear_insert(v, i, less);
        }
    } else {
        insertion_sort(v, first, last, less);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts() {
        let mut v: Vec<i32> = (0..1000).map(|i| (i * 7919) % 1009 - 500).collect();
        let mut expected = v.clone();
        expected.sort();
        sort_by(&mut v, |a, b| a < b);
        assert_eq!(v, expected);
    }

    #[test]
    fn empty_and_single() {
        let mut v: Vec<i32> = vec![];
        sort_by(&mut v, |a, b| a < b);
        let mut v = vec![1];
        sort_by(&mut v, |a, b| a < b);
        assert_eq!(v, [1]);
    }

    #[test]
    fn unstable_order_of_ties() {
        // Keys with ties, tagged with their original position. The expected
        // order was produced by libstdc++'s std::sort (GCC).
        let keys = [3, 1, 3, 2, 1, 3, 2, 2, 1, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 2];
        let mut v: Vec<(i32, usize)> = keys.iter().copied().zip(0..).collect();
        sort_by(&mut v, |a, b| a.0 < b.0);
        let order: Vec<usize> = v.iter().map(|x| x.1).collect();
        assert_eq!(
            order,
            [
                1, 16, 13, 10, 8, 4, 6, 7, 3, 11, 14, 17, 19, 5, 9, 12, 2, 15, 0, 18
            ]
        );
    }
}
