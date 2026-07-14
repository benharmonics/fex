use std::{
  ffi::OsStr,
  fs::{self, DirEntry, File},
  io,
  path::PathBuf,
};

use anyhow::{Context, Result};
use ssh2:: Sftp;

pub fn local_files(dir: &PathBuf) -> Result<Vec<DirEntry>> {
  let contents = fs::read_dir(dir)
    .with_context(|| format!("failed to read directory {}", dir.to_string_lossy()))?;
  Ok(contents.filter_map(|r| r.ok()).collect())
}

pub fn remote_files(dir: &PathBuf, sftp: &Sftp) -> Result<Vec<PathBuf>> {
  Ok(
    sftp
      .readdir(dir)
      .with_context(|| format!("failed to read remote directory {}", dir.to_string_lossy()))?
      .into_iter()
      .map(|(buf, _)| buf)
      .collect(),
  )
}

pub fn download_from_remote_host_recursive(
  path_buf: &PathBuf,
  target_dir: &PathBuf,
  sftp: &Sftp,
  follow_symlinks: bool,
) -> Result<()> {
  for f in remote_files(path_buf, sftp)? {
    // TODO
    if follow_symlinks && f.is_symlink() {
      continue;
    }

    if f.is_dir() {
      let new_target = match f.file_name() {
        Some(name) => target_dir.join(name),
        None => unreachable!("no file name"),
      };
      download_from_remote_host_recursive(&f, &new_target, sftp, follow_symlinks)?;
    }

    download_file_from_remote_host(&f, target_dir, sftp)?;
  }

  Ok(())
}

fn download_file_from_remote_host(
  path_buf: &PathBuf,
  target_dir: &PathBuf,
  sftp: &Sftp,
) -> Result<()> {
  let filename = path_buf.file_name().unwrap_or(&OsStr::new("unknown")); // TODO
  let mut dest = File::create(target_dir.join(filename))
    .with_context(|| format!("failed to create local file {}", filename.to_string_lossy()))?;

  let mut contents = sftp
    .open(path_buf)
    .with_context(|| format!("failed to read remote path {}", path_buf.to_string_lossy()))?;
  io::copy(&mut contents, &mut dest).with_context(|| {
    format!(
      "failed to download file {} to {}",
      filename.to_string_lossy(),
      target_dir.to_string_lossy()
    )
  })?;

  Ok(())
}
