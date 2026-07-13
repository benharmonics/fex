use std::io;

use ratatui::{DefaultTerminal, prelude::CrosstermBackend};

use fex::{app::App, args};

fn main() {
  let app = App::new(args::get_matches()).unwrap_or_else(|e| {
    eprintln!("{e:#}");
    std::process::exit(1);
  });

  let mut terminal =
    DefaultTerminal::new(CrosstermBackend::new(io::stdout())).unwrap_or_else(|e| {
      eprintln!("Failed to start terminal: {e:#}");
      std::process::exit(1);
    });

  while !app.should_quit {
    terminal
      .draw(|frame| {
        frame.render_widget(&app, frame.area());
      })
      .expect("Unexpected app failure");
  }
}
