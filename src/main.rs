use std::{io, time::Duration};

use anyhow::{Context, Result};
use ratatui::{
  DefaultTerminal, Frame,
  crossterm::{
    ExecutableCommand, event::{self, Event, KeyCode, KeyEventKind}, terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode}
  },
  layout::{Constraint, Direction, Layout},
  prelude::CrosstermBackend,
  style::{Color, Modifier, Style},
  widgets::{Block, Borders, List, ListItem},
};

use fex::{app::{App, Focus}, args};

fn main() -> Result<()> {
  let mut app = App::new(args::get_matches()).context("failed to start app")?;

  // raw mode allows the application to track individual key strokes, i.e. without hitting enter.
  enable_raw_mode().context("failed to enable raw mode")?;

  let backend = CrosstermBackend::new(io::stdout());
  let mut terminal = DefaultTerminal::new(backend).context("failed to start terminal")?;
  terminal.backend_mut().execute(EnterAlternateScreen)?;

  // TEA loop
  loop {
    if event::poll(Duration::from_millis(100))? {
      if let Event::Key(key) = event::read()? {
        if key.kind == KeyEventKind::Press {
          match key.code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => {},
            KeyCode::Char('k') | KeyCode::Up => {},
            KeyCode::Tab => app.switch_focus(),
            _ => {},
          }
        }
      }
    }

    if app.should_quit {
      break;
    }

    terminal
      .draw(|f| ui(f, &mut app))
      .expect("failed to render UI");
  }

  disable_raw_mode().context("failed to disable raw mode")?;
  terminal.backend_mut().execute(LeaveAlternateScreen)?;
  terminal.show_cursor()?;

  Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
  let chunks = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
    .split(f.area());

  // Local items
  let left_items: Vec<ListItem> = app
    .local_items
    .iter()
    .map(|buf| ListItem::new(buf.to_string_lossy()))
    .collect();
  let left_block = Block::default()
    .title("Local")
    .borders(Borders::ALL)
    .border_style(match app.focus {
      Focus::Local => Style::default().fg(Color::Magenta),
      Focus::Remote => Style::default(),
    });
  let left_list = List::new(left_items)
    .block(left_block)
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol("> ");
  f.render_stateful_widget(left_list, chunks[0], &mut app.local_state);

  // Remote items
  let right_items: Vec<ListItem> = app
    .remote_items
    .iter()
    .map(|buf| ListItem::new(buf.to_string_lossy()))
    .collect();
  let right_block = Block::default()
    .title("Remote")
    .borders(Borders::ALL)
    .border_style(match app.focus {
      Focus::Local => Style::default(),
      Focus::Remote => Style::default().fg(Color::Magenta),
    });
  let right_list = List::new(right_items)
    .block(right_block)
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol("> ");
  f.render_stateful_widget(right_list, chunks[1], &mut app.remote_state);
}
