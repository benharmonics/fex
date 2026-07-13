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

use fex::{app::App, args};

fn run() -> Result<()> {
  let mut app = App::new(args::get_matches()).context("failed to start app")?;

  // raw mode allows the application to track individual key strokes, i.e. without hitting enter.
  enable_raw_mode().context("failed to enable raw mode")?;

  let backend = CrosstermBackend::new(io::stdout());
  let mut terminal = DefaultTerminal::new(backend).context("failed to start terminal")?;
  terminal.backend_mut().execute(EnterAlternateScreen)?;

  // TEA loop
  loop {
    app.update()?; // TODO: shouldn't I pass in a message? `update` just polls...

    if app.should_quit {
      break;
    }

    terminal.draw(|f| app.render(f))?;
  }

  disable_raw_mode().context("failed to disable raw mode")?;
  terminal.backend_mut().execute(LeaveAlternateScreen)?;
  terminal.show_cursor()?;

  Ok(())
}

fn main() {
  run().unwrap_or_else(|e| {
    eprintln!("{:#}", e);
    std::process::exit(1);
  })
}
