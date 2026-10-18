//! The shared driver behind every `info`, `set` and `reset` subcommand.
//!
//! Per-command property enums and their logic live in that command's own
//! handler (see [`crate::commands::handlers`]); this module only loads,
//! validates, saves and reports for a [`ConfigCommand`] implementation. A save
//! warns that anything derived from the previous settings is now stale; nothing
//! is cleaned or recomputed behind the user's back.

use std::path::Path;

use anyhow::{Result, bail};
use serde::Serialize;
use thiserror::Error;
use tracing::warn;

use crate::commands::{
    config::{ConfigAction, ConfigCommand, InputOverride, PropertyChange},
    handlers::load_configuration,
};

/// Errors shared by every `info` / `set` / `reset` operation.
#[derive(Debug, Error)]
pub enum ConfigEditError {
    /// An `input` property was given no flags at all.
    #[error(
        "provide at least one of --path, --decoder, --filters, --vs-args or --stream-concurrently"
    )]
    InputOverrideRequired,
}

/// Prints `value` as pretty JSON, in the shape it has in `condor.json`.
pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Warns that saved results derived from the edited settings are now stale.
pub fn warn_stale(subject: &str) {
    warn!(
        "{subject} changed; saved results/files may be stale. Nothing was cleaned or recomputed."
    );
}

/// Rejects a *set input* that carries no flags before anything is written.
pub fn ensure_override_requested(args: &InputOverride) -> Result<()> {
    if args.is_empty() {
        bail!(ConfigEditError::InputOverrideRequired);
    }
    Ok(())
}

/// Prints what a `set` or `reset` changed. The status line names the property
/// and the verb, which is enough to explain the run; the value it left behind
/// is printed only under `--verbose`.
pub fn announce(subject: &str, change: &PropertyChange, reset: bool) {
    let verb = if reset {
        "reset to its default"
    } else {
        "updated"
    };
    println!("{subject} {} {verb}", change.property);
    if crate::commands::is_verbose() {
        let pretty = serde_json::to_string_pretty(&change.value)
            .unwrap_or_else(|_| change.value.to_string());
        println!("{pretty}");
    }
}

/// Applies one `info` / `set` / `reset` action to a command's configuration
/// section.
pub fn edit<C: ConfigCommand>(
    config_path: Option<&Path>,
    action: &ConfigAction<C::Key, C::Set>,
) -> Result<()> {
    match action {
        ConfigAction::Info {
            key,
        } => {
            let (configuration, _) = load_configuration(config_path)?;
            let value = C::render(&configuration, key.as_ref())?;
            print_json(&value)?;
        },
        ConfigAction::Set {
            setting,
        } => {
            let (mut configuration, path) = load_configuration(config_path)?;
            let change = C::apply(&mut configuration, setting)?;
            configuration.save(&path)?;
            warn_stale(C::subject());
            announce(C::subject(), &change, false);
        },
        ConfigAction::Reset {
            key,
        } => {
            let (mut configuration, path) = load_configuration(config_path)?;
            let Some(change) = C::restore(&mut configuration, key.as_ref())? else {
                return Ok(());
            };
            configuration.save(&path)?;
            warn_stale(C::subject());
            announce(C::subject(), &change, true);
            if let Some(hint) = C::reset_hint(key.as_ref()) {
                println!("{hint}");
            }
        },
    }
    Ok(())
}
