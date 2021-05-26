use std::io::prelude::*;
use std::path::{is_separator, Path, PathBuf};
use std::process::{exit, Command, Stdio};
use std::{
  fs::{remove_file, File},
  path,
};

/// Returns file if it have suffix of media file
fn match_file_type(input: &Path) -> bool {
  let extension = input.extension().unwrap().to_str().unwrap();

  if ["mkv", "mp4", "mov", "avi", "flv", "m2ts"]
    .iter()
    .any(|&v| input.extension().map_or(false, |u| v == u))
  {
    true
  } else {
    false
  }
}

fn validate_files(files: Vec<&Path>) -> Vec<&Path> {
  let valid: Vec<&Path> = files.iter().cloned().filter(|x| x.exists()).collect();
  valid
}

/// Process given input file/dir
/// Returns vector of files to process
fn process_inputs(inputs: Vec<&Path>) -> Vec<&Path> {
  if inputs.is_empty() {
    println!("No inputs");
    exit(0);
  }

  let mut input_files: Vec<&Path> = vec![];

  // Process all inputs (folders and files)
  // into single path vector
  for fl in inputs.clone() {
    if fl.is_dir() {
      for file in fl {
        let path_file = Path::new(file);
        input_files.push(path_file);
      }
    } else {
      input_files.push(fl);
    }
  }

  // Check are all files real
  let valid_input = validate_files(input_files);

  // Match files to media file extensions
  let result: Vec<&Path> = valid_input
    .iter()
    .cloned()
    .filter(|x| match_file_type(*x))
    .collect();

  if result.is_empty() {
    println!("Not valid inputs");
    println!("{:#?}", inputs);
    exit(1);
  }

  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_match_file_type_true() {
    let file = Path::new("input.mkv");

    assert_eq!(match_file_type(file), true)
  }

  #[test]
  fn test_match_file_type_false() {
    let file = Path::new("picture.png");

    assert_eq!(match_file_type(file), false)
  }

  #[test]
  fn test_validate_files() {
    // Create dummy files
    File::create("dummy_1.mkv").unwrap();
    File::create("dummy_2.txt").unwrap();
    File::create("dummy_3.jpeg").unwrap();

    let files = vec![
      Path::new("dummy_1.mkv"),
      Path::new("dummy_2.txt"),
      Path::new("dummy_3.jpeg"),
      Path::new("dummy_4.404"),
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
