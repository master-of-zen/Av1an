use std::env;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize the logging system for the application.
///
/// This function sets up tracing with the following features:
/// - Reads log level from the RUST_LOG environment variable (defaults to "info")
/// - Enables logging to both console and a file
/// - Uses daily log rotation for file logging
/// - Logs the duration of each span
/// - Includes file and line numbers in log messages
///
/// # Panics
///
/// This function will panic if it fails to initialize the global logger.
pub fn init_logging() {
    let rust_log = env::var("RUST_LOG").unwrap_or_else(|_| "INFO".to_string());

    // Set up daily rotating file appender
    let file_appender = RollingFileAppender::new(Rotation::DAILY, "logs", "application.log");

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new(rust_log))
        .with(
            fmt::Layer::new()
                .with_writer(std::io::stdout)
                .with_ansi(true)
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true),
        )
        .with(
            fmt::Layer::new()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true),
        );

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global default subscriber");

    tracing::info!("Logging initialized with daily rotation");
}