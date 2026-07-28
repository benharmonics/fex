use anyhow::{Context, Result, bail};
use ssh2::{CheckResult, KnownHostFileKind, Session, Sftp};
use std::{
  net::TcpStream,
  path::{Path, PathBuf},
};

#[derive(Clone)]
pub enum AuthMethod {
  PrivateKey {
    key_path: PathBuf,
    passphrase: Option<String>,
  },
  PasswordInput {
    password: String,
  },
}

pub struct ConnectSftpParams<'a> {
  pub username: &'a str,
  pub host: &'a str,
  pub port: u16,
  pub auth: AuthMethod,
}

pub fn connect_sftp(ps: &ConnectSftpParams) -> Result<Sftp> {
  let addr = format!("{}:{}", ps.host, ps.port);
  let tcpstream = TcpStream::connect(&addr).context("failed to construct TCP stream")?;

  let mut sess = Session::new().context("failed to create session")?;
  sess.set_tcp_stream(tcpstream);
  sess.handshake().context("failed SSH handshake")?;

  verify_host_key(&sess, ps.host).context("host validation failed")?;
  authenticate(&sess, ps.username, ps.auth.clone()).context("authentication failed")?;

  let sftp = sess.sftp().context("failed to generate SFTP context")?;

  Ok(sftp)
}

fn verify_host_key(sess: &Session, host: &str) -> Result<()> {
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

fn authenticate(sess: &Session, username: &str, auth: AuthMethod) -> Result<()> {
  if try_agent(username, sess).context("failed to try SSH agent")? {
    return Ok(());
  }

  match auth {
    AuthMethod::PrivateKey {
      key_path,
      passphrase,
    } => {
      sess
        .userauth_pubkey_file(username, None, Path::new(&key_path), passphrase.as_deref())
        .context("failed to set pubkey auth")?;
    }
    AuthMethod::PasswordInput { password } => {
      sess
        .userauth_password(username, &password)
        .context("failed password authentication")?;
    }
  }

  if !sess.authenticated() {
    bail!("session not authenticated");
  }

  Ok(())
}

fn try_agent(username: &str, sess: &Session) -> Result<bool> {
  if let Ok(mut agent) = sess.agent()
    && agent.connect().is_ok()
    && agent.list_identities().is_ok()
  {
    for identity in agent.identities()? {
      if agent.userauth(username, &identity).is_ok() {
        return Ok(true);
      }
    }
  }
  Ok(false)
}
