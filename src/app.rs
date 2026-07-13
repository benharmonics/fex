use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use ratatui::{
  Frame,
  crossterm::event::{self, Event, KeyCode, KeyEventKind},
  layout::{Constraint, Direction, Layout},
  style::{Color, Modifier, Style},
  widgets::{Block, Borders, List, ListItem, ListState},
};
use ssh2::Sftp;

use crate::sftp::{self, AuthMethod};

pub enum Focus {
  Local,
  Remote,
}

pub struct App {
  pub should_quit: bool,

  pub focus: Focus,

  pub local_state: ListState,
  pub remote_state: ListState,

  pub local_path: PathBuf,
  pub remote_path: PathBuf,

  pub local_items: Vec<PathBuf>,
  pub remote_items: Vec<PathBuf>,

  pub sftp: Sftp,
}

struct HostAddress {
  pub username: Option<String>,
  pub host: String,
  pub port: Option<u16>,
}

fn parse_host_address(input: &str) -> Result<HostAddress> {
  let (username, host_part) = match input.split_once('@') {
    Some((user, rest)) if !user.is_empty() => (Some(user.to_string()), rest),
    _ => (None, input),
  };

  let (host, port) = match host_part.rsplit_once(':') {
    Some((h, p)) if !p.is_empty() => {
      let port_num = p
        .parse::<u16>()
        .with_context(|| format!("failed to parse port {p} as u16"))?;
      (h.to_string(), Some(port_num))
    }
    _ => (host_part.to_string(), None),
  };

  if host.is_empty() {
    bail!("invalid format: empty host");
  }

  Ok(HostAddress {
    username,
    host,
    port,
  })
}

impl App {
  pub fn new(matches: ArgMatches) -> Result<Self> {
    let host_addr = parse_host_address(
      matches
        .get_one::<String>("host")
        .expect("host is required argument"),
    )
    .context("failed to parse host address")?;
    let username = host_addr.username.unwrap_or_else(|| {
      whoami::username().expect("failed to get username as fallback when none provided")
    });
    let passphrase = matches.get_one::<String>("passphrase").cloned();

    // TODO: this might need to be parsed later in the workflow
    let auth = match matches.get_one::<PathBuf>("identity") {
      Some(key_path) => AuthMethod::PrivateKey {
        key_path: key_path.to_path_buf(),
        passphrase,
      },

      None => AuthMethod::PasswordInput,
    };

    Ok(Self {
      should_quit: false,
      focus: Focus::Local,
      local_state: ListState::default(),
      remote_state: ListState::default(),
      local_path: PathBuf::new(),
      remote_path: PathBuf::new(),
      local_items: Vec::new(),
      remote_items: Vec::new(),
      sftp: sftp::connect_sftp(sftp::ConnectSftpParams {
        username: &username,
        host: &host_addr.host,
        port: host_addr.port.unwrap_or(22),
        auth,
      })
      .with_context(|| {
        format!(
          "failed to connect to host {} with username {}",
          host_addr.host, username
        )
      })?,
    })
  }

  pub fn update(&mut self) -> Result<()> {
    if event::poll(Duration::from_millis(100))? {
      if let Event::Key(key) = event::read()? {
        if key.kind == KeyEventKind::Press {
          match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => {}
            KeyCode::Char('k') | KeyCode::Up => {}
            KeyCode::Tab => self.switch_focus(),
            _ => {}
          }
        }
      }
    }

    Ok(())
  }

  pub fn render(&mut self, f: &mut Frame) {
    let chunks = Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
      .split(f.area());

    // Local items
    let left_items: Vec<ListItem> = self
      .local_items
      .iter()
      .map(|buf| ListItem::new(buf.to_string_lossy()))
      .collect();
    let left_block = Block::default()
      .title("Local")
      .borders(Borders::ALL)
      .border_style(match self.focus {
        Focus::Local => Style::default().fg(Color::Magenta),
        Focus::Remote => Style::default(),
      });
    let left_list = List::new(left_items)
      .block(left_block)
      .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
      .highlight_symbol("> ");
    f.render_stateful_widget(left_list, chunks[0], &mut self.local_state);

    // Remote items
    let right_items: Vec<ListItem> = self
      .remote_items
      .iter()
      .map(|buf| ListItem::new(buf.to_string_lossy()))
      .collect();
    let right_block = Block::default()
      .title("Remote")
      .borders(Borders::ALL)
      .border_style(match self.focus {
        Focus::Local => Style::default(),
        Focus::Remote => Style::default().fg(Color::Magenta),
      });
    let right_list = List::new(right_items)
      .block(right_block)
      .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
      .highlight_symbol("> ");
    f.render_stateful_widget(right_list, chunks[1], &mut self.remote_state);
  }

  fn switch_focus(&mut self) {
    self.focus = match self.focus {
      Focus::Local => Focus::Remote,
      Focus::Remote => Focus::Local,
    }
  }
}
