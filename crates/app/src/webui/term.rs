//! The agents' terminals: one PTY per tab, its bytes passed to xterm.js.
//!
//! A tab runs the user's login shell, or an agent's command inside it
//! (`$SHELL -l -i -c 'exec claude'`), so the agent finds the PATH and the
//! keys the user's own terminal gives it: the app knows a command's name and
//! nothing else about an agent — no models, no keys, no config.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::{Context, Result, bail};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// What a tab can run: the shell, or an agent by its command's name. The page
/// asks by kind, never by command line.
pub const AGENTS: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("codex", "codex"),
    ("agy", "agy"),
    ("gemini", "gemini"),
    ("opencode", "opencode"),
];

/// What the PTYs said since the last frame.
pub enum TermOut {
    Data(String, Vec<u8>),
    /// The process ended; its exit code, when it had one.
    Exit(String, Option<u32>),
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

/// Every open tab's PTY. Its readers run on their own threads and send here.
pub struct Terms {
    sessions: HashMap<String, Session>,
    tx: Sender<TermOut>,
    rx: Receiver<TermOut>,
}

impl Default for Terms {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            sessions: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl Terms {
    pub fn open(&mut self, id: &str, kind: &str, cols: u16, rows: u16, cwd: &Path) -> Result<()> {
        if self.sessions.contains_key(id) {
            bail!("terminal {id} is already open");
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = CommandBuilder::new(&shell);
        if kind == "shell" {
            cmd.arg("-l");
        } else {
            let Some((_, program)) = AGENTS.iter().find(|(name, _)| *name == kind) else {
                bail!("no agent called {kind}");
            };
            // Interactive as well as login: PATH is often set in .zshrc.
            cmd.args(["-l", "-i", "-c", &format!("exec {program}")]);
        }
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "LocalGPT");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        if std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
        }

        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("opening a PTY")?;
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("starting {shell} for {kind}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let killer = child.clone_killer();

        let tx = self.tx.clone();
        let tab = id.to_string();
        std::thread::Builder::new()
            .name(format!("pty-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx
                                .send(TermOut::Data(tab.clone(), buf[..n].to_vec()))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
                let code = child.wait().ok().map(|status| status.exit_code());
                let _ = tx.send(TermOut::Exit(tab, code));
            })?;

        self.sessions.insert(
            id.to_string(),
            Session {
                master: pair.master,
                writer,
                killer,
            },
        );
        Ok(())
    }

    pub fn input(&mut self, id: &str, data: &[u8]) -> Result<()> {
        let session = self.sessions.get_mut(id).context("no such terminal")?;
        session.writer.write_all(data)?;
        session.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let session = self.sessions.get(id).context("no such terminal")?;
        session.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn close(&mut self, id: &str) {
        if let Some(mut session) = self.sessions.remove(id) {
            let _ = session.killer.kill();
        }
    }

    /// Everything the readers sent, in order; a tab whose process ended is
    /// forgotten here, after its last bytes.
    pub fn drain(&mut self) -> Vec<TermOut> {
        let out: Vec<TermOut> = self.rx.try_iter().collect();
        for item in &out {
            if let TermOut::Exit(id, _) = item {
                self.sessions.remove(id);
            }
        }
        out
    }
}

impl Drop for Terms {
    fn drop(&mut self) {
        for session in self.sessions.values_mut() {
            let _ = session.killer.kill();
        }
    }
}
