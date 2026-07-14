# AGENTS Guide

## Project Snapshot
- **Language:** Rust (edition 2024)
- **Purpose:** Terminal UI SFTP file transfer client with split local/remote panes.
- **Entry point:** `src/main.rs` calls `fex::run()` from `src/lib.rs`.
- **Core modules:**
  - `src/app.rs`: TUI state machine, key handling, render loop, transfer trigger logic
  - `src/sftp.rs`: SSH/SFTP connect flow, host key verification, authentication
  - `src/files.rs`: local/remote listing and recursive remote download implementation
  - `src/args.rs`: CLI argument definitions and parsing

## Essential Commands
Observed from repository files/config:

```bash
# run the app (HOST format: [username@]host[:port])
cargo run -- <HOST>

# run tests
cargo test

# build binary
cargo build

# install from local checkout (README)
cargo install --path /absolute/path/to/fex
```

Formatting is configured via `.rustfmt.toml` (`tab_spaces = 2`), so use rustfmt-compatible formatting:

```bash
cargo fmt
```

## CLI and Connection Behavior
- CLI arguments are defined in `src/args.rs`:
  - positional `HOST` in `[username@]host[:port]` format
  - optional `--identity/-i <PATH>` for private key auth
  - optional `--passphrase/-p <STRING>` used with identity file
- Host parsing happens in `parse_host_address` (`src/app.rs`). It uses `split_once('@')` and `rsplit_once(':')`.
  - Non-obvious implication: this parser is simple and may not handle IPv6 host notation.
- If username is omitted, fallback is `whoami::username()` (`src/app.rs`).

## App Architecture and Control Flow
1. `run()` (`src/lib.rs`) creates `App`, enables raw mode, enters alternate screen.
2. Main loop: render frame then process one event (`App::update`).
3. Event producer thread (`spawn_app_event_threads` in `src/app.rs`) reads crossterm events and sends key presses on an mpsc channel.
4. On exit, raw mode and alternate screen are restored in `run()`.

### Input Handling Currently Implemented
In `App::update` (`src/app.rs`), active key bindings are:
- `q` to quit
- `j`/Down to move selection down
- `k`/Up to move selection up
- `Tab` to switch pane focus
- `Enter` to trigger transfer action

## Transfer Behavior
- `Enter` with **remote pane focused** triggers recursive download:
  - `files::download_from_remote_host_recursive(selected, &local_path, &sftp, false)`
- `Enter` with **local pane focused** is currently TODO (upload path not implemented).
- Recursive downloader logic lives in `src/files.rs`:
  - Traverses remote entries via `Sftp::readdir`
  - Recurses on directories
  - Copies file contents with `io::copy` from remote handle to local file

## Authentication and Security Expectations
- SSH/SFTP setup in `connect_sftp` (`src/sftp.rs`):
  1. TCP connect
  2. SSH handshake
  3. host key verification against `$HOME/.ssh/known_hosts`
  4. authentication
  5. SFTP session creation
- Host key mismatches and unknown hosts are hard failures (`bail!`), not interactive prompts.
- Auth order in `authenticate`:
  1. Try SSH agent identities first
  2. Fall back to selected auth method
- `AuthMethod::PasswordInput` is currently `unimplemented!()`.

## Non-Obvious Gotchas
- `README.md` lists many controls, but current `App::update` only handles a subset (`q/j/k/tab/enter` plus arrows for j/k). Treat README key list as aspirational unless code confirms behavior.
- `AppEvent::DownloadComplete` and `AppEvent::Tick` exist but are not currently produced; matching arms are `todo!()`.
- `main.rs` currently prints the same error multiple times before exiting. This is current behavior and may affect debugging output readability.
- `files.rs` has a symlink gate in recursive download controlled by `follow_symlinks`; verify intent before modifying recursive traversal behavior.

## Style and Conventions Observed
- Indentation follows 2 spaces (`.rustfmt.toml`).
- Error handling pattern: `anyhow::{Result, Context, bail}` with contextual messages.
- Strong preference for explicit, user-facing error context in each fallible step.
- Module layout is flat under `src/` with `lib.rs` declaring internal modules.

## Testing Status and Approach
- No dedicated `tests/` directory observed.
- No inline unit tests observed in `src/*.rs`.
- Default validation path is currently `cargo test` (compilation-level confidence + any future tests).

## Practical Editing Guidance for Agents
- When changing key bindings, update both `src/app.rs` behavior and `README.md` controls together to avoid drift.
- When touching auth flows in `src/sftp.rs`, preserve host key verification semantics unless intentionally changing trust policy.
- For transfer features, changes typically span both `src/app.rs` (trigger/state/UI) and `src/files.rs` (file operation mechanics).
- Keep error context messages detailed; this project already relies on contextualized `anyhow` errors for diagnostics.
