use std::path::PathBuf;

use clap::{ArgMatches, Command, arg, value_parser};

pub fn get_matches() -> ArgMatches {
  Command::new("fex")
    .arg(
      arg!(
        -i --identity <IDENTITY> "Selects the file from which the identity (private key) for public key authentication is read.  This option is directly passed to ssh(1)."
      )
      .required(false)
      .value_parser(value_parser!(PathBuf))
    )
    .arg(
      arg!(
        -p --passphrase "If set, the app will prompt for the additional passphrase for your SSH key."
      )
      .required(false)
    )
    .arg(
      arg!(-w --workers <NUM_WORKERS> "The SFTP worker pool size for concurrent transfers")
      .default_value("4")
      .value_parser(value_parser!(usize))
    )
    .arg(
      arg!(<HOST> "The remote host in format [username@]host[:port]").id("host").value_parser(value_parser!(String))
    )
    .get_matches()
}
