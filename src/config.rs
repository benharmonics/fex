use std::{num::NonZero, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::ArgMatches;

use crate::sftp::AuthMethod;

struct SshConnectionConfig {
  host: String,
  port: u16,
  username: Option<String>,
}

impl SshConnectionConfig {
  fn parse(input: &str) -> Result<Self> {
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

    Ok(Self {
      username,
      host,
      port: port.unwrap_or(22),
    })
  }
}

pub struct AppConfig {
  pub sftp_pool_size: usize,
  cfg: SshConnectionConfig,
  identity: Option<PathBuf>,
  passphrase: bool,
}

impl AppConfig {
  pub fn parse_args(matches: &ArgMatches) -> Result<Self> {
    let host = SshConnectionConfig::parse(
      matches
        .get_one::<String>("host")
        .expect("host is required argument"),
    )
    .context("failed to parse host address")?;

    let sftp_pool_size = matches
      .get_one::<usize>("workers")
      .expect("required argument");
    let max_threads = std::thread::available_parallelism()
      .unwrap_or(NonZero::new(8).expect("plausible default max threads"));
    if !(2..=max_threads.into()).contains(sftp_pool_size) {
      bail!("worker pool size must be in [2, {max_threads}]");
    }

    Ok(Self {
      cfg: host,
      sftp_pool_size: *sftp_pool_size,
      passphrase: matches.get_flag("passphrase"),
      identity: matches.get_one::<PathBuf>("identity").cloned(),
    })
  }

  pub fn host(&self) -> String {
    self.cfg.host.clone()
  }

  pub fn port(&self) -> u16 {
    self.cfg.port
  }

  pub fn username(&self) -> String {
    self.cfg.username.clone().unwrap_or_else(|| {
      whoami::username().expect("failed to get username as fallback when none provided")
    })
  }

  pub fn auth(&self) -> AuthMethod {
    match self.identity.clone() {
      Some(key_path) => AuthMethod::PrivateKey {
        key_path: key_path.to_path_buf(),
        passphrase: if self.passphrase {
          Some(
            rpassword::prompt_password(format!(
              "Enter passphrase for private key {:?}: ",
              key_path
            ))
            .expect("failed to prompt for passphrase"),
          )
        } else {
          None
        },
      },
      None => AuthMethod::PasswordInput {
        password: rpassword::prompt_password(format!("Enter password for {}: ", self.username()))
          .expect("failed to prompt for password"),
      },
    }
  }
}
