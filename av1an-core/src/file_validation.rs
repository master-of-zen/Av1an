use std::path::PathBuf;
use std::process::exit;

/// Returns file if it have suffix of media file
fn match_file_type(input: PathBuf) -> bool {
  ["mkv", "mp4", "mov", "avi", "flv", "m2ts", "y4m"]
    .iter()
    .any(|&v| input.extension().map_or(false, |u| v == u))
}

fn validate_files(files: Vec<PathBuf>) -> Vec<PathBuf> {
  let valid: Vec<PathBuf> = files
    .iter()
    .cloned()
    .filter(|x| x.as_path().exists())
    .collect();
  valid
}

/// Process given input file/dir
/// Returns vector of files to process
pub fn process_inputs(inputs: Vec<PathBuf>) -> Vec<PathBuf> {
  if inputs.is_empty() {
    println!("No inputs");
    exit(0);
  }

  let mut input_files: Vec<PathBuf> = vec![];

  // Process all inputs (folders and files)
  // into single path vector
  for fl in &inputs {
    if fl.as_path().is_dir() {
      for file in fl.as_path().read_dir().unwrap() {
        let entry = file.unwrap();
        let path_file = entry.path();
        input_files.push(path_file);
      }
    } else {
      input_files.push(fl.to_path_buf());
    }
  }

  // Check are all files real
  let valid_input = validate_files(input_files);
  // Match files to media file extensions
  let result: Vec<PathBuf> = valid_input
    .iter()
    .cloned()
    .filter(|x| match_file_type(x.to_path_buf()))
    .collect();

  if result.is_empty() {
    println!("No valid inputs");
    println!("{:#?}", &inputs);
    exit(1);
  }

  result
}

#[cfg(test)]
mod tests {
  use super::*;

  use std::fs::{remove_file, File};

  #[test]
  fn test_match_file_type() {
    assert_eq!(match_file_type(PathBuf::from("input.mkv")), true);
    assert_eq!(match_file_type(PathBuf::from("picture.png")), false);
  }

  #[test]
  fn test_validate_files() {
    // Create dummy files
    File::create("dummy_1.mkv").unwrap();
    File::create("dummy_2.txt").unwrap();
    File::create("dummy_3.jpeg").unwrap();

    let files = vec![
      PathBuf::from("dummy_1.mkv"),
      PathBuf::from("dummy_2.txt"),
      PathBuf::from("dummy_3.jpeg"),
      PathBuf::from("dummy_4.404"),
    ];

    let mut valid_files = files.clone();
    valid_files.pop();

    let validated = validate_files(files);

    // Remove dummy files
    remove_file("dummy_1.mkv").unwrap();
    remove_file("dummy_2.txt").unwrap();
    remove_file("dummy_3.jpeg").unwrap();
    assert_eq!(validated, valid_files)
  }
}
