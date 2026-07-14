use std::env;
use std::fs::DirEntry;
use std::path::Path;
use std::{path::PathBuf, sync::mpsc::Receiver};
use std::{sync::mpsc, thread};

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use ratatui::{
  Frame,
  crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
  layout::{Constraint, Direction, Layout},
  style::{Color, Modifier, Style},
  widgets::{Block, Borders, List, ListItem, ListState},
};
use ssh2::Sftp;

use crate::files;
use crate::sftp::{AuthMethod, ConnectSftpParams, connect_sftp};

struct HostAddress {
  username: Option<String>,
  host: String,
  port: Option<u16>,
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

pub enum Focus {
  Local,
  Remote,
}

pub struct App {
  pub should_quit: bool,

  events: Receiver<AppEvent>,
  focus: Focus,

  local_state: ListState,
  remote_state: ListState,

  local_path: PathBuf,
  remote_path: PathBuf,

  local_items: Vec<DirEntry>,
  remote_items: Vec<PathBuf>,

  sftp: Sftp,
}

pub enum AppEvent {
  KeyPress(KeyEvent),
  DownloadComplete,
  Tick,
}

fn spawn_app_event_threads() -> Receiver<AppEvent> {
  let (tx, rx) = mpsc::channel();

  let tx_keypress = tx.clone();
  thread::spawn(move || {
    loop {
      if let Event::Key(key) = event::read().expect("failed to read key events") {
        if key.kind == KeyEventKind::Press {
          tx_keypress
            .send(AppEvent::KeyPress(key))
            .expect("channel open");
        }
      }
    }
  });

  // TODO: downloads
  // TODO: ticks

  return rx;
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

    let sftp = connect_sftp(ConnectSftpParams {
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
    })?;

    let local_path = env::current_dir().context("failed to get local directory")?;
    let remote_path = sftp
      .realpath(Path::new("."))
      .context("failed to get remote directory")?;

    let local_items = files::local_files(&local_path)?;
    let remote_items = files::remote_files(&remote_path, &sftp)?;

    let local_state = ListState::default().with_selected(if local_items.is_empty() {
      None
    } else {
      Some(0)
    });
    let remote_state = ListState::default().with_selected(if remote_items.is_empty() {
      None
    } else {
      Some(0)
    });

    Ok(Self {
      should_quit: false,
      events: spawn_app_event_threads(),
      focus: Focus::Local,

      local_state,
      remote_state,
      local_path,
      remote_path,
      local_items,
      remote_items,

      sftp,
    })
  }

  pub fn update(&mut self) -> Result<()> {
    match self.events.recv()? {
      AppEvent::KeyPress(key) => match key.code {
        KeyCode::Char('q') => self.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => {}
        KeyCode::Char('k') | KeyCode::Up => {}
        KeyCode::Tab => self.switch_focus(),
        KeyCode::Enter => {}
        _ => {}
      },
      AppEvent::DownloadComplete => todo!(),
      AppEvent::Tick => todo!(),
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
      .map(|e| ListItem::new(e.file_name().to_string_lossy().to_string()))
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
      .filter_map(|buf| buf.file_name())
      .map(|s| ListItem::new(s.to_string_lossy()))
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
