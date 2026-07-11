use anyhow::{Context, Result};
use fex::sftp::AuthMethod;
use ratatui::{DefaultTerminal, Frame, crossterm};
use std::path::PathBuf;

fn parse_user_and_host(raw: &str) -> (Option<String>, String) {
  if let Some((username, host)) = raw.split_once('@') {
    (Some(username.into()), host.into())
  } else {
    (None, raw.into())
  }
}

fn app(terminal: &mut DefaultTerminal) -> Result<()> {
  loop {
    terminal.draw(render).context("failed to render terminal")?;
    if crossterm::event::read()
      .context("failed to read crossterm event")?
      .is_key_press()
    {
      break Ok(());
    }
  }
}

fn render(frame: &mut Frame) {
  frame.render_widget("hello world", frame.area());
}

fn main() {
  let matches = fex::args::get_matches();
  let (user, host) = parse_user_and_host(
    matches
      .get_one::<String>("host")
      .expect("host is required argument"),
  );
  let username = user.unwrap_or_else(|| {
    whoami::username().expect("failed to get username as fallback when none provided")
  });
  let passphrase = matches.get_one::<String>("passphrase").cloned();

  let auth = match matches.get_one::<PathBuf>("identity").cloned() {
    Some(key_path) => AuthMethod::PrivateKey {
      username,
      key_path,
      passphrase,
    },
    None => AuthMethod::PasswordInput { username },
  };

  ratatui::run(app).unwrap_or_else(|e| {
    eprintln!("Unexpected failure: {e}.");
    std::process::exit(1);
  })
}
