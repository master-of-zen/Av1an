use av1an_cli::run;
use std::panic;
use std::process;

fn main() -> anyhow::Result<()> {
  let orig_hook = panic::take_hook();
  // Catch panics in child threads
  panic::set_hook(Box::new(move |panic_info| {
    orig_hook(panic_info);
    process::exit(1);
  }));
  run()
}
