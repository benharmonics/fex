mod app;
mod args;
mod config;
mod files;
mod sftp;
mod transfer;

use std::io;

use anyhow::{Context, Result};
use ratatui::{
  DefaultTerminal,
  crossterm::{
    ExecutableCommand,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
  },
  prelude::CrosstermBackend,
};

struct TerminalGuard {
  terminal: DefaultTerminal,
}

impl TerminalGuard {
  fn new() -> Result<Self> {
    // raw mode allows the application to track individual key strokes, i.e. without hitting enter.
    enable_raw_mode().context("failed to enable raw mode")?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = DefaultTerminal::new(backend).context("failed to start terminal")?;
    terminal.backend_mut().execute(EnterAlternateScreen)?;
    Ok(Self { terminal })
  }
}

impl Drop for TerminalGuard {
  fn drop(&mut self) {
    // best-effort cleanup - ignore errors
    let _ = disable_raw_mode();
    let _ = self.terminal.backend_mut().execute(LeaveAlternateScreen);
    let _ = self.terminal.show_cursor();
  }
}

pub fn run() -> Result<()> {
  let cfg = config::AppConfig::parse_args(&args::get_matches())?;
  let mut app = app::App::new(cfg).context("failed to start app")?;

  let mut tg = TerminalGuard::new()?;

  while !app.should_quit {
    tg.terminal.draw(|f| app.render(f))?;
    app.update();
  }

  Ok(())
}
