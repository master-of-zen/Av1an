use std::error::Error;

use vergen::EmitBuilder;

fn main() -> Result<(), Box<dyn Error>> {
  EmitBuilder::builder()
    .git_sha(true)
    .git_commit_date()
    .cargo_debug()
    .cargo_target_triple()
    .rustc_semver()
    .rustc_llvm_version()
    .emit()?;
  Ok(())
}
