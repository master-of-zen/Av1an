use std::iter::zip;

pub fn extra_splits(
  split_locations: Vec<usize>,
  total_frames: usize,
  split_size: usize,
) -> Vec<usize> {
  let mut result_vec: Vec<usize> = split_locations.clone();

  let mut total_length = split_locations.clone();
  total_length.insert(0, 0);
  total_length.push(total_frames);

  let iter = total_length[..total_length.len() - 1]
    .iter()
    .zip(total_length[1..].iter());

  for (x, y) in iter {
    let distance = y - x;
    if distance > split_size {
      let additional_splits = (distance / split_size) + 1;
      for n in 1..additional_splits {
        let new_split = (distance as f64 * (n as f64 / additional_splits as f64)) as usize + x;
        result_vec.push(new_split);
      }
    }
  }

  result_vec.sort();

  result_vec
}
