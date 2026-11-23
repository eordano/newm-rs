//! Session save and restore: newm-rs remembers which windows are open and where, and at every
//! start relaunches their apps and puts each window back on its saved tiles.
//!
//! Wayland clients die with the compositor, so the windows are saved to
//! `$XDG_STATE_HOME/newm-rs/session.json` (readable by the user only: titles and command lines are
//! private) once a change has held for [`SAVE_INTERVAL`], and at a quit the user asks for. A
//! teardown (logout, reboot) that closes windows faster than that is never saved, so the last
//! settled session is what comes back after it, as after a crash.
//!
//! At start, after the config's `spawn-at-startup` programs, each saved canvas gets a placeholder
//! on every window's saved tiles, every saved process is launched once (a Chromium-family browser
//! told to restore its last session, see [`restoring`]), and once the windows took their
//! placeholders (or [`RESTORE_LIFETIME`] passed) each canvas takes its saved focus, zoom and view.
//!
//! Only the session on a TTY is saved and restored: a nested newm-rs (a window of another
//! session) would relaunch the outer session's apps there and overwrite its saved session.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use newm_canvas::Rect;
use newm_kde::{apps, exec};
use niri_config::utils::RegexEq;
use niri_config::{Config, SpawnAtStartup, SpawnShAtStartup};
use serde::{Deserialize, Serialize};

use crate::backend::Backend;
use crate::layout::canvas::{RestoreCanvas, RestoreWindow};
use crate::layout::{Layout, LayoutElement as _};
use crate::niri::State;
use crate::utils::spawning::spawn_with;
use crate::window::Mapped;

mod terminals;
use terminals::{Ask, Terminals};

/// How long a change must hold before it is saved; the session is written at most this often.
pub const SAVE_INTERVAL: Duration = Duration::from_secs(2);

/// How long a restore waits for its windows before it drops the placeholders left.
pub const RESTORE_LIFETIME: Duration = Duration::from_secs(20);

const OUTPUT_WAIT: Duration = Duration::from_millis(500);
const OUTPUT_TRIES: u32 = 20;

/// How long a quit waits for the session writer to save the session.
const QUIT_WRITE_WAIT: Duration = Duration::from_secs(3);

/// Parts of app IDs (any case) of prompts, agents and lockers, which are never saved nor
/// relaunched.
const NEVER: &[&str] = &[
    "polkit",
    "pinentry",
    "askpass",
    "xdg-desktop-portal",
    "org.freedesktop.impl.portal",
    "kwallet",
    "ksecretd",
    "org.kde.kded",
    "swaylock",
    "gtklock",
    "hyprlock",
    "newm-rs-lock",
];

/// Programs whose windows belong to other processes: their command lines bring back nothing.
const HELPERS: &[&str] = &["xwayland-satellite", "Xwayland"];

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub canvases: Vec<SavedCanvas>,
    /// What brings the windows back: one launch per process, or per window for a terminal.
    pub launches: Vec<Launch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedCanvas {
    /// The output's connector name.
    pub output: Option<String>,
    /// The workspace's index on its output.
    pub workspace: usize,
    pub zoom: i32,
    /// The camera's view, in logical pixels at 1:1.
    pub view: (f64, f64),
    /// Index of the focused window in `windows`.
    pub focus: Option<usize>,
    pub windows: Vec<SavedWindow>,
    pub floating: Vec<SavedFloating>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub app_id: String,
    pub title: Option<String>,
    /// Its tiles: column, row, width and height.
    pub rect: (i32, i32, i32, i32),
    /// Index in [`Session::launches`] of what brings it back.
    pub launch: Option<usize>,
}

/// A floating window: saved, and back when a tiled window's launch brings its process back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedFloating {
    pub app_id: String,
    pub title: Option<String>,
    /// Its position on the workspace and its size, in logical pixels.
    pub pos: (f64, f64),
    pub size: (f64, f64),
    pub launch: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Launch {
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
}

/// How a window comes back: with its process, launched once for all its windows, or on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relaunch {
    Process(Launch),
    Window(Launch),
}

/// A window as the saver finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// The window's ID in the compositor.
    pub id: u64,
    pub app_id: String,
    pub title: Option<String>,
    pub pid: Option<i32>,
}

/// Where a found window was.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Tiled(Rect),
    Floating((f64, f64), (f64, f64)),
}

/// Saves the session (see the module docs).
#[derive(Debug)]
pub struct Saver {
    path: Option<PathBuf>,
    /// Whether the saved session was put back (or there was none): before that, saving would
    /// overwrite it.
    ready: bool,
    /// The session last found, written once it holds for an interval.
    seen: Option<String>,
    written: Option<String>,
    /// The thread that writes the session file, started at the first write.
    writer: Option<Writer>,
    resolver: Resolver,
    terminals: Terminals,
}

/// A write of the session file: its text, and who waits for it to land.
type WriteJob = (String, Option<SyncSender<()>>);

/// Writes the session file on a thread of its own: [`write_private`] syncs it to disk, and an
/// fsync on a busy pool takes the tens of milliseconds in which the compositor would neither draw
/// nor read input. One thread writes them all, so they land in the order asked.
#[derive(Debug)]
struct Writer {
    tx: Option<SyncSender<WriteJob>>,
}

impl Writer {
    fn new(path: PathBuf) -> Self {
        let (tx, rx) = sync_channel::<WriteJob>(1);
        let tx = thread::Builder::new()
            .name(String::from("newm-rs-session"))
            .spawn(move || {
                while let Ok((json, done)) = rx.recv() {
                    if let Err(err) = write_private(&path, &json) {
                        warn!("error saving the session to {path:?}: {err:?}");
                    }
                    if let Some(done) = done {
                        let _ = done.send(());
                    }
                }
            })
            .inspect_err(|err| warn!("error starting the session writer: {err:?}"))
            .ok()
            .map(|_| tx);
        Self { tx }
    }
}

impl Default for Saver {
    fn default() -> Self {
        Self {
            path: session_path(),
            ready: false,
            seen: None,
            written: None,
            writer: None,
            resolver: Resolver::default(),
            terminals: Terminals::default(),
        }
    }
}

impl Saver {
    /// Saves the session now, the user quitting, and waits for it to land (after any write
    /// queued before it); skipped before the saved one is back.
    pub fn save_now(&mut self, layout: &Layout<Mapped>, config: &Config) {
        if let Some(json) = self.current(layout, config) {
            self.write_waiting(json);
        }
    }

    /// Writes `json` after any write queued before it, and waits for it to land.
    fn write_waiting(&mut self, json: String) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if let Some(tx) = self.writer.as_ref().and_then(|w| w.tx.as_ref()) {
            let (done_tx, done) = sync_channel(1);
            if tx.send((json.clone(), Some(done_tx))).is_ok()
                && done.recv_timeout(QUIT_WRITE_WAIT).is_ok()
            {
                self.written = Some(json);
                return;
            }
            warn!("the session writer did not answer; saving the session here");
        }
        match write_private(&path, &json) {
            Ok(()) => self.written = Some(json),
            Err(err) => warn!("error saving the session to {path:?}: {err:?}"),
        }
    }

    /// The host the terminal window `id` is connected to with ssh, from the terminal finder's
    /// last answer; `None` for a window that shows something local, or that it has not answered
    /// for yet.
    pub fn remote_host(&self, id: u64) -> Option<String> {
        terminals::ssh_host(&self.terminals.command(id)?)
    }

    /// Saves the session when it changed and the change held since the last tick.
    fn tick(&mut self, layout: &Layout<Mapped>, config: &Config) {
        let start = Instant::now();
        let Some(json) = self.current(layout, config) else {
            return;
        };
        if self.seen.as_ref() == Some(&json) {
            if self.written.as_ref() != Some(&json) {
                self.write(json);
            }
        } else {
            self.seen = Some(json);
        }
        crate::utils::log_slow("session tick", start);
    }

    fn current(&mut self, layout: &Layout<Mapped>, config: &Config) -> Option<String> {
        let canvas = &config.layout.canvas;
        if !self.ready || !canvas.restore_session || layout.canvas_restoring() {
            return None;
        }
        let home = home();
        self.resolver.processes.clear();
        let mut ask = Ask::default();
        let terminals = &self.terminals;
        let session = snapshot(layout, &canvas.restore_exclude, |w| {
            if let Some(terminal) = terminals::find(&w.app_id) {
                ask.windows.push(terminals::Window {
                    id: w.id,
                    pid: w.pid,
                    terminal,
                    title: w
                        .title
                        .as_deref()
                        .map(|t| terminal.title(t).to_owned())
                        .unwrap_or_default(),
                });
            }
            self.resolver.relaunch(w, &home, terminals)
        });
        self.terminals.ask(ask);
        serde_json::to_string_pretty(&session)
            .inspect_err(|err| warn!("error serializing the session: {err:?}"))
            .ok()
    }

    /// Hands the session to the writer thread; a busy writer leaves it to the next tick.
    fn write(&mut self, json: String) {
        let Some(path) = &self.path else {
            return;
        };
        let writer = self.writer.get_or_insert_with(|| Writer::new(path.clone()));
        let Some(tx) = &writer.tx else {
            match write_private(path, &json) {
                Ok(()) => self.written = Some(json),
                Err(err) => warn!("error saving the session to {path:?}: {err:?}"),
            }
            return;
        };
        match tx.try_send((json.clone(), None)) {
            Ok(()) => self.written = Some(json),
            Err(TrySendError::Full(_)) => (),
            Err(TrySendError::Disconnected(_)) => {
                warn!("the session writer stopped; starting another");
                self.writer = None;
            }
        }
    }

    fn load(&self) -> Option<Session> {
        let path = self.path.as_ref()?;
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            Err(err) => {
                warn!("error reading the session {path:?}: {err:?}");
                return None;
            }
        };
        serde_json::from_str(&text)
            .inspect_err(|err| warn!("error parsing the session {path:?}: {err:?}"))
            .ok()
    }
}

/// `$XDG_STATE_HOME/newm-rs/session.json`, `~/.local/state` without it.
fn session_path() -> Option<PathBuf> {
    let dirs = directories::BaseDirs::new()?;
    let state = dirs
        .state_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dirs.home_dir().join(".local/state"));
    Some(state.join("newm-rs").join("session.json"))
}

fn home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Writes `text` to `path` through a temporary file and a rename, both mode 0600 in a directory
/// only the user can enter, so a reader never sees half a session.
pub fn write_private(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

/// Whether windows with `app_id` are never saved nor relaunched: prompts, agents and lockers, and
/// what the config's `restore-exclude` patterns match.
pub fn excluded(app_id: &str, patterns: &[RegexEq]) -> bool {
    let lower = app_id.to_lowercase();
    NEVER.iter().any(|never| lower.contains(never)) || patterns.iter().any(|p| p.0.is_match(app_id))
}

/// The window's app ID, title and process, when it is saved: a toplevel with an app ID, not a
/// dialog (those come back with their parent's app, or not at all), and not excluded, by its
/// app or by a client's `NewmRestoreExclude` request.
fn found(mapped: &Mapped, exclude: &[RegexEq]) -> Option<Found> {
    if mapped.toplevel().parent().is_some() || mapped.is_restore_excluded() {
        return None;
    }
    let app_id = mapped.app_id().filter(|id| !id.is_empty())?;
    if excluded(&app_id, exclude) {
        return None;
    }
    Some(Found {
        id: mapped.id().get(),
        app_id,
        title: mapped.title(),
        pid: mapped.credentials().map(|c| c.pid),
    })
}

/// The session the layout shows now.
pub(crate) fn snapshot(
    layout: &Layout<Mapped>,
    exclude: &[RegexEq],
    relaunch: impl FnMut(&Found) -> Option<Relaunch>,
) -> Session {
    let mut canvases = Vec::new();
    let mut windows: Vec<(usize, Place, bool, Found)> = Vec::new();
    for (mon, idx, ws) in layout.workspaces() {
        let space = ws.scrolling();
        let canvas = space.canvas();
        let k = canvases.len();
        let focused = space.active_window().map(|w| w.id());
        for (mapped, rect) in space.grid_windows() {
            if let Some(w) = found(mapped, exclude) {
                windows.push((k, Place::Tiled(rect), Some(mapped.id()) == focused, w));
            }
        }
        for (tile, pos, _) in ws.tiles_with_render_positions() {
            let mapped = tile.window();
            if mapped.is_floating()
                && let Some(w) = found(mapped, exclude)
            {
                let size = tile.tile_size();
                windows.push((
                    k,
                    Place::Floating((pos.x, pos.y), (size.w, size.h)),
                    false,
                    w,
                ));
            }
        }
        let scale = canvas.scale();
        let (x, y) = canvas.view();
        canvases.push(SavedCanvas {
            output: mon.map(|mon| mon.output_name().clone()),
            workspace: idx,
            zoom: canvas.zoom(),
            view: (x / scale, y / scale),
            focus: None,
            windows: Vec::new(),
            floating: Vec::new(),
        });
    }

    let found: Vec<Found> = windows.iter().map(|w| w.3.clone()).collect();
    let (launch_of, launches) = group(&found, relaunch);
    for ((k, place, focused, w), launch) in windows.into_iter().zip(launch_of) {
        let canvas = &mut canvases[k];
        match place {
            Place::Tiled(r) => {
                if focused {
                    canvas.focus = Some(canvas.windows.len());
                }
                canvas.windows.push(SavedWindow {
                    app_id: w.app_id,
                    title: w.title,
                    rect: (r.i, r.j, r.w, r.h),
                    launch,
                });
            }
            Place::Floating(pos, size) => canvas.floating.push(SavedFloating {
                app_id: w.app_id,
                title: w.title,
                pos,
                size,
                launch,
            }),
        }
    }
    canvases.retain(|c| !c.windows.is_empty() || !c.floating.is_empty());
    Session { canvases, launches }
}

/// Gives each window the index of the launch that brings it back: the windows of one process
/// share one launch (one per command: a browser's web apps have their own), a window relaunched
/// on its own has its own.
pub fn group(
    found: &[Found],
    mut relaunch: impl FnMut(&Found) -> Option<Relaunch>,
) -> (Vec<Option<usize>>, Vec<Launch>) {
    let mut launches = Vec::new();
    let mut by_process: HashMap<(i32, Launch), usize> = HashMap::new();
    let push = |launches: &mut Vec<Launch>, launch: Launch| {
        launches.push(launch);
        launches.len() - 1
    };
    let launch_of = found
        .iter()
        .map(|w| match relaunch(w)? {
            Relaunch::Window(launch) => Some(push(&mut launches, launch)),
            Relaunch::Process(launch) => Some(match w.pid {
                Some(pid) => *by_process
                    .entry((pid, launch.clone()))
                    .or_insert_with(|| push(&mut launches, launch)),
                None => push(&mut launches, launch),
            }),
        })
        .collect();
    (launch_of, launches)
}

/// Finds how windows are relaunched, caching what it reads.
#[derive(Debug, Default)]
struct Resolver {
    /// The desktop entry's launch per app ID.
    entries: HashMap<String, Option<Launch>>,
    /// The desktop files, read on the first app ID that is not a terminal's.
    apps: Option<apps::Index>,
    processes: HashMap<i32, Option<Launch>>,
}

impl Resolver {
    /// A terminal's window, on its own: a window of that terminal running what it showed (see
    /// [`terminals`]), else in the directory the title names, else home. Others: the desktop
    /// entry of the app the app ID names ([`apps::Index::path`]: the file named after it in any
    /// case, else one naming it as `StartupWMClass` or program), else the process's command line
    /// and directory.
    fn relaunch(&mut self, w: &Found, home: &Path, terminals: &Terminals) -> Option<Relaunch> {
        if let Some(terminal) = terminals::find(&w.app_id) {
            let instance = match (&terminal.single_instance, w.pid) {
                (Some(_), Some(pid)) => self
                    .process(pid)
                    .map(|l| terminal.instance(&l.command))
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            let title = w.title.as_deref().map(|t| terminal.title(t));
            if let Some(command) = terminals.command(w.id) {
                return Some(Relaunch::Window(terminal.running(home, &instance, command)));
            }
            let dir = title.and_then(|t| terminals.dir(t, home));
            return Some(Relaunch::Window(
                terminal.window(dir.as_deref().unwrap_or(home), &instance),
            ));
        }
        if let Some(launch) = self.desktop_entry(&w.app_id) {
            return Some(Relaunch::Process(launch));
        }
        self.process(w.pid?).map(Relaunch::Process)
    }

    fn process(&mut self, pid: i32) -> Option<Launch> {
        self.processes
            .entry(pid)
            .or_insert_with(|| read_process(pid))
            .clone()
    }

    fn desktop_entry(&mut self, app_id: &str) -> Option<Launch> {
        if let Some(launch) = self.entries.get(app_id) {
            return launch.clone();
        }
        let launch = self
            .apps
            .get_or_insert_with(|| apps::Index::read(&apps::data_dirs()))
            .path(app_id)
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| desktop_launch(&text));
        self.entries.insert(app_id.to_owned(), launch.clone());
        launch
    }
}

/// The directory a terminal's title names: the whole title, or its last word, as an absolute or
/// `~` path to an existing directory.
pub fn title_dir(title: &str, home: &Path) -> Option<PathBuf> {
    let last = title.rsplit([' ', ':']).next().unwrap_or(title);
    [title.trim(), last.trim()].into_iter().find_map(|t| {
        let path = if t == "~" {
            home.to_path_buf()
        } else if let Some(rest) = t.strip_prefix("~/") {
            home.join(rest)
        } else if t.starts_with('/') {
            PathBuf::from(t)
        } else {
            return None;
        };
        path.is_dir().then_some(path)
    })
}

/// A desktop file's `Exec` as arguments ([`exec::argv`]), in its `Path`.
pub fn desktop_launch(text: &str) -> Option<Launch> {
    let icon = apps::entry_key(text, "Icon").map(exec::unescape);
    let name = apps::entry_key(text, "Name").map(exec::unescape);
    let codes = exec::Fields {
        icon: icon.as_deref(),
        name: name.as_deref(),
        location: None,
    };
    let command = exec::argv(apps::entry_key(text, "Exec")?, &codes);
    (!command.is_empty()).then(|| Launch {
        command,
        cwd: apps::entry_key(text, "Path")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from),
    })
}

/// The process's command line and directory, unless it is a helper serving other programs'
/// windows.
fn read_process(pid: i32) -> Option<Launch> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let raw = raw.strip_suffix(&[0]).unwrap_or(&raw);
    let command: Vec<String> = raw
        .split(|&b| b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let argv0 = command.first().filter(|a| !a.is_empty())?;
    if HELPERS.contains(&basename(argv0)) || pid == std::process::id() as i32 {
        return None;
    }
    Some(Launch {
        command,
        cwd: fs::read_link(format!("/proc/{pid}/cwd")).ok(),
    })
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The program names the config starts at startup: launches of these are left to the config.
pub fn startup_programs(spawn: &[SpawnAtStartup], spawn_sh: &[SpawnShAtStartup]) -> Vec<String> {
    let mut names: Vec<String> = spawn
        .iter()
        .filter_map(|s| s.command.first())
        .map(|c| basename(c).to_owned())
        .collect();
    for s in spawn_sh {
        let words = s
            .command
            .split(|c: char| c.is_whitespace() || ";&|()'\"`".contains(c))
            .filter(|w| !w.is_empty() && !w.starts_with('-') && !w.contains('='));
        names.extend(words.map(|w| basename(w).to_owned()));
    }
    names
}

/// A saved canvas with its output's name and its workspace's index there.
pub type OnWorkspace = (Option<String>, usize, RestoreCanvas);

/// What a saved session puts back: each canvas's placeholders and camera, and the launches its
/// tiled windows need, less excluded apps and programs the config starts at startup. A launch only
/// floating windows need is left out: those are mostly prompts.
pub fn plan(
    session: Session,
    exclude: &[RegexEq],
    startup: &[String],
) -> (Vec<OnWorkspace>, Vec<Launch>) {
    let mut needed = BTreeSet::new();
    let mut canvases = Vec::new();
    for canvas in session.canvases {
        let mut windows = Vec::new();
        let mut focus = None;
        for (k, w) in canvas.windows.into_iter().enumerate() {
            if excluded(&w.app_id, exclude) || !saved_rect_ok(w.rect) {
                continue;
            }
            if canvas.focus == Some(k) {
                focus = Some(windows.len());
            }
            needed.extend(w.launch);
            let (i, j, width, height) = w.rect;
            windows.push(RestoreWindow {
                app_id: w.app_id,
                title: w.title,
                rect: Rect::new(i, j, width, height),
            });
        }
        if !windows.is_empty() {
            let saved = RestoreCanvas {
                windows,
                zoom: canvas.zoom,
                view: canvas.view,
                focus,
            };
            canvases.push((canvas.output, canvas.workspace, saved));
        }
    }
    let launches = needed
        .into_iter()
        .filter_map(|k| session.launches.get(k))
        .filter(|l| {
            l.command
                .first()
                .is_some_and(|argv0| !startup.iter().any(|s| s == basename(argv0)))
        })
        .cloned()
        .map(restoring)
        .collect();
    (canvases, launches)
}

/// Whether a saved window's tiles are ones newm-rs could have saved: a hand-edited or damaged
/// session file must not put a window where the canvas's tile math leaves i32.
fn saved_rect_ok((i, j, w, h): (i32, i32, i32, i32)) -> bool {
    const REACH: i32 = 1 << 20;
    const SPAN: i32 = 1024;
    (-REACH..=REACH).contains(&i)
        && (-REACH..=REACH).contains(&j)
        && (1..=SPAN).contains(&w)
        && (1..=SPAN).contains(&h)
}

/// Chromium and the browsers built on it. Started bare, each profile opens its own startup pages
/// (a new tab, a fixed page) instead of the windows it had.
const CHROMIUM_BROWSERS: &[&str] = &[
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "google-chrome-beta",
    "google-chrome-unstable",
    "brave",
    "brave-browser",
    "microsoft-edge",
    "microsoft-edge-stable",
    "vivaldi",
    "vivaldi-stable",
];

const RESTORE_LAST_SESSION: &str = "--restore-last-session";

/// `launch` as a restore runs it: a bare Chromium-family browser reopens its last session (every
/// profile that had windows, with their tabs) whatever each profile's startup setting says. A
/// launch that opens something itself (a URL, a file, an `--app` window) is left as it is: with
/// the browser already running, the flag would open the whole session a second time.
fn restoring(mut launch: Launch) -> Launch {
    let Some((argv0, args)) = launch.command.split_first() else {
        return launch;
    };
    let bare = !args
        .iter()
        .any(|a| !a.starts_with('-') || a.starts_with("--app") || a == RESTORE_LAST_SESSION);
    if bare && CHROMIUM_BROWSERS.contains(&basename(argv0)) {
        launch.command.push(String::from(RESTORE_LAST_SESSION));
    }
    launch
}

/// Runs a launch, by its program's name when its path is gone (an old Nix store path), in its
/// directory when that still exists, with the variables `env` set on top of the compositor's.
pub fn launch(launch: &Launch, env: Vec<(String, String)>) {
    let mut command = launch.command.clone();
    if let Some(argv0) = command.first_mut()
        && argv0.starts_with('/')
        && !Path::new(argv0).exists()
    {
        *argv0 = basename(argv0).to_owned();
    }
    let cwd = launch.cwd.clone().filter(|dir| dir.is_dir());
    spawn_with(command, None, cwd, env);
}

/// Puts the saved session back once an output is connected (unless the config turns restoring
/// off), then saves the windows as they change.
pub fn start(state: &mut State, startup: Vec<String>) {
    if !matches!(state.backend, Backend::Tty(_)) {
        info!("not restoring nor saving the session of a nested newm-rs");
        return;
    }
    let mut startup = Some(startup);
    let mut tries = 0;
    let timer = Timer::immediate();
    let res = state
        .niri
        .event_loop
        .insert_source(timer, move |_, _, state| {
            if startup.is_some() {
                if state.niri.layout.outputs().next().is_none() && tries < OUTPUT_TRIES {
                    tries += 1;
                    return TimeoutAction::ToDuration(OUTPUT_WAIT);
                }
                if let Some(startup) = startup.take() {
                    restore(state, &startup);
                }
            }
            let niri = &mut state.niri;
            niri.session.tick(&niri.layout, &niri.config.borrow());
            for (_, mapped) in niri.layout.windows() {
                mapped.set_remote_host(niri.session.remote_host(mapped.id().get()));
            }
            TimeoutAction::ToDuration(SAVE_INTERVAL)
        });
    if let Err(err) = res {
        warn!("error starting the session saver: {err:?}");
    }
}

fn restore(state: &mut State, startup: &[String]) {
    let niri = &mut state.niri;
    niri.session.ready = true;
    let config = niri.config.borrow();
    if !config.layout.canvas.restore_session {
        return;
    }
    let Some(session) = niri.session.load() else {
        return;
    };
    let (canvases, launches) = plan(session, &config.layout.canvas.restore_exclude, startup);
    drop(config);
    info!(
        "restoring the session: {} canvases, {} launches",
        canvases.len(),
        launches.len()
    );
    for (output, idx, saved) in canvases {
        niri.layout
            .canvas_restore(output.as_deref(), idx, saved, RESTORE_LIFETIME);
    }
    for l in &launches {
        launch(l, Vec::new());
    }
    niri.queue_redraw_all();
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::str::FromStr as _;

    use super::*;

    const GHOSTTY: &str = "com.mitchellh.ghostty";

    /// The Ghostty window launch from before the terminal table, word for word.
    fn ghostty_window(dir: &Path) -> Launch {
        Launch {
            command: vec![
                String::from("ghostty"),
                String::from("+new-window"),
                format!("--working-directory={}", dir.display()),
            ],
            cwd: None,
        }
    }

    /// `ghostty +new-window` from home running `command`, from before the terminal table.
    fn ghostty_running(home: &Path, command: Vec<String>) -> Launch {
        let mut launch = ghostty_window(home);
        launch.command.push(String::from("-e"));
        launch.command.extend(command);
        launch
    }

    fn found(app_id: &str, title: &str, pid: i32) -> Found {
        Found {
            id: 0,
            app_id: app_id.to_owned(),
            title: Some(title.to_owned()),
            pid: Some(pid),
        }
    }

    fn command(args: &[&str]) -> Launch {
        Launch {
            command: args.iter().map(|a| String::from(*a)).collect(),
            cwd: None,
        }
    }

    fn window(app_id: &str, rect: (i32, i32, i32, i32), launch: Option<usize>) -> SavedWindow {
        SavedWindow {
            app_id: app_id.to_owned(),
            title: None,
            rect,
            launch,
        }
    }

    fn session() -> Session {
        Session {
            canvases: vec![SavedCanvas {
                output: Some(String::from("DP-1")),
                workspace: 0,
                zoom: 1,
                view: (-120., 40.5),
                focus: Some(2),
                windows: vec![
                    window("chromium-browser", (0, 0, 2, 1), Some(0)),
                    window("org.example.Vault", (2, 0, 1, 1), Some(1)),
                    window("chromium-browser", (0, 1, 1, 1), Some(0)),
                    window("swaybg-like", (3, 0, 1, 1), Some(2)),
                ],
                floating: vec![SavedFloating {
                    app_id: String::from("org.kde.kcalc"),
                    title: Some(String::from("KCalc")),
                    pos: (10., 20.),
                    size: (300., 400.),
                    launch: Some(3),
                }],
            }],
            launches: vec![
                command(&["chromium"]),
                command(&["vault"]),
                command(&["/nix/store/x/bin/swaybg-like", "-m", "fill"]),
                command(&["kcalc"]),
            ],
        }
    }

    #[test]
    fn a_session_survives_a_round_trip_through_a_private_file() {
        let dir = std::env::temp_dir().join(format!("newm-rs-session-{}", std::process::id()));
        let path = dir.join("state/newm-rs/session.json");
        let session = session();
        let json = serde_json::to_string_pretty(&session).unwrap();
        write_private(&path, &json).unwrap();
        write_private(&path, &json).unwrap();
        let back: Session = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back, session);
        assert_eq!(file_mode, 0o600, "titles and command lines are private");
        assert_eq!(dir_mode, 0o700);
        assert_eq!(
            leftovers, 1,
            "the temporary file was renamed over the session"
        );
    }

    #[test]
    fn a_restore_skips_windows_no_save_could_have_written() {
        let (base, _) = plan(session(), &[], &[]);
        let good = base[0].2.windows.len();
        let mut session = session();
        for rect in [
            (i32::MAX, 0, 1, 1),
            (0, i32::MIN, 1, 1),
            (0, 0, 0, 1),
            (0, 0, 1, i32::MAX),
        ] {
            session.canvases[0].windows.push(window("bad", rect, None));
        }
        let (canvases, _) = plan(session, &[], &[]);
        assert_eq!(canvases[0].2.windows.len(), good);
        assert!(canvases[0].2.windows.iter().all(|w| w.app_id != "bad"));
    }

    #[test]
    fn the_session_writer_lands_writes_in_order_and_a_quit_waits_for_it() {
        let dir = std::env::temp_dir().join(format!("newm-rs-writer-{}", std::process::id()));
        let path = dir.join("session.json");
        let mut saver = Saver {
            path: Some(path.clone()),
            ..Saver::default()
        };
        for k in 0..50 {
            saver.write(format!("queued {k}"));
        }
        assert!(saver.writer.as_ref().is_some_and(|w| w.tx.is_some()));
        saver.write_waiting(String::from("at quit"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "at quit");
        assert_eq!(saver.written.as_deref(), Some("at quit"));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn windows_of_one_process_share_a_launch() {
        let found = [
            found("chromium-browser", "a", 10),
            found("org.kde.dolphin", "x", 20),
            found("chromium-browser", "b", 10),
            found("org.kde.dolphin", "y", 21),
            found(GHOSTTY, "~", 30),
            found(GHOSTTY, "/tmp", 30),
            found("org.kde.dolphin", "z", 20),
            found("helper-served", "w", 40),
            found("chrome-app-Default", "web app", 10),
        ];
        let (launch_of, launches) = group(&found, |w| match w.app_id.as_str() {
            GHOSTTY => Some(Relaunch::Window(command(&["ghostty", w.title.as_deref()?]))),
            "helper-served" => None,
            app_id => Some(Relaunch::Process(command(&[app_id]))),
        });
        assert_eq!(
            launch_of,
            [
                Some(0),
                Some(1),
                Some(0),
                Some(2),
                Some(3),
                Some(4),
                Some(1),
                None,
                Some(5),
            ]
        );
        assert_eq!(
            launches,
            [
                command(&["chromium-browser"]),
                command(&["org.kde.dolphin"]),
                command(&["org.kde.dolphin"]),
                command(&["ghostty", "~"]),
                command(&["ghostty", "/tmp"]),
                command(&["chrome-app-Default"]),
            ],
            "one launch per process and command, one per Ghostty window"
        );
    }

    #[test]
    fn terminals_with_one_title_save_their_own_tmux_sessions() {
        let home = Path::new("/h");
        let attach = |n: u64| terminals::tmux_command(&[], &format!("work{n}"));
        let terminals = Terminals::with((1..=3).map(|n| (n, attach(n))).collect());
        let found: Vec<Found> = [100, 200, 300]
            .into_iter()
            .zip(1..)
            .map(|(pid, id)| Found {
                id,
                ..found("foot", "desk: me@desk: ~", pid)
            })
            .collect();
        let mut resolver = Resolver::default();
        let (launch_of, launches) = group(&found, |w| resolver.relaunch(w, home, &terminals));
        assert_eq!(launch_of, [Some(0), Some(1), Some(2)]);
        let running = |n| {
            let mut launch = command(&["foot", "--working-directory=/h"]);
            launch.command.extend(attach(n));
            launch
        };
        assert_eq!(launches, [running(1), running(2), running(3)]);
    }

    #[test]
    fn ghostty_windows_reopen_in_the_directory_their_title_names() {
        let home = std::env::temp_dir();
        let mut resolver = Resolver::default();
        let terminals = Terminals::with(HashMap::new());
        let relaunch = |resolver: &mut Resolver, title: &str| {
            resolver.relaunch(&found(GHOSTTY, title, 1), &home, &terminals)
        };
        assert_eq!(
            relaunch(&mut resolver, "me@desk: /"),
            Some(Relaunch::Window(ghostty_window(Path::new("/"))))
        );
        assert_eq!(
            relaunch(&mut resolver, "~"),
            Some(Relaunch::Window(ghostty_window(&home)))
        );
        assert_eq!(
            relaunch(&mut resolver, "less notes.md"),
            Some(Relaunch::Window(ghostty_window(&home))),
            "no directory in the title: home"
        );
        assert_eq!(title_dir("/no/such/dir", &home), None);
    }

    #[test]
    fn ghostty_windows_rerun_the_tmux_session_or_ssh_they_showed() {
        let home = std::env::temp_dir();
        let attach = terminals::tmux_command(&[], "5");
        let terminals = Terminals::with(HashMap::from([(3, attach.clone())]));
        let mut resolver = Resolver::default();
        let shown = Found {
            id: 3,
            ..found(GHOSTTY, "desk: ✳ notes", 1)
        };
        let launch = resolver.relaunch(&shown, &home, &terminals);
        assert_eq!(
            launch,
            Some(Relaunch::Window(ghostty_running(&home, attach)))
        );
        let Some(Relaunch::Window(launch)) = launch else {
            unreachable!()
        };
        assert_eq!(
            launch.command[..4],
            [
                String::from("ghostty"),
                String::from("+new-window"),
                format!("--working-directory={}", home.display()),
                String::from("-e"),
            ]
        );
        assert_eq!(
            resolver.relaunch(&found(GHOSTTY, "~", 1), &home, &terminals),
            Some(Relaunch::Window(ghostty_window(&home))),
            "a window no terminal was found for: a shell as before"
        );
    }

    #[test]
    fn other_terminals_rerun_what_they_showed_with_their_own_flags() {
        let home = std::env::temp_dir();
        let attach = terminals::tmux_command(&[], "5");
        let terminals = Terminals::with(HashMap::from([(3, attach.clone())]));
        let mut resolver = Resolver::default();
        resolver.processes.insert(1, None);
        // Window 3 shows tmux session 5; the others show a shell.
        let mut relaunch = |app_id: &str, title: &str| {
            let id = if title.starts_with("desk: ✳ notes") {
                3
            } else {
                0
            };
            resolver.relaunch(
                &Found {
                    id,
                    ..found(app_id, title, 1)
                },
                &home,
                &terminals,
            )
        };
        let words = |w: &[&str]| -> Vec<String> { w.iter().map(|&a| a.to_owned()).collect() };
        let running = |w: &[&str]| Launch {
            command: [words(w), attach.clone()].concat(),
            cwd: None,
        };
        let h = home.display().to_string();
        assert_eq!(
            relaunch("Alacritty", "desk: ✳ notes"),
            Some(Relaunch::Window(running(&[
                "alacritty",
                "--working-directory",
                &h,
                "-e"
            ])))
        );
        assert_eq!(
            relaunch("org.kde.konsole", "desk: ✳ notes \u{2014} Konsole"),
            Some(Relaunch::Window(running(&[
                "konsole",
                "--workdir",
                &h,
                "-e"
            ]))),
            "the window found, whatever Konsole appends to its title"
        );
        assert_eq!(
            relaunch("org.gnome.Terminal", "desk: ✳ notes"),
            Some(Relaunch::Window(running(&[
                "gnome-terminal",
                &format!("--working-directory={h}"),
                "--"
            ])))
        );
        assert_eq!(
            relaunch("XTerm", "desk: ✳ notes"),
            Some(Relaunch::Window(Launch {
                command: [words(&["xterm", "-e"]), attach.clone()].concat(),
                cwd: Some(home.clone()),
            }))
        );
        assert_eq!(
            relaunch("foot", "me@desk: /"),
            Some(Relaunch::Window(command(&[
                "foot",
                "--working-directory=/"
            ]))),
            "a plain shell: in the directory its title names"
        );
        assert_eq!(
            relaunch("org.kde.konsole", "/ \u{2014} Konsole"),
            Some(Relaunch::Window(command(&["konsole", "--workdir", "/"])))
        );
        assert_eq!(
            relaunch("kitty", "vim"),
            Some(Relaunch::Window(command(&[
                "kitty",
                &format!("--directory={h}")
            ]))),
            "no directory in the title: home"
        );
    }

    #[test]
    fn kitty_windows_go_back_to_the_single_instance_they_ran_in() {
        let home = Path::new("/h");
        let terminals = Terminals::with(HashMap::new());
        let mut resolver = Resolver::default();
        resolver.processes.insert(
            7,
            Some(command(&[
                "/nix/store/x/bin/kitty",
                "-1",
                "--instance-group",
                "w",
            ])),
        );
        resolver.processes.insert(8, Some(command(&["kitty"])));
        assert_eq!(
            resolver.relaunch(&found("kitty", "~", 7), home, &terminals),
            Some(Relaunch::Window(command(&[
                "kitty",
                "--single-instance",
                "--instance-group=w",
                "--directory=/h"
            ])))
        );
        assert_eq!(
            resolver.relaunch(&found("kitty", "~", 8), home, &terminals),
            Some(Relaunch::Window(command(&["kitty", "--directory=/h"])))
        );
    }

    #[test]
    fn an_unknown_terminal_relaunches_as_before() {
        let home = std::env::temp_dir();
        let terminals = Terminals::with(HashMap::from([(9, terminals::tmux_command(&[], "5"))]));
        let mut resolver = Resolver::default();
        let process = command(&["/nix/store/x/bin/unknownterm", "--hold"]);
        resolver.processes.insert(9, Some(process.clone()));
        assert_eq!(terminals::find("org.example.UnknownTerm"), None);
        assert_eq!(
            resolver.relaunch(
                &Found {
                    id: 9,
                    ..found("org.example.UnknownTerm", "desk: ✳ notes", 9)
                },
                &home,
                &terminals
            ),
            Some(Relaunch::Process(process)),
            "its process's command line, shared by its windows, not a tmux attach"
        );
    }

    #[test]
    fn a_ghostty_session_saved_before_the_terminal_table_restores_the_same() {
        let json = r#"{
  "canvases": [
    {
      "output": "DP-1",
      "workspace": 0,
      "zoom": 1,
      "view": [0.0, 0.0],
      "focus": 0,
      "windows": [
        {"app_id": "com.mitchellh.ghostty", "title": "desk: ✳ newm",
         "rect": [0, 0, 1, 1], "launch": 0},
        {"app_id": "com.mitchellh.ghostty", "title": "far: ~",
         "rect": [1, 0, 1, 1], "launch": 1},
        {"app_id": "com.mitchellh.ghostty", "title": "~",
         "rect": [2, 0, 1, 1], "launch": 2}
      ],
      "floating": []
    }
  ],
  "launches": [
    {"command": ["ghostty", "+new-window", "--working-directory=/home/u", "-e", "sh", "-c",
      "tmux new-session -A -s 0; exec \"${SHELL:-sh}\""], "cwd": null},
    {"command": ["ghostty", "+new-window", "--working-directory=/home/u", "-e", "sh", "-c",
      "ssh -t far tmux new-session -A -s 3; exec \"${SHELL:-sh}\""], "cwd": null},
    {"command": ["ghostty", "+new-window", "--working-directory=/home/u"], "cwd": null}
  ]
}"#;
        let session: Session = serde_json::from_str(json).unwrap();
        let saved = session.launches.clone();
        let (canvases, launches) = plan(session, &[], &[]);
        assert_eq!(launches, saved, "every launch runs as it was saved");
        let windows = &canvases[0].2.windows;
        assert_eq!(windows.len(), 3);
        assert!(windows.iter().all(|w| w.app_id == GHOSTTY));
        let home = Path::new("/home/u");
        assert_eq!(saved[2], ghostty_window(home));
        assert_eq!(
            saved[0],
            ghostty_running(home, terminals::tmux_command(&[], "0"))
        );
        let resolver_home = std::env::temp_dir();
        let mut resolver = Resolver::default();
        assert_eq!(
            resolver.relaunch(
                &found(GHOSTTY, "~", 1),
                &resolver_home,
                &Terminals::with(HashMap::new())
            ),
            Some(Relaunch::Window(ghostty_window(&resolver_home))),
            "and the saver writes the same launch again"
        );
    }

    #[test]
    fn desktop_entries_launch_their_exec_without_field_codes() {
        let entry = "[Desktop Entry]\nName=Chromium\nExec=chromium %U\nPath = /tmp\n\n\
                     [Desktop Action new-window]\nExec=chromium --new-window\n";
        assert_eq!(
            desktop_launch(entry),
            Some(Launch {
                command: vec![String::from("chromium")],
                cwd: Some(PathBuf::from("/tmp")),
            })
        );
        let entry = "[Desktop Entry]\nName=Term\nIcon=term\nExec=term %i --class=%c %F\n";
        assert_eq!(
            desktop_launch(entry).unwrap().command,
            ["term", "--icon", "term", "--class=Term"]
        );
        assert_eq!(desktop_launch("[Desktop Entry]\nExec=%F\n"), None);
        assert_eq!(desktop_launch("[Other]\nExec=app\n"), None);
    }

    #[test]
    fn prompts_lockers_and_configured_apps_are_excluded() {
        let patterns = [RegexEq::from_str("^mpv$").unwrap()];
        for app_id in [
            "org.kde.polkit-kde-authentication-agent-1",
            "pinentry-qt",
            "org.freedesktop.impl.portal.desktop.kde.xdg-desktop-portal-kde",
            "org.freedesktop.impl.portal.desktop.kde",
            "org.kde.ksshaskpass",
            "org.kde.kwalletd6",
            "mpv",
        ] {
            assert!(excluded(app_id, &patterns), "{app_id}");
        }
        for app_id in ["mpvpaper-ish", "org.gnome.clocks", "chromium-browser"] {
            assert!(!excluded(app_id, &patterns), "{app_id}");
        }
    }

    #[test]
    fn startup_programs_are_named_by_their_basename() {
        let spawn = [SpawnAtStartup {
            command: vec![
                String::from("/nix/store/x/bin/notifier"),
                String::from("--config"),
            ],
        }];
        let spawn_sh = [SpawnShAtStartup {
            command: String::from("swayidle -w timeout 300 'newm-rs-lock' && FOO=1 panel"),
        }];
        let names = startup_programs(&spawn, &spawn_sh);
        for name in ["notifier", "swayidle", "newm-rs-lock", "panel"] {
            assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
        }
        assert!(!names.iter().any(|n| n == "-w" || n == "FOO=1"));
    }

    #[test]
    fn a_restore_plans_tiled_windows_and_their_launches_once() {
        let patterns = [RegexEq::from_str("Vault").unwrap()];
        let startup = [String::from("swaybg-like")];
        let (canvases, launches) = plan(session(), &patterns, &startup);
        assert_eq!(canvases.len(), 1);
        let (output, idx, saved) = &canvases[0];
        assert_eq!((output.as_deref(), *idx), (Some("DP-1"), 0));
        let rects: Vec<(&str, Rect)> = saved
            .windows
            .iter()
            .map(|w| (w.app_id.as_str(), w.rect))
            .collect();
        assert_eq!(
            rects,
            [
                ("chromium-browser", Rect::new(0, 0, 2, 1)),
                ("chromium-browser", Rect::new(0, 1, 1, 1)),
                ("swaybg-like", Rect::new(3, 0, 1, 1)),
            ],
            "excluded apps get no placeholder; the config's own startup programs keep theirs"
        );
        assert_eq!(saved.focus, Some(1), "the focus follows its window");
        assert_eq!((saved.zoom, saved.view), (1, (-120., 40.5)));
        assert_eq!(
            launches,
            [command(&["chromium", "--restore-last-session"])],
            "Chromium once for its two windows, restoring its last session; no excluded, \
             startup or floating-only launch"
        );
    }

    #[test]
    fn chromium_browsers_restore_their_last_session_unless_they_open_something() {
        let restored = |args: &[&str]| restoring(command(args)).command;
        let words = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let chrome = [
            "/nix/store/x/bin/google-chrome-stable",
            "--ozone-platform=x11",
        ];
        assert_eq!(
            restored(&chrome),
            words(&[chrome[0], chrome[1], "--restore-last-session"])
        );
        let twice = ["chromium", "--restore-last-session"];
        assert_eq!(restored(&twice), words(&twice), "never twice");
        for kept in [
            &["chromium", "https://example.org"][..],
            &["chromium", "--profile-directory=Default", "--app-id=abc"],
            &["chromium", "--app=https://example.org"],
            &["firefox"],
            &["kcalc"],
        ] {
            assert_eq!(restored(kept), words(kept), "{kept:?}");
        }
    }
}
