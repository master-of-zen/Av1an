#[macro_export]
macro_rules! regex {
  ($re:literal $(,)?) => {{
    static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
    RE.get_or_init(|| regex::Regex::new($re).unwrap())
  }};
}

#[macro_export]
macro_rules! into_vec {
  ($($x:expr),* $(,)?) => {
    vec![
      $(
        $x.into(),
      )*
    ]
  };
}

#[macro_export]
macro_rules! ref_vec {
  ($t:ty, [$($x:expr),* $(,)?]$(,)?) => {
    vec![
      $(
        AsRef::<$t>::as_ref($x),
      )*
    ]
  };
}

#[macro_export]
macro_rules! into_array {
  ($($x:expr),* $(,)?) => {
    [
      $(
        $x.into(),
      )*
    ]
  };
}

/// Attempts to create the directory if it does not exist, logging and returning
/// and error if creating the directory failed.
#[macro_export]
macro_rules! create_dir {
  ($loc:expr) => {
    match std::fs::create_dir(&$loc) {
      Ok(_) => Ok(()),
      Err(e) => match e.kind() {
        std::io::ErrorKind::AlreadyExists => Ok(()),
        _ => {
          error!("Error while creating directory {:?}: {}", &$loc, e);
          Err(e)
        }
      },
    }
  };
}

#[inline]
pub(crate) fn printable_base10_digits(x: usize) -> u32 {
  (((x as f64).log10() + 1.0).floor() as u32).max(1)
}
