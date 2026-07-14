use std::{
  fs::{self, DirEntry, File},
  io,
  path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ssh2::{FileStat, Sftp};

/// List all remote files on a given path.
pub fn local_files(dir: &PathBuf) -> Result<Vec<DirEntry>> {
  let contents = fs::read_dir(dir)
    .with_context(|| format!("failed to read directory {}", dir.to_string_lossy()))?;
  let mut files: Vec<DirEntry> = contents.filter_map(|r| r.ok()).collect();
  files.sort_by(|a, b| {
    a.file_name()
      .to_string_lossy()
      .cmp(&b.file_name().to_string_lossy())
  });
  Ok(files)
}

/// List all remote files on a given path. Note that certain operations on PathBuf are not available
/// on the remote host, and for these cases - like `buf.is_dir()` - FileStat is provided to
/// substitute.
pub fn remote_files(dir: &PathBuf, sftp: &Sftp) -> Result<Vec<(PathBuf, FileStat)>> {
  let mut ret = sftp
    .readdir(dir)
    .with_context(|| format!("failed to read remote directory {}", dir.to_string_lossy()))?;
  ret.sort_by(|(a, _), (b, _)| {
    a.file_name()
      .unwrap_or_default()
      .to_string_lossy()
      .cmp(&b.file_name().unwrap_or_default().to_string_lossy())
  });

  Ok(ret)
}

/// Download a given file or directory from the remote host to the local host.
pub fn download(
  path_buf: &PathBuf,
  stat: &FileStat,
  target_dir: &Path,
  sftp: &Sftp,
) -> Result<()> {
  if stat.is_file() {
    download_file_from_remote_host(path_buf, target_dir, sftp)?;
  }

  if stat.is_dir() {
    let new_dir = &target_dir.join(path_buf.file_name().context("unnamed directory")?);
    fs::create_dir_all(new_dir)
      .with_context(|| format!("failed to make directory {:?}", new_dir))?;
    for (f, stat) in &remote_files(path_buf, sftp)? {
      download(f, stat, new_dir, sftp)?;
    }
  }

  // TODO: symlinks?

  Ok(())
}

fn download_file_from_remote_host(
  path_buf: &PathBuf,
  target_dir: &Path,
  sftp: &Sftp,
) -> Result<()> {
  let filename = path_buf.file_name().context("no file name")?; // TODO
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
