use std::{
  path::PathBuf,
  sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, Sender},
  },
  thread::{self, JoinHandle},
};

use anyhow::{Context, Result, bail};
use ssh2::FileStat;

use crate::{
  event::TransferEvent,
  files,
  sftp::{ConnectSftpParams, connect_sftp},
};

const MAX_SFTP_POOL_SIZE: usize = 8;

struct TransferJob {
  id: u64,
  remote_path: PathBuf,
  remote_stat: FileStat,
  local_target_dir: PathBuf,
}

pub struct TransferManager {
  jobs_tx: Option<Sender<TransferJob>>,
  worker_handles: Vec<JoinHandle<()>>,
  next_job_id: AtomicU64,
}

impl TransferManager {
  pub fn new(
    pool_size: usize,
    connect_sftp_params: &ConnectSftpParams,
    events_tx: Sender<TransferEvent>,
  ) -> Result<Self> {
    if !(1..=MAX_SFTP_POOL_SIZE).contains(&pool_size) {
      bail!(
        "invalid SFTP pool size {}; expected value between 1 and {}",
        pool_size,
        MAX_SFTP_POOL_SIZE
      );
    }

    let worker_count = pool_size.saturating_sub(1);
    let (jobs_tx, jobs_rx) = mpsc::channel::<TransferJob>();
    let shared_rx = Arc::new(Mutex::new(jobs_rx));

    let mut worker_handles = Vec::with_capacity(worker_count);
    for worker_id in 0..worker_count {
      let rx = Arc::clone(&shared_rx);
      let tx = events_tx.clone();
      let sftp = match connect_sftp(connect_sftp_params) {
        Ok(sftp) => sftp,
        Err(e) => {
          let evt = TransferEvent::WorkerInitFailed {
            worker_id,
            error: e.to_string(),
          };
          if tx.send(evt).is_err() {
            break; // receiver closed
          }
          continue;
        }
      };

      worker_handles.push(
        thread::Builder::new()
          .name(format!("transfer-worker-{worker_id}"))
          .spawn(move || {
            loop {
              let Ok(job) = rx.lock().expect("transfer queue lock poisoned").recv() else {
                break; // sender closed
              };

              let result = files::download(
                &job.remote_path,
                &job.remote_stat,
                &job.local_target_dir,
                &sftp,
              )
              .map_err(|e| format!("{e:#}"));

              let evt = TransferEvent::DownloadFinished {
                job_id: job.id,
                remote_path: job.remote_path,
                result,
              };
              if tx.send(evt).is_err() {
                break; // receiver closed
              }
            }
          })
          .context("failed to spawn transfer worker thread")?,
      );
    }

    Ok(Self {
      jobs_tx: Some(jobs_tx),
      worker_handles,
      next_job_id: AtomicU64::new(1),
    })
  }

  pub fn queue_download(
    &self,
    remote_path: PathBuf,
    remote_stat: FileStat,
    local_target_dir: PathBuf,
  ) -> Result<u64> {
    let job_id = self.next_job_id.fetch_add(1, Ordering::Relaxed);
    let job = TransferJob {
      id: job_id,
      remote_path,
      remote_stat,
      local_target_dir,
    };

    let tx = self
      .jobs_tx
      .as_ref()
      .context("transfer manager is shut down and cannot queue jobs")?;

    tx.send(job)
      .context("failed to queue transfer job because workers are unavailable")?;

    Ok(job_id)
  }
}

impl Drop for TransferManager {
  fn drop(&mut self) {
    self.jobs_tx.take();

    while let Some(handle) = self.worker_handles.pop() {
      let _ = handle.join();
    }
  }
}
