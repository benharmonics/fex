use std::{
  collections::HashMap,
  env,
  fs::DirEntry,
  path::Path,
  path::PathBuf,
  sync::mpsc::{self, Receiver},
  time::Duration,
};

use anyhow::{Context, Result};
use ratatui::{
  Frame,
  crossterm::event::{self, Event, KeyCode, KeyEventKind},
  layout::{Constraint, Direction, Layout},
  style::{Color, Modifier, Style},
  widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use ssh2::{FileStat, Sftp};

use crate::{
  config::AppConfig,
  files,
  sftp::{ConnectSftpParams, connect_sftp},
  transfer::{TransferEvent, TransferManager},
};

const HELP_TEXT_LINES: [&str; 3] = [
  "j or <down> - next              k or <up> - previous             y or <enter> - download",
  "h or <left> - leave directory   l or <right> - enter directory   w or <tab> - switch focus",
  "q or <esc> - exit               ? - show help text               a - toggle show hidden files",
];

enum Focus {
  Local,
  Remote,
}

pub struct App {
  pub should_quit: bool,
  show_help: bool,
  focus: Focus,
  status_line: String,
  browser_sftp: Sftp,

  transfer_manager: TransferManager,
  transfer_events_rx: Receiver<TransferEvent>,
  in_flight_transfers: HashMap<u64, PathBuf>,

  local_state: ListState,
  remote_state: ListState,

  local_path: PathBuf,
  remote_path: PathBuf,

  local_items: Vec<DirEntry>,
  remote_items: Vec<(PathBuf, FileStat)>,
}

impl App {
  pub fn new(config: AppConfig) -> Result<Self> {
    let connect_sftp_params = ConnectSftpParams {
      username: &config.username(),
      host: &config.host(),
      port: config.port(),
      auth: config.auth(),
    };
    // Since SFTP is fundamentally single-threaded, we'll actually initailize a browser connection
    // for reading directories etc., and one or more worker connections for file transfers.
    let browser_sftp = connect_sftp(&connect_sftp_params).context("sftp connection failed")?;

    let (transfer_events_tx, transfer_events_rx) = mpsc::channel::<TransferEvent>();
    let transfer_manager = TransferManager::new(
      config.sftp_pool_size,
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
      show_help: false,
      focus: Focus::Local,
      status_line: "q to quit | ? to show help".to_string(),
      browser_sftp,

      transfer_manager,
      in_flight_transfers: HashMap::new(),
      transfer_events_rx,

      local_state,
      remote_state,
      local_path,
      remote_path,
      local_items,
      remote_items,
    })
  }

  /// Update the application state.
  pub fn update(&mut self) {
    self.drain_transfer_events(); // flush any received transfer events

    if event::poll(Duration::from_millis(100)).expect("crossterm event reader closed")
      && let Event::Key(key) = event::read().expect("failed to read key events")
      && key.kind == KeyEventKind::Press
    {
      match key.code {
        KeyCode::Esc | KeyCode::Char('q') => self.should_quit = true,
        KeyCode::Char('?') => self.show_help = !self.show_help,
        KeyCode::Char('j') | KeyCode::Down => self.next(),
        KeyCode::Char('k') | KeyCode::Up => self.previous(),
        KeyCode::Char('h') | KeyCode::Left => self.enter_parent_dir(),
        KeyCode::Char('l') | KeyCode::Right => self.enter_child_dir(),
        KeyCode::Char('g') | KeyCode::Char('t') => self.first(),
        KeyCode::Char('G') | KeyCode::Char('b') => self.last(),
        KeyCode::Char('w') | KeyCode::Tab => self.switch_focus(),
        KeyCode::Char('y') | KeyCode::Enter => self.transfer_selection(),
        _ => {}
      }
    }

    self.drain_transfer_events(); // second flush for responsiveness
  }

  /// Render the application to a frame - usually the whole terminal object.
  pub fn render(&mut self, f: &mut Frame) {
    let outer_chunks = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
        Constraint::Fill(1),
        Constraint::Length(if self.show_help { 5 } else { 1 }),
        Constraint::Length(1),
      ])
      .split(f.area());

    let inner_chunks = Layout::default()
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
          .fg(match self.focus {
            Focus::Local => Color::LightMagenta,
            Focus::Remote => Color::default(),
          }),
      )
      .highlight_symbol("> ");
    f.render_stateful_widget(left_list, inner_chunks[0], &mut self.local_state);

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
          .fg(match self.focus {
            Focus::Local => Color::default(),
            Focus::Remote => Color::LightMagenta,
          }),
      )
      .highlight_symbol("> ");
    f.render_stateful_widget(right_list, inner_chunks[1], &mut self.remote_state);

    if self.show_help {
      let help_block = Block::default()
        .title("Help")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
      let help = Paragraph::new(HELP_TEXT_LINES.join("\n")).block(help_block);
      f.render_widget(help, outer_chunks[1]);
    }
    let status = Paragraph::new(self.status_line.clone());
    f.render_widget(status, outer_chunks[2]);
  }

  /// Toggle between focusing on local and remote host.
  fn switch_focus(&mut self) {
    self.focus = match self.focus {
      Focus::Local => Focus::Remote,
      Focus::Remote => Focus::Local,
    }
  }

  /// Go to first item in focused list.
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

  /// Go to last item in focused list.
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

  /// Go to previous item in focused list.
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

  /// Go to next item in focused list.
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

  /// Enter the currently selected directory, if possible.
  fn enter_child_dir(&mut self) {
    match self.focus {
      Focus::Local => {
        let Some(i) = self.local_state.selected() else {
          return;
        };
        let path_buf = &self.local_items[i].path();
        if !path_buf.is_dir() {
          self.status_line = "Not a directory.".to_string();
          return;
        };
        self.local_path = path_buf.clone();
        self.refresh_local_state();
      }
      Focus::Remote => {
        let Some(i) = self.remote_state.selected() else {
          return;
        };
        let (buf, stat) = &self.remote_items[i];
        if !stat.is_dir() {
          self.status_line = "Not a directory.".to_string();
          return;
        };
        self.remote_path = buf.clone();
        self.refresh_remote_state();
      }
    }
  }

  /// Exit the currently focused directory, entering its parent, if it has one.
  fn enter_parent_dir(&mut self) {
    match self.focus {
      Focus::Local => {
        let Some(parent) = self.local_path.parent() else {
          return;
        };
        self.local_path = parent.to_path_buf();
        self.refresh_local_state();
      }
      Focus::Remote => {
        let Some(parent) = self.remote_path.parent() else {
          return;
        };
        self.remote_path = parent.to_path_buf();
        self.refresh_remote_state();
      }
    }
  }

  /// Go to next item in focused list.
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
        self.in_flight_transfers.insert(job_id, local_path);
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

        self.in_flight_transfers.insert(job_id, remote_path.clone());
        self.status_line = format!("Queued download #{}: {:?}", job_id, remote_path);
      }
    }
  }

  /// Handle any transfer events received since last drain.
  fn drain_transfer_events(&mut self) {
    while let Ok(event) = self.transfer_events_rx.try_recv() {
      match event {
        TransferEvent::DownloadFinished {
          job_id,
          remote_path,
          result,
        } => {
          self.in_flight_transfers.remove(&job_id);
          match result {
            Ok(()) => {
              self.status_line = format!("Downloaded #{}: {:?}", job_id, remote_path);
              self.refresh_local_state();
            }
            Err(error) => {
              self.status_line =
                format!("Download failed #{}: {:?} ({})", job_id, remote_path, error);
            }
          }
        }

        TransferEvent::UploadFinished {
          job_id,
          local_path,
          result,
        } => {
          self.in_flight_transfers.remove(&job_id);
          match result {
            Ok(()) => {
              self.status_line = format!("Uploaded #{}: {:?}", job_id, local_path);
              self.refresh_remote_state();
            }
            Err(error) => {
              self.status_line = format!("Upload failed #{}: {:?} ({})", job_id, local_path, error);
            }
          }
        }

        TransferEvent::WorkerInitFailed { worker_id, error } => {
          self.status_line = format!("Worker {} failed: {}", worker_id, error);
        }
      }
    }
  }

  /// Re-read the current local path and refresh local items and state.
  fn refresh_local_state(&mut self) {
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

  /// Re-read the current remote path and refresh remote items and state.
  fn refresh_remote_state(&mut self) {
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
}
