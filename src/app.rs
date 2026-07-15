use std::{
  collections::HashMap,
  env,
  fs::DirEntry,
  path::Path,
  path::PathBuf,
  sync::mpsc::{self, Receiver, RecvTimeoutError},
  time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use ratatui::{
  Frame,
  crossterm::event::KeyCode,
  layout::{Constraint, Direction, Layout},
  style::{Color, Modifier, Style},
  widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use ssh2::{FileStat, Sftp};

use crate::{
  event::{self, AppEvent, TransferEvent},
  files,
  sftp::{AuthMethod, ConnectSftpParams, connect_sftp},
  transfer::TransferManager,
};

const DEFAULT_SFTP_POOL_SIZE: usize = 2;

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

enum Focus {
  Local,
  Remote,
}

pub struct App {
  pub should_quit: bool,

  events_rx: Receiver<AppEvent>,
  transfer_events_rx: Receiver<TransferEvent>,
  focus: Focus,

  local_state: ListState,
  remote_state: ListState,

  local_path: PathBuf,
  remote_path: PathBuf,

  local_items: Vec<DirEntry>,
  remote_items: Vec<(PathBuf, FileStat)>,

  browser_sftp: Sftp,
  transfer_manager: TransferManager,
  in_flight_downloads: HashMap<u64, PathBuf>,
  in_flight_uploads: HashMap<u64, PathBuf>,
  status_line: String,
}

impl App {
  pub fn new(matches: ArgMatches) -> Result<Self> {
    // TODO: move all interactions with ArgMatches somewhere else
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

    let auth = match matches.get_one::<PathBuf>("identity") {
      Some(key_path) => AuthMethod::PrivateKey {
        key_path: key_path.to_path_buf(),
        passphrase,
      },
      None => AuthMethod::PasswordInput,
    };

    let connect_sftp_params = ConnectSftpParams {
      username: &username,
      host: &host_addr.host,
      port: host_addr.port.unwrap_or(22),
      auth,
    };
    // Since SFTP is fundamentally single-threaded, we'll actually initailize a browser connection
    // for reading directories etc., and one or more worker connections for file transfers.
    let browser_sftp = connect_sftp(&connect_sftp_params).context("sftp connection failed")?;

    let (transfer_events_tx, transfer_events_rx) = mpsc::channel::<TransferEvent>();
    let transfer_manager = TransferManager::new(
      DEFAULT_SFTP_POOL_SIZE,
      &connect_sftp_params,
      transfer_events_tx,
    )
    .context("failed to initialize transfer worker pool")?;

    let local_path = env::current_dir().context("failed to get local directory")?;
    let remote_path = browser_sftp
      .realpath(Path::new("."))
      .context("failed to get remote directory")?;

    let local_items = files::local_files(&local_path)?;
    let remote_items = files::remote_files(&remote_path, &browser_sftp)?;

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
      events_rx: event::spawn_app_event_threads(),
      transfer_events_rx,
      focus: Focus::Local,

      local_state,
      remote_state,
      local_path,
      remote_path,
      local_items,
      remote_items,

      browser_sftp,
      transfer_manager,
      in_flight_downloads: HashMap::new(),
      in_flight_uploads: HashMap::new(),
      status_line: "Ready".to_string(),
    })
  }

  pub fn update(&mut self) -> Result<()> {
    self.drain_transfer_events(); // flush any received transfer events

    match self.events_rx.recv_timeout(Duration::from_millis(100)) {
      Ok(AppEvent::KeyPress(key)) => match key.code {
        KeyCode::Char('q') => self.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => self.next(),
        KeyCode::Char('k') | KeyCode::Up => self.previous(),
        KeyCode::Char('g') => self.first(),
        KeyCode::Char('G') => self.last(),
        KeyCode::Tab => self.switch_focus(),
        KeyCode::Enter => self.transfer_selection(),
        _ => {}
      },
      Err(RecvTimeoutError::Timeout) => {}
      Err(RecvTimeoutError::Disconnected) => {
        bail!("application event channel disconnected")
      }
    }

    self.drain_transfer_events(); // second flush for responsiveness

    Ok(())
  }

  pub fn render(&mut self, f: &mut Frame) {
    let outer_chunks = Layout::default()
      .direction(Direction::Vertical)
      .constraints([Constraint::Min(1), Constraint::Length(1)])
      .split(f.area());

    let chunks = Layout::default()
      .direction(Direction::Horizontal)
      .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
      .split(outer_chunks[0]);

    let left_items: Vec<ListItem> = self
      .local_items
      .iter()
      .map(|e| ListItem::new(e.file_name().to_string_lossy().to_string()))
      .collect();
    let left_block = Block::default()
      .title(self.local_path.to_string_lossy())
      .borders(Borders::ALL)
      .border_style(match self.focus {
        Focus::Local => Style::default().fg(Color::Magenta),
        Focus::Remote => Style::default(),
      });
    let left_list = List::new(left_items)
      .block(left_block)
      .highlight_style(
        Style::default()
          .add_modifier(Modifier::REVERSED)
          .fg(Color::Cyan),
      )
      .highlight_symbol("> ");
    f.render_stateful_widget(left_list, chunks[0], &mut self.local_state);

    let right_items: Vec<ListItem> = self
      .remote_items
      .iter()
      .filter_map(|(buf, _)| buf.file_name())
      .map(|s| ListItem::new(s.to_string_lossy()))
      .collect();
    let right_block = Block::default()
      .title(self.remote_path.to_string_lossy())
      .borders(Borders::ALL)
      .border_style(match self.focus {
        Focus::Local => Style::default(),
        Focus::Remote => Style::default().fg(Color::Magenta),
      });
    let right_list = List::new(right_items)
      .block(right_block)
      .highlight_style(
        Style::default()
          .add_modifier(Modifier::REVERSED)
          .fg(Color::Cyan),
      )
      .highlight_symbol("> ");
    f.render_stateful_widget(right_list, chunks[1], &mut self.remote_state);

    let status = Paragraph::new(self.status_line.clone());
    f.render_widget(status, outer_chunks[1]);
  }

  fn first(&mut self) {
    match self.focus {
      Focus::Local => {
        if self.local_items.is_empty() {
          return;
        }
        self.local_state.select(Some(0));
      }
      Focus::Remote => {
        if self.remote_items.is_empty() {
          return;
        }
        self.remote_state.select(Some(0))
      }
    }
  }

  fn last(&mut self) {
    match self.focus {
      Focus::Local => {
        if self.local_items.is_empty() {
          return;
        }
        self.local_state.select(Some(self.local_items.len() - 1));
      }
      Focus::Remote => {
        if self.remote_items.is_empty() {
          return;
        }
        self.remote_state.select(Some(self.remote_items.len() - 1))
      }
    }
  }

  fn previous(&mut self) {
    match self.focus {
      Focus::Local => {
        if self.local_items.is_empty() {
          return;
        }
        let i = self.local_state.selected().unwrap_or(0);
        let prev = if i == 0 {
          self.local_items.len() - 1
        } else {
          i - 1
        };
        self.local_state.select(Some(prev));
      }
      Focus::Remote => {
        if self.remote_items.is_empty() {
          return;
        }
        let i = self.remote_state.selected().unwrap_or(0);
        let prev = if i == 0 {
          self.remote_items.len() - 1
        } else {
          i - 1
        };
        self.remote_state.select(Some(prev));
      }
    }
  }

  fn next(&mut self) {
    match self.focus {
      Focus::Local => {
        if self.local_items.is_empty() {
          return;
        }
        let i = self.local_state.selected().unwrap_or(0);
        let next = (i + 1) % self.local_items.len();
        self.local_state.select(Some(next));
      }
      Focus::Remote => {
        if self.remote_items.is_empty() {
          return;
        }
        let i = self.remote_state.selected().unwrap_or(0);
        let next = (i + 1) % self.remote_items.len();
        self.remote_state.select(Some(next));
      }
    }
  }

  fn transfer_selection(&mut self) {
    match self.focus {
      // Upload
      Focus::Local => {
        self.status_line = "Upload not implemented".to_string();
        let Some(selected_i) = self.local_state.selected() else {
          self.status_line = "No selection.".to_string();
          return;
        };
        let local_dir_entry = &self.local_items[selected_i];
        let job_id = match self
          .transfer_manager
          .queue_upload(local_dir_entry.path(), self.remote_path.clone())
        {
          Ok(id) => id,
          Err(e) => {
            self.status_line = format!("Failure: {e:#}");
            return;
          }
        };

        let local_path = local_dir_entry.path();
        self.status_line = format!("Queued upload #{}: {:?}", job_id, local_path);
        self.in_flight_uploads.insert(job_id, local_path);
      }

      // Download
      Focus::Remote => {
        let Some(selected_i) = self.remote_state.selected() else {
          self.status_line = "No selection.".to_string();
          return;
        };
        let (remote_path, remote_stat) = &self.remote_items[selected_i];
        let job_id = match self.transfer_manager.queue_download(
          remote_path.clone(),
          remote_stat.clone(),
          self.local_path.clone(),
        ) {
          Ok(id) => id,
          Err(e) => {
            self.status_line = format!("Failure: {e:#}");
            return;
          }
        };

        self.in_flight_downloads.insert(job_id, remote_path.clone());
        self.status_line = format!("Queued download #{}: {:?}", job_id, remote_path);
      }
    }
  }

  fn switch_focus(&mut self) {
    self.focus = match self.focus {
      Focus::Local => Focus::Remote,
      Focus::Remote => Focus::Local,
    }
  }

  fn drain_transfer_events(&mut self) {
    while let Ok(event) = self.transfer_events_rx.try_recv() {
      match event {
        TransferEvent::DownloadFinished {
          job_id,
          remote_path,
          result,
        } => {
          self.in_flight_downloads.remove(&job_id);
          match result {
            Ok(()) => {
              self.status_line =
                format!("Downloaded #{}: {}", job_id, remote_path.to_string_lossy());
              let Ok(items) = files::local_files(&self.local_path) else {
                return;
              };
              self.local_items = items;
              if self.local_items.is_empty() {
                self.local_state.select(None);
              } else if self.local_state.selected().is_none() {
                self.local_state.select(Some(0));
              }
            }
            Err(error) => {
              self.status_line = format!(
                "Download failed #{}: {} ({})",
                job_id,
                remote_path.to_string_lossy(),
                error
              );
            }
          }
        }

        TransferEvent::UploadFinished {
          job_id,
          local_path,
          result,
        } => {
          self.in_flight_uploads.remove(&job_id);
          match result {
            Ok(()) => {
              self.status_line = format!("Uploaded #{}: {:?}", job_id, local_path);
              let Ok(items) = files::remote_files(&self.remote_path, &self.browser_sftp) else {
                return;
              };
              self.remote_items = items;
              if self.remote_items.is_empty() {
                self.remote_state.select(None);
              } else if self.remote_state.selected().is_none() {
                self.remote_state.select(Some(0));
              }
            }
            Err(error) => {
              self.status_line =
                format!("Upload failed #{}: {:?} ({})", job_id, local_path, error);
            }
          }
        }

        TransferEvent::WorkerInitFailed { worker_id, error } => {
          self.status_line = format!("Worker {} failed: {}", worker_id, error);
        }
      }
    }
  }
}
