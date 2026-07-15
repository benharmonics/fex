use std::{
  path::PathBuf,
  sync::mpsc::{self, Receiver},
  thread,
};

use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};

pub enum AppEvent {
  KeyPress(KeyEvent),
}

pub enum TransferEvent {
  DownloadFinished {
    job_id: u64,
    remote_path: PathBuf,
    result: Result<(), String>,
  },
  UploadFinished {
    job_id: u64,
    local_path: PathBuf,
    result: Result<(), String>,
  },
  WorkerInitFailed {
    worker_id: usize,
    error: String,
  },
}

pub fn spawn_app_event_threads() -> Receiver<AppEvent> {
  let (tx, rx) = mpsc::channel();

  let tx_keypress = tx.clone();
  thread::spawn(move || {
    loop {
      if let Event::Key(key) = event::read().expect("failed to read key events")
        && key.kind == KeyEventKind::Press
      {
        tx_keypress
          .send(AppEvent::KeyPress(key))
          .expect("channel open");
      }
    }
  });

  rx
}
