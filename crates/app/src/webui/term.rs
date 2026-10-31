//! The agents' terminals: one PTY per tab, its bytes passed to xterm.js.
//!
//! A tab runs the user's login shell, or an agent's command inside it
//! (`$SHELL -l -i -c 'exec claude'`), so the agent finds the PATH and the
//! keys the user's own terminal gives it: the app knows a command's name and
//! nothing else about an agent — no models, no keys, no config.
//!
//! Every page attached to the app sees every tab — the window's, and a paired
//! device's (`remote.rs`) — so each tab keeps its screen as a terminal would
//! (`vt100`), and a page that attaches late is sent the screen, not the
//! history. A PTY has one size and a phone is narrower: the page that typed
//! last sets it. And the title an agent sets says whether it is working, so
//! the end of its turn is seen.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::{Context, Result, bail};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

use super::Client;

/// What a tab can run: the shell, or an agent by its command's name. The page
/// asks by kind, never by command line.
pub const AGENTS: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("codex", "codex"),
    ("agy", "agy"),
    ("gemini", "gemini"),
    ("opencode", "opencode"),
];

/// What the tabs said since the last frame.
pub enum TermOut {
    Data(String, Vec<u8>),
    /// The process ended; its exit code, when it had one.
    Exit(String, Option<u32>),
    /// The PTY took another size: another page drives it now.
    Size(String, u16, u16),
    /// The agent's title changed what it says it is doing, or what it calls
    /// the task.
    State(String, Option<String>, Option<Activity>),
    /// An agent stopped working — its turn ended, or it is asking: the
    /// agent, by its command's name.
    TurnEnded(String),
    /// A tab opened or ended.
    Tabs,
}

/// What an agent says it is doing, by the marks it puts in its title.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Activity {
    Working,
    /// Stopped on a question for the person: a permission, a choice.
    Asking,
    Idle,
}

/// A tab as every page lists it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Tab {
    pub id: String,
    pub kind: String,
    pub cols: u16,
    pub rows: u16,
    /// What the agent calls the task, from its title.
    pub label: Option<String>,
    pub activity: Option<Activity>,
}

struct Session {
    id: String,
    kind: String,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// The screen as the PTY's output has drawn it.
    screen: vt100::Parser<Titles>,
    /// The PTY's size, columns then rows.
    size: (u16, u16),
    sizes: Sizes,
    label: Option<String>,
    activity: Option<Activity>,
}

impl Session {
    /// Gives the PTY and the screen `size`; whether it was another.
    fn apply(&mut self, size: (u16, u16)) -> Result<bool> {
        let (cols, rows) = clamp(size);
        if self.size == (cols, rows) {
            return Ok(false);
        }
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        self.screen.set_size(nonzero(rows), nonzero(cols));
        self.size = (cols, rows);
        Ok(true)
    }

    /// What a terminal needs to draw this screen from nothing: a reset, the
    /// alternate screen when the program is on it, then the cells, the
    /// cursor and the input modes.
    fn snapshot(&self) -> Vec<u8> {
        snapshot(self.screen.screen())
    }

    fn tab(&self) -> Tab {
        Tab {
            id: self.id.clone(),
            kind: self.kind.clone(),
            cols: self.size.0,
            rows: self.size.1,
            label: self.label.clone(),
            activity: self.activity,
        }
    }
}

fn snapshot(screen: &vt100::Screen) -> Vec<u8> {
    let mut out = b"\x1bc".to_vec();
    if screen.alternate_screen() {
        out.extend_from_slice(b"\x1b[?1049h");
    }
    out.extend_from_slice(screen.state_formatted().as_bytes());
    out
}

/// Every open tab's PTY, in the order they opened. Its readers run on their
/// own threads and send here.
pub struct Terms {
    sessions: Vec<Session>,
    tx: Sender<TermOut>,
    rx: Receiver<TermOut>,
    /// What changed here rather than in a PTY, for the next drain.
    pending: Vec<TermOut>,
}

impl Default for Terms {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            sessions: Vec::new(),
            tx,
            rx,
            pending: Vec::new(),
        }
    }
}

impl Terms {
    /// Starts a tab for the page `by`, at its size.
    pub fn open(
        &mut self,
        id: &str,
        kind: &str,
        size: (u16, u16),
        cwd: &Path,
        by: Client,
    ) -> Result<()> {
        if self.sessions.iter().any(|s| s.id == id) {
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

        let (cols, rows) = clamp(size);
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

        self.sessions.push(Session {
            id: id.to_string(),
            kind: kind.to_string(),
            master: pair.master,
            writer,
            killer,
            screen: vt100::Parser::new_with_callbacks(
                nonzero(rows),
                nonzero(cols),
                0,
                Titles::default(),
            ),
            size: (cols, rows),
            sizes: Sizes::opened(by, (cols, rows)),
            label: None,
            activity: None,
        });
        self.pending.push(TermOut::Tabs);
        Ok(())
    }

    /// A page shows a tab it did not open: what fits it is noted, and it gets
    /// the screen to start from, with the size it is drawn at. `None` when
    /// there is no such tab.
    pub fn attach(
        &mut self,
        id: &str,
        by: Client,
        size: (u16, u16),
    ) -> Option<(Vec<u8>, (u16, u16))> {
        let Self {
            sessions, pending, ..
        } = self;
        let session = sessions.iter_mut().find(|s| s.id == id)?;
        if let Some(size) = session.sizes.ask(by, size) {
            resize(session, size, pending);
        }
        Some((session.snapshot(), session.size))
    }

    /// Typing drives: the tab takes the typing page's size first.
    pub fn input(&mut self, id: &str, by: Client, data: &[u8]) -> Result<()> {
        let Self {
            sessions, pending, ..
        } = self;
        let session = sessions
            .iter_mut()
            .find(|s| s.id == id)
            .context("no such terminal")?;
        if let Some(size) = session.sizes.take(by) {
            resize(session, size, pending);
        }
        session.writer.write_all(data)?;
        session.writer.flush()?;
        Ok(())
    }

    /// What fits the page `by` now; the PTY takes it while that page drives.
    pub fn resize(&mut self, id: &str, by: Client, size: (u16, u16)) -> Result<()> {
        let Self {
            sessions, pending, ..
        } = self;
        let session = sessions
            .iter_mut()
            .find(|s| s.id == id)
            .context("no such terminal")?;
        if let Some(size) = session.sizes.ask(by, size) {
            resize(session, size, pending);
        }
        Ok(())
    }

    /// The page `by` was clicked into the tab: it drives, as if it had typed.
    pub fn focus(&mut self, id: &str, by: Client) {
        let Self {
            sessions, pending, ..
        } = self;
        if let Some(session) = sessions.iter_mut().find(|s| s.id == id)
            && let Some(size) = session.sizes.take(by)
        {
            resize(session, size, pending);
        }
    }

    /// The page `by` went away: the tabs it drove go back to the window.
    pub fn leave(&mut self, by: Client) {
        let Self {
            sessions, pending, ..
        } = self;
        for session in sessions.iter_mut() {
            if let Some(size) = session.sizes.leave(by) {
                resize(session, size, pending);
            }
        }
    }

    pub fn close(&mut self, id: &str) {
        if let Some(at) = self.sessions.iter().position(|s| s.id == id) {
            let mut session = self.sessions.remove(at);
            let _ = session.killer.kill();
            self.pending.push(TermOut::Tabs);
        }
    }

    pub fn tabs(&self) -> Vec<Tab> {
        self.sessions.iter().map(Session::tab).collect()
    }

    /// Everything that happened, in order: the readers' bytes (each drawn on
    /// its tab's screen first, and what its title says after), and what the
    /// pages changed. A tab whose process ended is forgotten here, after its
    /// last bytes.
    pub fn drain(&mut self) -> Vec<TermOut> {
        let mut out = std::mem::take(&mut self.pending);
        let arrived: Vec<TermOut> = self.rx.try_iter().collect();
        for item in arrived {
            match item {
                TermOut::Data(id, bytes) => {
                    let said = self
                        .sessions
                        .iter_mut()
                        .find(|s| s.id == id)
                        .map(|session| draw(session, &bytes))
                        .unwrap_or_default();
                    out.push(TermOut::Data(id, bytes));
                    out.extend(said);
                }
                TermOut::Exit(id, code) => {
                    let known = self.sessions.iter().position(|s| s.id == id);
                    if let Some(at) = known {
                        self.sessions.remove(at);
                    }
                    out.push(TermOut::Exit(id, code));
                    if known.is_some() {
                        out.push(TermOut::Tabs);
                    }
                }
                other => out.push(other),
            }
        }
        out
    }
}

impl Drop for Terms {
    fn drop(&mut self) {
        for session in &mut self.sessions {
            let _ = session.killer.kill();
        }
    }
}

fn resize(session: &mut Session, size: (u16, u16), pending: &mut Vec<TermOut>) {
    match session.apply(size) {
        Ok(true) => pending.push(TermOut::Size(
            session.id.clone(),
            session.size.0,
            session.size.1,
        )),
        Ok(false) => {}
        Err(e) => tracing::debug!("terminal {}: {e}", session.id),
    }
}

/// Draws a chunk on the tab's screen; what its titles said, in order.
fn draw(session: &mut Session, bytes: &[u8]) -> Vec<TermOut> {
    session.screen.process(bytes);
    let titles = std::mem::take(&mut session.screen.callbacks_mut().0);
    let mut said = Vec::new();
    for title in titles {
        let label = Some(label(&title)).filter(|l| !l.is_empty());
        let now = activity(&title);
        if label == session.label && now == session.activity {
            continue;
        }
        let was = session.activity;
        session.label = label.clone();
        session.activity = now;
        said.push(TermOut::State(session.id.clone(), label, now));
        let stopped = matches!(now, Some(Activity::Idle | Activity::Asking));
        if was == Some(Activity::Working) && stopped && session.kind != "shell" {
            said.push(TermOut::TurnEnded(session.kind.clone()));
        }
    }
    said
}

/// The titles a chunk of output set, in order.
#[derive(Default)]
struct Titles(Vec<String>);

impl vt100::Callbacks for Titles {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.0.push(String::from_utf8_lossy(title).into_owned());
    }

    // A title with a `;` in it arrives in pieces.
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        if let [b"0" | b"2", title @ ..] = params
            && !title.is_empty()
        {
            self.0
                .push(String::from_utf8_lossy(&title.join(&b';')).into_owned());
        }
    }
}

/// The marks agents put at the front of their titles: spinner frames while
/// they work (Claude Code's braille, then its quarter circles; Gemini CLI's
/// ✦ and ⏲), ✋ when they ask, ✳ and ◇ at rest. Read as Orca reads them.
fn spinner(c: char) -> bool {
    matches!(c, '\u{2800}'..='\u{28FF}' | '\u{25D0}'..='\u{25D3}' | '\u{2726}' | '\u{23F2}')
}

fn mark(c: char) -> bool {
    spinner(c) || matches!(c, '\u{270B}' | '\u{2733}' | '\u{25C7}')
}

/// What a title says an agent is doing. A title without the marks — the
/// shell's own once an agent in it exits, a folder — says it is not known:
/// a tab is never left working after its agent has gone.
pub fn activity(title: &str) -> Option<Activity> {
    if title.contains('\u{270B}') {
        Some(Activity::Asking)
    } else if title.chars().any(spinner) {
        Some(Activity::Working)
    } else if title.starts_with('\u{2733}') || title.contains('\u{25C7}') {
        Some(Activity::Idle)
    } else {
        None
    }
}

/// A title without its marks: what the agent calls the task.
fn label(title: &str) -> String {
    title
        .trim_start_matches(|c: char| mark(c) || c.is_whitespace())
        .trim()
        .to_string()
}

/// Who sets a tab's size. Each page says what fits it; a PTY has one size,
/// so the page that typed or focused the tab last gets its own — as tmux
/// gives the latest client its size — and when a device goes, the window
/// takes it back.
#[derive(Debug, Default, PartialEq)]
struct Sizes {
    fits: Vec<(Client, (u16, u16))>,
    driver: Option<Client>,
}

impl Sizes {
    fn opened(by: Client, size: (u16, u16)) -> Self {
        Self {
            fits: vec![(by, size)],
            driver: Some(by),
        }
    }

    fn fit(&self, by: Client) -> Option<(u16, u16)> {
        self.fits.iter().find(|(c, _)| *c == by).map(|(_, s)| *s)
    }

    /// What fits `by` now: the size to give the PTY when `by` drives (or
    /// nobody does), else nothing.
    fn ask(&mut self, by: Client, size: (u16, u16)) -> Option<(u16, u16)> {
        match self.fits.iter_mut().find(|(c, _)| *c == by) {
            Some((_, fit)) => *fit = size,
            None => self.fits.push((by, size)),
        }
        match self.driver {
            Some(driver) if driver != by => None,
            _ => {
                self.driver = Some(by);
                Some(size)
            }
        }
    }

    /// `by` typed or focused: it drives now, at its size.
    fn take(&mut self, by: Client) -> Option<(u16, u16)> {
        if self.driver == Some(by) {
            return None;
        }
        let size = self.fit(by)?;
        self.driver = Some(by);
        Some(size)
    }

    /// `by` went away. When it drove, the window does again, or whoever is
    /// left: the size to give the PTY.
    fn leave(&mut self, by: Client) -> Option<(u16, u16)> {
        self.fits.retain(|(c, _)| *c != by);
        if self.driver != Some(by) {
            return None;
        }
        let next = self
            .fits
            .iter()
            .find(|(c, _)| *c == Client::Window)
            .or(self.fits.first())
            .copied();
        self.driver = next.map(|(c, _)| c);
        next.map(|(_, size)| size)
    }
}

/// A PTY of no size is no terminal: one cell at least, each way.
fn clamp((cols, rows): (u16, u16)) -> (u16, u16) {
    (cols.max(1), rows.max(1))
}

fn nonzero(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_says_what_the_agent_is_doing() {
        // Claude Code: braille frames, then quarter circles, while working;
        // ✳ at rest.
        assert_eq!(activity("⠂ Fix the jetty"), Some(Activity::Working));
        assert_eq!(activity("◐ Fix the jetty"), Some(Activity::Working));
        assert_eq!(activity("✳ Fix the jetty"), Some(Activity::Idle));
        // Gemini CLI.
        assert_eq!(activity("✦ Gemini CLI"), Some(Activity::Working));
        assert_eq!(activity("✋ Gemini CLI"), Some(Activity::Asking));
        assert_eq!(activity("◇ Gemini CLI"), Some(Activity::Idle));
        // A shell's title says nothing.
        assert_eq!(activity("~/worlds/harbour"), None);
        assert_eq!(activity(""), None);
        assert_eq!(label("⠐ Fix the jetty "), "Fix the jetty");
        assert_eq!(label("✳ Claude Code"), "Claude Code");
    }

    fn screen(rows: u16, cols: u16) -> vt100::Parser<Titles> {
        vt100::Parser::new_with_callbacks(nonzero(rows), nonzero(cols), 0, Titles::default())
    }

    /// A page that attaches late draws the screen the tab shows, not what
    /// was written to it.
    #[test]
    fn a_snapshot_draws_the_screen_again() {
        let mut tab = screen(6, 30);
        tab.process(b"one\r\ntwo\r\n\x1b[1;31mred\x1b[0m and plain\r\n> ");
        // A TUI redraws in place: the cursor goes up and overwrites.
        tab.process(b"\x1b[2;1Htwo, again\x1b[4;3H");
        let mut late = screen(6, 30);
        late.process(&snapshot(tab.screen()));
        assert_eq!(late.screen().contents(), tab.screen().contents());
        assert_eq!(late.screen().cursor_position(), (3, 2));
        let red = late.screen().cell(2, 0).expect("a cell");
        assert!(red.bold());
        assert_eq!(red.fgcolor(), vt100::Color::Idx(1));
        // A program on the alternate screen is drawn there.
        tab.process(b"\x1b[?1049h\x1b[Hfull screen");
        let mut late = screen(6, 30);
        late.process(&snapshot(tab.screen()));
        assert!(late.screen().alternate_screen());
        assert_eq!(late.screen().contents(), tab.screen().contents());
    }

    #[test]
    fn titles_arrive_in_order_even_with_a_semicolon_in_them() {
        let mut tab = screen(4, 20);
        tab.process(b"\x1b]0;\xe2\xa0\x82 Fix it\x07work\x1b]2;\xe2\x9c\xb3 a; b\x07");
        assert_eq!(tab.callbacks().0, ["⠂ Fix it", "✳ a; b"]);
    }

    #[test]
    fn the_page_that_typed_last_sets_the_size() {
        let phone = Client::Device(1);
        let mut sizes = Sizes::opened(Client::Window, (120, 40));
        // The phone attaches: noted, and the window keeps driving.
        assert_eq!(sizes.ask(phone, (48, 30)), None);
        // The window's own fit applies while it drives.
        assert_eq!(sizes.ask(Client::Window, (118, 40)), Some((118, 40)));
        // The phone types: its size.
        assert_eq!(sizes.take(phone), Some((48, 30)));
        assert_eq!(sizes.take(phone), None, "already driving");
        // The window lays out again while the phone drives: noted only.
        assert_eq!(sizes.ask(Client::Window, (100, 40)), None);
        // The person clicks into the tab on the desktop: the window's again.
        assert_eq!(sizes.take(Client::Window), Some((100, 40)));
        // The phone types, then goes: the window takes it back.
        sizes.take(phone);
        assert_eq!(sizes.leave(phone), Some((100, 40)));
        assert_eq!(sizes.driver, Some(Client::Window));
        // A page that never said what fits it cannot drive.
        assert_eq!(sizes.take(Client::Device(2)), None);
        // One that leaves without driving changes nothing.
        sizes.ask(phone, (48, 30));
        assert_eq!(sizes.leave(phone), None);
    }
}
