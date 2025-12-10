use super::*;

fn assert_intersect_sorted(a: &[i32], b: &[i32], expected: &[i32]) {
    // Owning version
    let owned = intersect_sorted(a, b);
    assert_eq!(owned, expected, "intersect_sorted({:?}, {:?})", a, b);

    // Into version
    let mut out = Vec::new();
    intersect_sorted_into(a, b, &mut out);
    assert_eq!(out, expected, "intersect_sorted_into({:?}, {:?})", a, b);

    assert_eq!(owned, out);
}

#[test]
fn intersect_sorted_basic_cases() {
    // Both empty
    assert_intersect_sorted(&[], &[], &[]);

    // One empty
    assert_intersect_sorted(&[], &[1, 2, 3], &[]);
    assert_intersect_sorted(&[1, 2, 3], &[], &[]);

    // Disjoint
    assert_intersect_sorted(&[1, 3, 5], &[2, 4, 6], &[]);

    // Simple overlap
    assert_intersect_sorted(&[1, 2, 3], &[2, 3, 4], &[2, 3]);

    // Identical slices
    assert_intersect_sorted(&[1, 2, 3], &[1, 2, 3], &[1, 2, 3]);

    // Duplicates: min(count_a, count_b) should appear
    assert_intersect_sorted(&[1, 1, 2, 2, 2, 3], &[1, 2, 2, 4], &[1, 2, 2]);
}

#[test]
fn intersect_sorted_into_reuses_buffer() {
    let a = [1, 2, 3, 4];
    let b = [2, 4, 6];

    let mut out = Vec::new();
    intersect_sorted_into(&a, &b, &mut out);
    assert_eq!(out, vec![2, 4]);

    // Call again with different inputs and ensure clear() + refill works
    let c = [1, 3, 5, 7];
    let d = [2, 3, 4, 5, 6];
    intersect_sorted_into(&c, &d, &mut out);
    assert_eq!(out, vec![3, 5]);
}

#[test]
fn intersect_adaptive_matches_sorted_for_balanced_sizes() {
    let a = [1, 2, 3, 4, 5];
    let b = [3, 4, 5, 6, 7];

    let sorted = intersect_sorted(&a, &b);
    let adaptive = intersect_adaptive(&a, &b);
    assert_eq!(sorted, adaptive);

    let mut out = Vec::new();
    intersect_adaptive_into(&a, &b, &mut out);
    assert_eq!(out, sorted);
}

#[test]
fn intersect_adaptive_uses_galloping_path_for_skewed_sizes_and_matches_sorted() {
    // a is tiny, b is large; ratio > 8x to trigger galloping
    let small = [3, 10, 50];
    let mut large = Vec::new();
    for i in 0..1000 {
        large.push(i);
    }

    // Ground truth via linear merge
    let baseline = intersect_sorted(&small, &large);

    // Adaptive (should choose galloping)
    let adaptive = intersect_adaptive(&small, &large);
    assert_eq!(adaptive, baseline);

    // into variant
    let mut out = Vec::new();
    intersect_adaptive_into(&small, &large, &mut out);
    assert_eq!(out, baseline);
}

#[test]
fn union_sorted_basic_cases() {
    assert_eq!(union_sorted::<i32>(&[], &[]), Vec::<i32>::new());

    assert_eq!(union_sorted(&[], &[1, 2, 3]), vec![1, 2, 3]);
    assert_eq!(union_sorted(&[1, 2, 3], &[]), vec![1, 2, 3]);

    assert_eq!(union_sorted(&[1, 3, 5], &[2, 4, 6]), vec![1, 2, 3, 4, 5, 6]);

    assert_eq!(
        union_sorted(&[1, 2, 2, 3], &[2, 3, 3, 4]),
        vec![1, 2, 2, 3, 3, 4]
    );
}

#[test]
fn diff_sorted_basic_cases() {
    // Both empty
    assert_eq!(diff_sorted::<i32>(&[], &[]), Vec::<i32>::new());

    // Sub empty: full base
    assert_eq!(diff_sorted(&[1, 2, 3], &[]), vec![1, 2, 3]);

    // Base empty: nothing
    assert_eq!(diff_sorted(&[], &[1, 2, 3]), Vec::<i32>::new());

    // Simple difference
    assert_eq!(diff_sorted(&[1, 2, 3, 4, 5], &[2, 4]), vec![1, 3, 5]);

    // Sub has elements not in base
    assert_eq!(diff_sorted(&[1, 3, 5], &[2, 3, 4, 6]), vec![1, 5]);

    // Duplicates in base and sub
    assert_eq!(diff_sorted(&[1, 1, 2, 2, 3], &[1, 2]), vec![1, 2, 3]);
}

#[test]
fn generics_work_for_non_integers() {
    let a = ['a', 'b', 'c', 'd'];
    let b = ['b', 'd', 'f'];

    let inter = intersect_sorted(&a, &b);
    assert_eq!(inter, vec!['b', 'd']);

    let uni = union_sorted(&a, &b);
    assert_eq!(uni, vec!['a', 'b', 'c', 'd', 'f']);

    let diff = diff_sorted(&a, &b);
    assert_eq!(diff, vec!['a', 'c']);
}
