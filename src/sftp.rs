use anyhow::{Context, Result, bail};
use ssh2::{CheckResult, KnownHostFileKind, Session, Sftp};
use std::{
  net::TcpStream,
  path::{Path, PathBuf},
};

pub enum AuthMethod {
  PrivateKey {
    username: String,
    key_path: PathBuf,
    passphrase: Option<String>,
  },
  PasswordCli {
    username: String,
    password: String,
  },
  PasswordInput {
    username: String,
  },
}

impl AuthMethod {
  pub fn username<'a>(&'a self) -> &'a str {
    match self {
      Self::PrivateKey {
        username,
        key_path: _,
        passphrase: _,
      } => username,
      Self::PasswordCli {
        username,
        password: _,
      } => username,
      Self::PasswordInput { username } => username,
    }
  }
}

pub fn connect_sftp(host: &str, auth: AuthMethod) -> Result<Sftp> {
  let tcpstream = TcpStream::connect(host).context("failed to connect to remote host")?;

  let mut sess = Session::new().context("failed to construct SSH session with remote host")?;
  sess.set_tcp_stream(tcpstream);
  sess
    .handshake()
    .context("failed session handshake with remote host")?;

  verify_host_key(&sess, host)?;
  authenticate(&sess, auth)?;

  let sftp = sess.sftp().context("failed to initialize SFTP subsystem")?;

  return Ok(sftp);
}

pub fn verify_host_key(sess: &Session, host: &str) -> Result<()> {
  let mut known_hosts = sess
    .known_hosts()
    .context("failed to get session's known hosts")?;

  let home = std::env::var("HOME").context("environment variable HOME not set")?;
  let known_hosts_path: PathBuf = [home.as_str(), ".ssh/known_hosts"].iter().collect();

  known_hosts
    .read_file(&known_hosts_path, KnownHostFileKind::OpenSSH)
    .context("failed to read known hosts file")?;
  let (key, _key_type) = sess.host_key().context("failed to get session host key")?;

  match known_hosts.check(host, key) {
    CheckResult::Match => {
      // trusted
    }
    CheckResult::Mismatch => {
      bail!(
        "mismatch with host: possible man-in-the-middle attack: {}",
        host
      );
    }
    CheckResult::NotFound => {
      bail!("host not found in known hosts: {}", host);
    }
    CheckResult::Failure => {
      bail!("failed to check host key for host {}", host);
    }
  }

  Ok(())
}

fn authenticate(sess: &Session, auth: AuthMethod) -> Result<()> {
  if try_agent(auth.username(), sess).context("failed to try SSH agent")? {
    return Ok(());
  }

  match auth {
    AuthMethod::PrivateKey {
      username,
      key_path,
      passphrase,
    } => {
      sess
        .userauth_pubkey_file(&username, None, Path::new(&key_path), passphrase.as_deref())
        .context("failed to set pubkey auth")?;
    }
    AuthMethod::PasswordCli { username, password } => {
      sess
        .userauth_password(&username, &password)
        .context("failed password authentication")?;
    }
    AuthMethod::PasswordInput { username } => {
      unimplemented!();
      // sess
      //   .userauth_password(&username, &password)
      //   .context("failed password authentication")?;
    }
  }

  if !sess.authenticated() {
    bail!("session not authenticated");
  }

  Ok(())
}

fn try_agent(username: &str, sess: &Session) -> Result<bool> {
  if let Ok(mut agent) = sess.agent() {
    if agent.connect().is_ok() && agent.list_identities().is_ok() {
      for identity in agent.identities()? {
        if agent.userauth(username, &identity).is_ok() {
          return Ok(true);
        }
      }
    }
  }
  Ok(false)
}
