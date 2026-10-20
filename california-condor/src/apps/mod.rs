use std::{
    io::{self, BufWriter, IsTerminal, Write, stderr, stdout},
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread,
    time::Duration,
};

use andean_condor::core::sequence::SequenceStatus;
use anyhow::Result;
use ratatui::{
    Frame,
    Terminal,
    crossterm::{
        self,
        event::{self, Event as TermEvent, KeyCode, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    prelude::CrosstermBackend,
};
use tracing::debug;

pub mod benchmarker;
pub mod noise_detection;
pub mod parallel_encoder;
pub mod quality_check;
pub mod scene_concatenator;
pub mod scene_detection;
pub mod target_quality;

pub type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>;
pub type StdOutOrErrTerminal = Terminal<CrosstermBackend<BufWriter<Box<dyn Write + Send>>>>;

/// When `CONDOR_TEST_MODE=1` is set (testing / automation), skip
/// terminal-specific operations (raw mode, alternate screen, keyboard input)
/// and use JSON progress output instead.
pub fn is_test_mode() -> bool {
    std::env::var("CONDOR_TEST_MODE").is_ok_and(|v| v == "1")
}

/// Whether the app should drive the TUI or wait headlessly instead.
///
/// The TUI renders to stdout when it is a terminal and to stderr otherwise,
/// and it needs raw mode plus the alternate screen on whichever it picks.
/// With *neither* stream on a terminal — output piped into a file, a CI job,
/// or a container started without `-t` — enabling raw mode fails with
/// `ENXIO` (`No such device or address`) because there is no controlling
/// terminal to take over.
fn run_headless(stdout_is_terminal: bool, stderr_is_terminal: bool, test_mode: bool) -> bool {
    test_mode || !(stdout_is_terminal || stderr_is_terminal)
}

/// Thread-safe container for sharing mutable progress state between a
/// producer thread (progress receiver) and a consumer thread (UI event loop).
///
/// The producer calls [`SharedProgress::apply`] to update state. The consumer
/// calls [`SharedProgress::read_if_dirty`] once per tick to retrieve the
/// latest snapshot.
pub struct SharedProgress<State: Clone + Send + 'static> {
    state: Arc<Mutex<State>>,
    dirty: Arc<AtomicBool>,
}

impl<State: Clone + Send + 'static> SharedProgress<State> {
    pub fn new(initial: State) -> Self {
        Self {
            state: Arc::new(Mutex::new(initial)),
            dirty: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Apply an update function to the shared state.
    ///
    /// The `update` closure receives a mutable reference to the inner state and
    /// returns `true` if the state was meaningfully changed (setting the dirty
    /// flag so the consumer will pick it up on the next tick).
    pub fn apply(&self, update: impl FnOnce(&mut State) -> bool) {
        let mut guard = self.state.lock().expect("SharedProgress lock");
        if update(&mut *guard) {
            self.dirty.store(true, Ordering::Release);
        }
    }

    /// Read-and-clear the dirty flag.
    ///
    /// Returns `Some(state)` if a new snapshot is available since the last
    /// call, or `None` if nothing has changed.
    pub fn read_if_dirty(&self) -> Option<State> {
        if self.dirty.swap(false, Ordering::Acquire) {
            let guard = self.state.lock().expect("SharedProgress lock");
            return Some(guard.clone());
        }
        None
    }

    /// Force-read the current state, ignoring the dirty flag.
    pub fn read(&self) -> State {
        let guard = self.state.lock().expect("SharedProgress lock");
        guard.clone()
    }
}

// Every clone shares the same `Arc`-backed state.
impl<S: Clone + Send + 'static> Clone for SharedProgress<S> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            dirty: Arc::clone(&self.dirty),
        }
    }
}

enum AppEvent {
    Quit,
}

pub trait TuiApp: Send + Sync + 'static {
    /// The progress snapshot shared between the producer and consumer threads.
    type State: Clone + Send + 'static;

    /// The struct must have a field to store original panic hook as
    /// Option<PanicHook>
    fn original_panic_hook(&mut self) -> &mut Option<PanicHook>;

    /// The shared progress container backing this app.
    fn shared_progress(&self) -> &SharedProgress<Self::State>;

    /// The latest cached snapshot, updated by the event loop.
    fn cached_state(&self) -> &Self::State;

    /// Mutable access to the cached snapshot (used by the event loop).
    fn cached_state_mut(&mut self) -> &mut Self::State;

    /// The `attempted_cancel` flag (set on first Ctrl-C).
    fn attempted_cancel(&self) -> bool;

    /// Mutable access to the `attempted_cancel` flag.
    fn attempted_cancel_mut(&mut self) -> &mut bool;

    /// The message printed to a non-TTY stdout on the first Ctrl-C.
    fn cancel_message(&self) -> &'static str;

    /// Map a single [`SequenceStatus`] onto the shared state, returning `true`
    /// if the state changed. Console (non-TTY) emission is handled here too.
    ///
    /// This is a static method (no `self`) so it can run on the spawned
    /// progress thread. Any per-app context needed here must live in `State`.
    fn map_progress(status: SequenceStatus, state: &mut Self::State) -> bool;

    /// Optional hook invoked with each fresh snapshot read from the shared
    /// state. The default implementation caches it. Apps with UI-local state
    /// (e.g. scene tables) or transition reactions (e.g. resetting a pass
    /// timer) override this to sync before caching.
    fn on_snapshot(&mut self, snapshot: Self::State) {
        *self.cached_state_mut() = snapshot;
    }

    /// Optional hook invoked once after the loop terminates (both TTY and
    /// headless paths). Used for post-run reports. Defaults to a no-op.
    fn after_loop(&mut self) {
    }

    fn init(&mut self) -> Result<StdOutOrErrTerminal> {
        let use_stdout = stdout().is_terminal();

        if !is_test_mode() {
            enable_raw_mode()?;
        }

        let writer: Box<dyn Write + Send> = if is_test_mode() {
            Box::new(io::sink())
        } else if use_stdout {
            Box::new(stdout())
        } else {
            Box::new(stderr())
        };
        let mut writer = BufWriter::new(writer);

        if !is_test_mode() {
            execute!(writer, EnterAlternateScreen)?;
        }

        let original_hook = std::panic::take_hook();
        *self.original_panic_hook() = Some(original_hook);
        std::panic::set_hook(Box::new(move |panic_info| {
            if !is_test_mode() {
                let _ = disable_raw_mode();
                let mut w: Box<dyn Write + Send> = if use_stdout {
                    Box::new(stdout())
                } else {
                    Box::new(stderr())
                };
                let _ = execute!(w, LeaveAlternateScreen);
                let _ = execute!(w, crossterm::cursor::Show);
            }
            println!("{:?}", panic_info);
        }));

        let backend = CrosstermBackend::new(writer);
        Ok(Terminal::new(backend)?)
    }

    fn restore(&mut self, mut terminal: StdOutOrErrTerminal) -> Result<()> {
        // Guarding on the stored hook also makes a repeat restore a no-op: a
        // second call would otherwise install this app's own hook as the
        // "original", losing the process-wide one.
        if self.original_panic_hook().is_none() {
            return Ok(());
        }

        if !is_test_mode() {
            disable_raw_mode()?;
            execute!(io::stdout(), LeaveAlternateScreen)
                .or_else(|_| execute!(io::stderr(), LeaveAlternateScreen))?;

            let (_columns, rows) = crossterm::terminal::size().unwrap_or((80, 24));
            for _ in 0..rows {
                println!();
            }
            terminal.clear()?;
            terminal.draw(|f| self.render(f))?;
            execute!(io::stdout(), crossterm::cursor::Show)
                .or_else(|_| execute!(io::stderr(), crossterm::cursor::Show))?;
        }

        let _ = std::panic::take_hook();
        if let Some(original) = self.original_panic_hook().take() {
            std::panic::set_hook(original);
        }

        Ok(())
    }

    /// End the process after a force quit.
    ///
    /// Exiting the process rather than just the UI loop is what makes the
    /// second Ctrl-C prompt: the sequence driving this app is blocked on
    /// encoders that cannot be interrupted. Called only after the terminal
    /// is restored, since exiting first would strand raw mode and the
    /// alternate screen.
    fn force_quit() -> ! {
        debug!("Exiting immediately");
        std::process::exit(0);
    }

    /// Handle a single Ctrl-C key press, returning `true` when the app should
    /// force-quit (second press).
    fn handle_ctrl_c(
        &mut self,
        key: event::KeyEvent,
        cancelled: &Arc<AtomicBool>,
        stdout_is_terminal: bool,
    ) -> bool {
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.is_press()
        {
            *self.attempted_cancel_mut() = true;
            let already_cancelled = cancelled.swap(true, Ordering::SeqCst);
            if already_cancelled {
                debug!("Force quit Condor");
                return true;
            } else if !stdout_is_terminal {
                println!("{}", self.cancel_message());
            }
        }
        false
    }

    /// The full runtime: spawns the progress thread, waits headlessly in
    /// test mode, and drives the terminal event loop otherwise.
    fn run(
        &mut self,
        progress_rx: Receiver<SequenceStatus>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        // Install the render theme for the whole run. Reading an env var keeps
        // the plumbing out of every `new()`; the default (dark) is the
        // historical look. Headless runs never render, so this is a no-op there.
        let _theme_guard = crate::theme::Theme::set_mode(crate::theme::ThemeMode::parse(
            std::env::var("CONDOR_THEME").ok().as_deref(),
        ));
        let (event_tx, event_rx) = mpsc::channel();

        let shared_progress = self.shared_progress().clone();
        let quit = Arc::new(AtomicBool::new(false));
        let quit_flag = Arc::clone(&quit);
        thread::spawn(move || {
            for progress in progress_rx {
                shared_progress.apply(|state| Self::map_progress(progress, state));
            }
            let _ = event_tx.send(AppEvent::Quit);
            quit_flag.store(true, Ordering::Release);
        });

        if run_headless(
            stdout().is_terminal(),
            stderr().is_terminal(),
            is_test_mode(),
        ) {
            // No terminal to take over (or test mode): avoid any terminal
            // operations - headless wait.
            while !quit.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(10));
                if let Some(snapshot) = self.shared_progress().read_if_dirty() {
                    self.on_snapshot(snapshot);
                }
                while event_rx.try_recv().is_ok() {}
            }
            let snapshot = self.shared_progress().read();
            self.on_snapshot(snapshot);
            self.after_loop();
            return Ok(());
        }

        let mut terminal = self.init()?;
        let stdout_is_terminal = stdout().is_terminal();
        loop {
            // Keyboard input is read here, never on a helper thread:
            // crossterm's event reader is process-wide, and teardown's
            // cursor position query (restore -> terminal.clear) must acquire
            // it, so no other thread may hold it. The drain also ends when
            // input is exhausted or no terminal can be read (`Err`).
            while matches!(event::poll(Duration::ZERO), Ok(true)) {
                if let Ok(TermEvent::Key(key)) = event::read()
                    && self.handle_ctrl_c(key, &cancelled, stdout_is_terminal)
                {
                    if let Some(snapshot) = self.shared_progress().read_if_dirty() {
                        self.on_snapshot(snapshot);
                    }
                    terminal.draw(|f| self.render(f))?;
                    self.restore(terminal)?;
                    Self::force_quit();
                }
            }

            if let Some(snapshot) = self.shared_progress().read_if_dirty() {
                self.on_snapshot(snapshot);
            }

            if quit.load(Ordering::Acquire) {
                let snapshot = self.shared_progress().read();
                self.on_snapshot(snapshot);
                terminal.draw(|f| self.render(f))?;
                self.restore(terminal)?;
                break;
            }

            match event_rx.recv_timeout(Duration::from_millis(33)) {
                Ok(AppEvent::Quit) => {
                    if let Some(snapshot) = self.shared_progress().read_if_dirty() {
                        self.on_snapshot(snapshot);
                    }
                    terminal.draw(|f| self.render(f))?;
                    self.restore(terminal)?;
                    break;
                },
                Err(RecvTimeoutError::Timeout) => {
                    terminal.draw(|f| self.render(f))?;
                },
                Err(RecvTimeoutError::Disconnected) => {
                    if let Some(snapshot) = self.shared_progress().read_if_dirty() {
                        self.on_snapshot(snapshot);
                    }
                    terminal.draw(|f| self.render(f))?;
                    self.restore(terminal)?;
                    break;
                },
            }
        }

        self.after_loop();
        Ok(())
    }

    fn render(&self, frame: &mut Frame);
}

#[cfg(test)]
mod tests {
    use super::run_headless;

    #[test]
    fn always_run_headless_in_test_mode() {
        assert!(run_headless(true, true, true));
        assert!(run_headless(false, false, true));
    }

    #[test]
    fn a_terminal_on_either_stream_runs_the_tui() {
        assert!(!run_headless(true, false, false));
        assert!(!run_headless(false, true, false));
        assert!(!run_headless(true, true, false));
    }

    #[test]
    fn no_terminal_at_all_runs_headless() {
        // The `docker run` without `-t` case: raw mode would fail with ENXIO.
        assert!(run_headless(false, false, false));
    }
}
