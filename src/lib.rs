mod app;
mod args;
mod files;
mod sftp;

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

pub fn run() -> Result<()> {
  let mut app = app::App::new(args::get_matches()).context("failed to start app")?;

  // raw mode allows the application to track individual key strokes, i.e. without hitting enter.
  enable_raw_mode().context("failed to enable raw mode")?;

  let backend = CrosstermBackend::new(io::stdout());
  let mut terminal = DefaultTerminal::new(backend).context("failed to start terminal")?;
  terminal.backend_mut().execute(EnterAlternateScreen)?;

  while !app.should_quit {
    terminal.draw(|f| app.render(f))?;
    app.update()?;
  }

  disable_raw_mode().context("failed to disable raw mode")?;
  terminal.backend_mut().execute(LeaveAlternateScreen)?;
  terminal.show_cursor()?;

  Ok(())
}
