use std::collections::HashMap;
use std::env;
use std::fmt::Debug;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use once_cell::sync::OnceCell;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

// Store the worker guard globally
static WORKER_GUARD: OnceCell<WorkerGuard> = OnceCell::new();
pub const DEFAULT_CONSOLE_LEVEL: LevelFilter = LevelFilter::INFO;
pub const DEFAULT_LOG_LEVEL: LevelFilter = LevelFilter::DEBUG;

// Define our module configuration structure
#[derive(Debug, Clone)]
struct ModuleConfig {
  console_level: LevelFilter,
  file_level: LevelFilter,
  console_enabled: bool,
  file_enabled: bool,
}

impl Default for ModuleConfig {
  fn default() -> Self {
    Self {
      console_level: DEFAULT_CONSOLE_LEVEL,
      file_level: DEFAULT_LOG_LEVEL,
      console_enabled: true,
      file_enabled: true,
    }
  }
}

/// Initialize logging with per-module configuration
pub fn init_logging(console_level: LevelFilter, log_path: PathBuf, file_level: LevelFilter) {
  // Set up our module configurations
  let mut module_configs = HashMap::new();

  // Configure core module
  module_configs.insert(
    "av1an_core",
    ModuleConfig {
      console_level,
      file_level,
      console_enabled: true,
      file_enabled: true,
    },
  );

  // Configure scene detection module
  module_configs.insert(
    "av1an_core::scene_detect",
    ModuleConfig {
      console_level,
      file_level,
      console_enabled: true,
      file_enabled: true,
    },
  );

  // Allow override through environment variables
  if let Ok(rust_log) = env::var("RUST_LOG") {
    for directive in rust_log.split(',') {
      if let Some((module, level)) = directive.split_once('=') {
        if let (Some(config), Ok(level)) =
          (module_configs.get_mut(module), level.parse::<LevelFilter>())
        {
          config.console_level = level;
          config.file_level = level;
        }
      }
    }
  }

  // Create our filters
  let console_filter = {
    let mut filter = String::new();
    for (module, config) in &module_configs {
      if config.console_enabled {
        if !filter.is_empty() {
          filter.push(',');
        }
        filter.push_str(&format!("{}={}", module, config.console_level));
      }
    }
    EnvFilter::try_new(&filter).unwrap()
  };

  let file_filter = {
    let mut filter = String::new();
    for (module, config) in &module_configs {
      if config.file_enabled {
        if !filter.is_empty() {
          filter.push(',');
        }
        filter.push_str(&format!("{}={}", module, config.file_level));
      }
    }
    EnvFilter::try_new(&filter).unwrap()
  };

  // Set up file appender
  let file_appender = if log_path.parent().unwrap_or_else(|| Path::new("")) == Path::new("")
    && log_path.file_name().unwrap() == "av1an.log"
  {
    RollingFileAppender::new(Rotation::DAILY, "logs", "av1an.log")
  } else {
    RollingFileAppender::new(
      Rotation::NEVER,
      Path::new("logs").join(log_path.parent().unwrap_or_else(|| Path::new(""))),
      log_path.file_name().unwrap(),
    )
  };

  let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
  WORKER_GUARD
    .set(guard)
    .expect("Failed to store worker guard");

  // Create our subscriber with correctly ordered layers
  let subscriber = tracing_subscriber::registry()
    // File output layer
    .with(
      fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_level(true)
        .with_writer(non_blocking)
        // Apply the filter last
        .with_filter(file_filter),
    )
    // Console output layer
    .with(
      fmt::layer()
        .compact()
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .with_file(false)
        .without_time()
        .with_writer(std::io::stderr)
        // Apply the filter last
        .with_filter(console_filter),
    );

  // Set as global default
  tracing::subscriber::set_global_default(subscriber)
    .expect("Failed to set global default subscriber");

  // Log initialization
  tracing::debug!("Logging system initialized");
  tracing::debug!("Module-specific logging enabled");
}
