use anyhow::{Context, Result, bail};
use ssh2::{Session, Sftp};
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

pub fn connect_sftp(host: &str, auth: AuthMethod) -> Result<Sftp> {
  let tcpstream = TcpStream::connect(host).context("failed to connect to remote host")?;

  let mut sess = Session::new().context("failed to construct SSH session with remote host")?;
  sess.set_tcp_stream(tcpstream);
  sess
    .handshake()
    .context("failed session handshake with remote host")?;

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
    }
  }

  if !sess.authenticated() {
    bail!("session not authenticated");
  }

  let sftp = sess.sftp().context("failed to initialize SFTP subsystem")?;

  return Ok(sftp);
}
