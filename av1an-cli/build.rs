use vergen::{vergen, Config, ShaKind, TimestampKind};

fn main() {
  let mut config = Config::default();

  *config.git_mut().sha_kind_mut() = ShaKind::Short;
  *config.git_mut().commit_timestamp_kind_mut() = TimestampKind::All;
  *config.build_mut().kind_mut() = TimestampKind::All;

  vergen(config).unwrap();
}
