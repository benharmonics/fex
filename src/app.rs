use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ArgMatches;
use ratatui::{
  prelude::{Buffer, Rect},
  style::Style,
  widgets::Widget,
};
use ssh2::Sftp;

use crate::sftp::{self, AuthMethod};

pub struct App {
  pub should_quit: bool,
  local_path: PathBuf,
  remote_path: PathBuf,
  sftp: Sftp,
}

fn parse_user_and_host(raw: &str) -> (Option<String>, String) {
  if let Some((username, host)) = raw.split_once('@') {
    (Some(username.into()), host.into())
  } else {
    (None, raw.into())
  }
}

impl App {
  pub fn new(matches: ArgMatches) -> Result<Self> {
    let (user, host) = parse_user_and_host(
      matches
        .get_one::<String>("host")
        .expect("host is required argument"),
    );
    let username = user.unwrap_or_else(|| {
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
      local_path: PathBuf::new(),
      remote_path: PathBuf::new(),
      sftp: sftp::connect_sftp(sftp::ConnectSftpParams {
        username: &username,
        host: &host,
        port: 22, // TODO
        auth,
      })
      .with_context(|| format!("failed to connect to host {host} with username {username}"))?,
    })
  }
}

impl Widget for &App {
  fn render(self, area: Rect, buf: &mut Buffer) {
    // Render stuff
    buf.set_string(area.x, area.y, "Hello world", Style::new());
  }
}
