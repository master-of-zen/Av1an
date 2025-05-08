use crate::target_quality::lagrange_bisect;

#[test]
fn test_bisect() {
  let sorted = vec![(0, 0.0), (1, 1.0), (256, 256.0 * 256.0)];

  assert!(lagrange_bisect(&sorted, 0.0).0 == 0);
  assert!(lagrange_bisect(&sorted, 1.0).0 == 1);
  assert!(lagrange_bisect(&sorted, 256.0 * 256.0).0 == 256);

  assert!(lagrange_bisect(&sorted, 8.0).0 == 3);
  assert!(lagrange_bisect(&sorted, 9.0).0 == 3);

  assert!(lagrange_bisect(&sorted, -1.0).0 == 0);
  assert!(lagrange_bisect(&sorted, 2.0 * 256.0 * 256.0).0 == 256);
}
