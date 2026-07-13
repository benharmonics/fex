use std::path::PathBuf;

use anyhow::{Context, Result, bail};
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

pub struct HostAddress {
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
      local_path: PathBuf::new(),
      remote_path: PathBuf::new(),
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
}

impl Widget for &App {
  fn render(self, area: Rect, buf: &mut Buffer) {
    // Render stuff
    buf.set_string(area.x, area.y, "Hello world", Style::new());
  }
}
