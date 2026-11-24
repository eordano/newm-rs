//! What each terminal window shows, so a restore brings it back: the local tmux session it is
//! attached to, or the ssh command it runs and the remote tmux session there. [`TERMINALS`] lists
//! the terminals this knows, how their windows are told apart and how each opens a window again.
//!
//! A window whose terminal runs one process per window is tied to the shell in it by that
//! process: the tmux client or ssh under it is the window's. A terminal can also serve every
//! window from one process (Ghostty, gnome-terminal), and then only titles tell its windows apart.
//! tmux sets the window title (`set-titles`), so `tmux list-clients` with
//! `#{T:set-titles-string}` names the session behind each title; over ssh the remote tmux sets it,
//! and the remote `tmux list-clients` is asked through an ssh ControlMaster that is already open
//! (never a new connection: [`QUERY_GUARD`] makes one fail before it authenticates, so it never
//! asks for a key). An ssh window whose remote has no tmux, or no open master, is matched by its
//! title's host when only one fits. [`resolve_in`] states how windows with one title are told
//! apart.
//!
//! The work runs on its own thread: the saver asks with the windows it found and reads the
//! answer at its next tick. The directories titles name are looked up there too: a title can name
//! a path on a network mount, and a stat of a hung NFS mount would freeze the compositor.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// Options that keep a query from opening a connection: with no master to multiplex over, ssh
/// connects through `false` and fails before it authenticates.
const QUERY_GUARD: [&str; 5] = ["-o", "BatchMode=yes", "-o", "ProxyCommand=false", "-T"];

/// How long a tmux or ssh query may take.
const TIMEOUT: Duration = Duration::from_secs(3);

/// How long a remote host's answer is reused while every title it is asked for is in it.
const REMOTE_FRESH: Duration = Duration::from_secs(30);

/// The tmux clients' format: client PID, session and the title tmux gives the terminal.
const CLIENTS: &str = "#{client_pid}\t#{session_name}\t#{T:set-titles-string}";

/// The remote tmux clients' format, quoted for the remote shell.
const REMOTE_CLIENTS: &str = "tmux list-clients -F '#{session_name}\t#{T:set-titles-string}'";

/// OpenSSH options that take an argument.
const SSH_ARG_OPTS: &str = "BbcDEeFIiJLlmOoPpQRSWw";

/// How a terminal opens a window in a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// One argument: `flag=DIR`.
    Joined(&'static str),
    /// Two arguments: `flag DIR`.
    Split(&'static str),
    /// No flag: the launch starts in the directory.
    Cwd,
}

/// A terminal that runs one process for every window only when started with a flag.
#[derive(Debug, PartialEq, Eq)]
pub struct SingleInstance {
    /// The flags that turn it on; a relaunch passes the first.
    pub flags: &'static [&'static str],
    /// The option naming the shared instance, kept from the saved process.
    pub group: Option<&'static str>,
}

/// A terminal whose windows a restore reopens on their own, running what they showed (see the
/// module docs), else in the directory the title names. The flags are the ones the `--help` of
/// the nixpkgs version gives.
#[derive(Debug, PartialEq, Eq)]
pub struct Terminal {
    /// The Wayland app IDs and X11 classes of its windows, matched in any case.
    pub app_ids: &'static [&'static str],
    /// The names of the process whose children are its windows' shells.
    pub exes: &'static [&'static str],
    /// The command that opens a window.
    pub program: &'static [&'static str],
    pub dir: Dir,
    /// What comes between the window's options and the command it runs.
    pub exec: &'static [&'static str],
    /// How the relaunch keeps one shared process, when the terminal has a flag for it. Ghostty
    /// (`+new-window`), gnome-terminal, xfce4-terminal, wezterm and footclient hand every new
    /// window to the running instance on their own, Konsole when its single-process setting is on.
    pub single_instance: Option<SingleInstance>,
    /// What the terminal appends to the title the shell sets: Qt adds the app's name.
    pub title_suffix: Option<&'static str>,
}

/// Ghostty runs one resident process for all its windows: `ghostty +new-window` opens another.
pub const GHOSTTY: Terminal = Terminal {
    app_ids: &["com.mitchellh.ghostty"],
    exes: &["ghostty"],
    program: &["ghostty", "+new-window"],
    dir: Dir::Joined("--working-directory"),
    exec: &["-e"],
    single_instance: None,
    title_suffix: None,
};

/// The terminals a restore knows.
pub const TERMINALS: &[Terminal] = &[
    GHOSTTY,
    // A process per window.
    Terminal {
        app_ids: &["Alacritty"],
        exes: &["alacritty"],
        program: &["alacritty"],
        dir: Dir::Split("--working-directory"),
        exec: &["-e"],
        single_instance: None,
        title_suffix: None,
    },
    Terminal {
        app_ids: &["kitty"],
        exes: &["kitty"],
        program: &["kitty"],
        dir: Dir::Joined("--directory"),
        exec: &[],
        single_instance: Some(SingleInstance {
            flags: &["--single-instance", "-1"],
            group: Some("--instance-group"),
        }),
        title_suffix: None,
    },
    // A process per window.
    Terminal {
        app_ids: &["foot"],
        exes: &["foot"],
        program: &["foot"],
        dir: Dir::Joined("--working-directory"),
        exec: &[],
        single_instance: None,
        title_suffix: None,
    },
    // The windows of `foot --server`, whose shells are the server's children.
    Terminal {
        app_ids: &["footclient"],
        exes: &["foot"],
        program: &["footclient"],
        dir: Dir::Joined("--working-directory"),
        exec: &[],
        single_instance: None,
        title_suffix: None,
    },
    Terminal {
        app_ids: &["org.kde.konsole", "konsole"],
        exes: &["konsole"],
        program: &["konsole"],
        dir: Dir::Split("--workdir"),
        exec: &["-e"],
        single_instance: None,
        title_suffix: Some(" \u{2014} Konsole"),
    },
    Terminal {
        app_ids: &["org.wezfurlong.wezterm"],
        exes: &["wezterm-gui", "wezterm"],
        program: &["wezterm", "start"],
        dir: Dir::Split("--cwd"),
        exec: &["--"],
        single_instance: None,
        title_suffix: None,
    },
    // One gnome-terminal-server holds every window.
    Terminal {
        app_ids: &["org.gnome.Terminal", "gnome-terminal"],
        exes: &["gnome-terminal-server"],
        program: &["gnome-terminal"],
        dir: Dir::Joined("--working-directory"),
        exec: &["--"],
        single_instance: None,
        title_suffix: None,
    },
    Terminal {
        app_ids: &["xfce4-terminal", "org.xfce.terminal"],
        exes: &["xfce4-terminal"],
        program: &["xfce4-terminal"],
        dir: Dir::Joined("--working-directory"),
        exec: &["-x"],
        single_instance: None,
        title_suffix: None,
    },
    // X11 only: its windows come from xwayland-satellite's process, so its own are found by name.
    Terminal {
        app_ids: &["XTerm"],
        exes: &["xterm"],
        program: &["xterm"],
        dir: Dir::Cwd,
        exec: &["-e"],
        single_instance: None,
        title_suffix: None,
    },
];

/// The terminal whose windows have the app ID (or X11 class) `app_id`.
pub fn find(app_id: &str) -> Option<&'static Terminal> {
    TERMINALS
        .iter()
        .find(|t| t.app_ids.iter().any(|id| id.eq_ignore_ascii_case(app_id)))
}

/// A program's name from its `argv[0]`, without the directory or Nix's `.NAME-wrapped`.
pub fn exe_name(argv0: &str) -> &str {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    base.strip_prefix('.')
        .and_then(|b| b.strip_suffix("-wrapped"))
        .unwrap_or(base)
}

impl Terminal {
    /// The title the shell set, without what the terminal appends.
    pub fn title<'a>(&self, title: &'a str) -> &'a str {
        self.title_suffix
            .and_then(|suffix| title.strip_suffix(suffix))
            .unwrap_or(title)
    }

    /// Whether `argv0` runs this terminal's process.
    pub fn is_exe(&self, argv0: &str) -> bool {
        self.exes.contains(&exe_name(argv0))
    }

    /// The arguments that put a relaunched window in the shared instance the saved process
    /// `argv` ran with, if any.
    pub fn instance(&self, argv: &[String]) -> Vec<String> {
        let Some(single) = &self.single_instance else {
            return Vec::new();
        };
        if !argv
            .iter()
            .skip(1)
            .any(|a| single.flags.contains(&a.as_str()))
        {
            return Vec::new();
        }
        let mut out = vec![single.flags[0].to_owned()];
        if let Some(group) = single.group {
            let joined = format!("{group}=");
            let mut args = argv.iter().skip(1);
            while let Some(a) = args.next() {
                let name = if a == group {
                    args.next().cloned()
                } else {
                    a.strip_prefix(&joined).map(str::to_owned)
                };
                if let Some(name) = name {
                    out.push(format!("{group}={name}"));
                    break;
                }
            }
        }
        out
    }

    /// A window in `dir`, in the shared instance `instance` names (see [`Self::instance`]).
    pub fn window(&self, dir: &Path, instance: &[String]) -> super::Launch {
        let mut command: Vec<String> = self.program.iter().map(|&a| a.to_owned()).collect();
        command.extend(instance.iter().cloned());
        let mut cwd = None;
        match self.dir {
            Dir::Joined(flag) => command.push(format!("{flag}={}", dir.display())),
            Dir::Split(flag) => command.extend([flag.to_owned(), dir.display().to_string()]),
            Dir::Cwd => cwd = Some(dir.to_path_buf()),
        }
        super::Launch { command, cwd }
    }

    /// A window from home running `command`.
    pub fn running(&self, home: &Path, instance: &[String], command: Vec<String>) -> super::Launch {
        let mut launch = self.window(home, instance);
        launch
            .command
            .extend(self.exec.iter().map(|&a| a.to_owned()));
        launch.command.extend(command);
        launch
    }
}

/// A terminal window the saver asks about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// The window's ID in the compositor, which keys the answer.
    pub id: u64,
    /// The window's process.
    pub pid: Option<i32>,
    pub terminal: &'static Terminal,
    /// Its title without what the terminal appends, empty when it has none.
    pub title: String,
}

/// What the saver asks: the terminal windows it found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ask {
    pub windows: Vec<Window>,
}

impl Ask {
    /// The windows' titles, each once.
    fn titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = Vec::new();
        for w in &self.windows {
            if !w.title.is_empty() && !titles.contains(&w.title) {
                titles.push(w.title.clone());
            }
        }
        titles
    }
}

/// Finds what terminal windows show, on a thread of its own (see the module docs).
#[derive(Debug)]
pub struct Terminals {
    tx: Option<SyncSender<Ask>>,
    /// The command that brings back each window, by window ID.
    found: Arc<Mutex<HashMap<u64, Vec<String>>>>,
    /// The existing directory each asked title names (see [`super::title_dir`]).
    dirs: Arc<Mutex<HashMap<String, PathBuf>>>,
    /// The last ask sent and when: the same windows are asked again only after [`REASK`].
    last: Option<(Ask, Instant)>,
}

/// How long an unchanged set of windows waits before it is asked again: a master may have
/// opened, or a session changed, under the same titles.
const REASK: Duration = Duration::from_secs(60);

impl Default for Terminals {
    fn default() -> Self {
        let found = Arc::new(Mutex::new(HashMap::new()));
        let dirs = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = sync_channel(1);
        let shared = (found.clone(), dirs.clone());
        let tx = thread::Builder::new()
            .name(String::from("newm-rs-terminals"))
            .spawn(move || work(rx, shared.0, shared.1))
            .inspect_err(|err| warn!("error starting the terminal finder: {err:?}"))
            .ok()
            .map(|_| tx);
        Self {
            tx,
            found,
            dirs,
            last: None,
        }
    }
}

impl Terminals {
    /// A finder that only answers from `found`, for tests.
    #[cfg(test)]
    pub fn with(found: HashMap<u64, Vec<String>>) -> Self {
        Self {
            tx: None,
            found: Arc::new(Mutex::new(found)),
            dirs: Arc::new(Mutex::new(HashMap::new())),
            last: None,
        }
    }

    /// Asks what the windows show, unless the same was asked within [`REASK`]; a busy finder
    /// skips it.
    pub fn ask(&mut self, ask: Ask) {
        let same = self
            .last
            .as_ref()
            .is_some_and(|(last, at)| *last == ask && at.elapsed() < REASK);
        if ask.windows.is_empty() || same {
            return;
        }
        if let Some(tx) = &self.tx
            && tx.try_send(ask.clone()).is_ok()
        {
            self.last = Some((ask, Instant::now()));
        }
    }

    /// The command that brings back the window `id`, from the last answer.
    pub fn command(&self, id: u64) -> Option<Vec<String>> {
        self.found.lock().ok()?.get(&id).cloned()
    }

    /// The existing directory the title names, from the last answer; a finder without its thread
    /// (tests) looks it up itself.
    pub fn dir(&self, title: &str, home: &Path) -> Option<PathBuf> {
        if self.tx.is_none() {
            return super::title_dir(title, home);
        }
        self.dirs.lock().ok()?.get(title).cloned()
    }
}

fn work(
    rx: Receiver<Ask>,
    found: Arc<Mutex<HashMap<u64, Vec<String>>>>,
    dirs: Arc<Mutex<HashMap<String, PathBuf>>>,
) {
    let mut remote = HashMap::new();
    while let Ok(ask) = rx.recv() {
        let answer = resolve(&ask, &mut remote);
        if let Ok(mut found) = found.lock() {
            *found = answer;
        }
        let home = super::home();
        let named: HashMap<String, PathBuf> = ask
            .titles()
            .into_iter()
            .filter_map(|t| {
                let dir = super::title_dir(&t, &home)?;
                Some((t, dir))
            })
            .collect();
        if let Ok(mut dirs) = dirs.lock() {
            *dirs = named;
        }
    }
}

/// An ssh command line split into its options, destination and remote command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshArgs {
    pub options: Vec<String>,
    pub dest: String,
    pub command: Vec<String>,
}

impl SshArgs {
    /// Parses the arguments after `ssh`. Like OpenSSH, options may follow the destination; the
    /// first other word after it starts the remote command.
    pub fn parse(args: &[String]) -> Option<Self> {
        let mut options = Vec::new();
        let mut dest = None;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                if dest.is_none() {
                    dest = args.get(i + 1).cloned();
                    i += 1;
                }
                i += 1;
                break;
            }
            match a.strip_prefix('-').filter(|f| !f.is_empty()) {
                Some(flags) => {
                    options.push(a.clone());
                    if let Some((j, c)) = flags
                        .char_indices()
                        .find(|&(_, c)| SSH_ARG_OPTS.contains(c))
                        && j + c.len_utf8() == flags.len()
                    {
                        i += 1;
                        options.push(args.get(i)?.clone());
                    }
                }
                None if dest.is_none() => dest = Some(a.clone()),
                None => break,
            }
            i += 1;
        }
        Some(Self {
            options,
            dest: dest?,
            command: args.get(i..).unwrap_or_default().to_vec(),
        })
    }

    /// The options that pick the connection (user, port, config, identity, jump host, `-o`), and
    /// the destination: what a query or a master needs, without forwards or a terminal.
    pub fn connection(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut opts = self.options.iter();
        while let Some(o) = opts.next() {
            let flags = &o[1..];
            let keep = flags.len() == 1 && "lpFiJo46C".contains(flags);
            let takes = flags.len() == 1 && SSH_ARG_OPTS.contains(flags);
            let attached = flags.len() > 1 && "lpFiJo".contains(&flags[..1]);
            if keep {
                out.push(o.clone());
                if takes && let Some(v) = opts.next() {
                    out.push(v.clone());
                }
            } else if attached {
                out.push(o.clone());
            } else if takes {
                opts.next();
            }
        }
        out.push(self.dest.clone());
        out
    }
}

/// Quotes `s` for a POSIX shell.
pub fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c))
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn sh_words(words: &[String]) -> String {
    words
        .iter()
        .map(|w| sh_quote(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs `script` in `sh`, then the user's shell, so the window stays when it ends.
fn then_shell(script: String) -> Vec<String> {
    vec![
        String::from("sh"),
        String::from("-c"),
        format!("{script}; exec \"${{SHELL:-sh}}\""),
    ]
}

/// Attaches to the local tmux session `session` on the server `socket` names (`-L`/`-S`),
/// creating it when it is gone.
pub fn tmux_command(socket: &[String], session: &str) -> Vec<String> {
    let mut words = vec![String::from("tmux")];
    words.extend(socket.iter().cloned());
    words.extend(["new-session", "-A", "-s", session].map(String::from));
    then_shell(sh_words(&words))
}

/// Runs the saved ssh command again with a terminal, attached to the remote tmux session
/// `session` when it was in one and the saved command ran nothing else. With `multiplexed`, a
/// lock lets one window open the master first, so windows to one host ask for one key touch.
pub fn ssh_command(
    argv: &[String],
    args: &SshArgs,
    session: Option<&str>,
    multiplexed: bool,
) -> Vec<String> {
    let mut script = String::new();
    if multiplexed {
        let conn = sh_words(&args.connection());
        let mut hasher = DefaultHasher::new();
        conn.hash(&mut hasher);
        script.push_str(&format!(
            "exec 9>\"${{XDG_RUNTIME_DIR:-/tmp}}/newm-rs-ssh-{:016x}.lock\"; flock 9; \
             ssh -O check {conn} >/dev/null 2>&1 || ssh -fN {conn}; flock -u 9; exec 9>&-; ",
            hasher.finish()
        ));
    }
    let safe = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
    };
    // By name: the saved path can be a Nix store path that is gone by the next login.
    let mut words = vec![String::from("ssh")];
    let interactive = args.command.is_empty() || attaches_tmux(&args.command);
    match session.filter(|s| interactive && safe(s)) {
        Some(session) => {
            // A restored window runs `ssh -t ... tmux new-session -A -s S`: its command and
            // terminal flag are dropped, not doubled.
            let opts = &argv[1..argv.len().saturating_sub(args.command.len()).max(1)];
            if !opts.iter().any(|o| o == "-t" || o == "-tt") {
                words.push(String::from("-t"));
            }
            words.extend(opts.iter().cloned());
            script.push_str(&sh_words(&words));
            script.push_str(&format!(" tmux new-session -A -s {session}"));
        }
        None => {
            words.extend(argv[1..].iter().cloned());
            script.push_str(&sh_words(&words));
        }
    }
    then_shell(script)
}

/// The words of a shell command line [`sh_words`] wrote: single quotes group, and outside them a
/// backslash takes the next character as it is.
fn sh_split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if !quoted => {
                word.extend(chars.next());
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// The host the command that brings a window back connects to with ssh ([`ssh_command`]'s
/// result), as the user wrote it without the login name; `None` for a local command.
pub fn ssh_host(command: &[String]) -> Option<String> {
    let [sh, flag, script] = command else {
        return None;
    };
    if sh != "sh" || flag != "-c" {
        return None;
    }
    // After the lock that lets one window open the master first comes the ssh itself.
    let run = script.rsplit("exec 9>&-; ").next()?;
    let run = run.split("; exec ").next()?;
    let words = sh_split(run);
    if words.first().map(String::as_str) != Some("ssh") {
        return None;
    }
    let dest = SshArgs::parse(&words[1..])?.dest;
    let dest = dest.strip_prefix("ssh://").unwrap_or(&dest);
    let host = dest.rsplit('@').next()?;
    (!host.is_empty()).then(|| host.to_owned())
}

/// Whether a remote command only attaches tmux (`tmux new-session -A`, `tmux attach`): the
/// command a restored ssh window runs, which the remote tmux's clients name better.
pub fn attaches_tmux(command: &[String]) -> bool {
    let words: Vec<&str> = command.iter().flat_map(|c| c.split_whitespace()).collect();
    words
        .first()
        .is_some_and(|w| w.rsplit('/').next() == Some("tmux"))
        && words[1..].iter().any(|w| {
            matches!(
                *w,
                "new-session" | "new" | "attach-session" | "attach" | "a" | "at"
            )
        })
}

/// The host a terminal title names: the word before its first `": "` (tmux and fish put
/// `host: ` first).
pub fn title_host(title: &str) -> Option<&str> {
    let (host, _) = title.split_once(": ")?;
    (!host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
    .then_some(host)
}

/// tmux's global options that pick its server, `-L name` or `-S path`.
pub fn tmux_socket(argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = argv.iter().skip(1);
    while let Some(a) = args.next() {
        let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty()) else {
            break;
        };
        match flags.chars().next() {
            Some(c @ ('L' | 'S')) if flags.len() == 1 => {
                if let Some(v) = args.next() {
                    out.extend([format!("-{c}"), v.clone()]);
                }
            }
            Some('L' | 'S') => out.push(a.clone()),
            Some('c' | 'f' | 'T') if flags.len() == 1 => {
                args.next();
            }
            _ => {}
        }
    }
    out
}

/// A process under a terminal window: its shell, or a child of the shell.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub pid: i32,
    pub argv: Vec<String>,
    /// The window process it runs under, when that process is the window's terminal; `None` for
    /// a process found under a terminal process by its program name (see [`World::shown`]).
    pub root: Option<i32>,
}

/// What `ssh -G` says about a connection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SshConfig {
    pub hostname: Option<String>,
    /// Whether ControlMaster is on (`auto`, `yes`, `ask` or `autoask`).
    pub multiplexed: bool,
    /// The master's socket, when it exists.
    pub master: bool,
}

/// What [`resolve`] needs from the system, separated for the tests.
pub trait World {
    fn host(&self) -> String;
    /// The shells under the windows' terminals and their children, each with the window process
    /// it runs under.
    fn shown(&self, windows: &[(Option<i32>, &'static Terminal)]) -> Vec<Shown>;
    /// `tmux <socket> list-clients -F` [`CLIENTS`] lines.
    fn tmux_clients(&mut self, socket: &[String]) -> Option<String>;
    fn ssh_config(&mut self, conn: &[String]) -> SshConfig;
    /// The remote `session<TAB>title` lines, asked through the open master.
    fn remote_clients(&mut self, conn: &[String]) -> Option<String>;
}

/// An ssh process under a terminal window.
struct Ssh {
    root: Option<i32>,
    argv: Vec<String>,
    args: SshArgs,
    conn: Vec<String>,
    config: SshConfig,
    /// Whether a window took it.
    used: bool,
}

impl Ssh {
    /// Whether it may show a remote tmux session: it runs a shell or only attaches tmux, over an
    /// open master.
    fn interactive(&self) -> bool {
        (self.args.command.is_empty() || attaches_tmux(&self.args.command)) && self.config.master
    }
}

/// The command that brings back each terminal window, by window ID, for the windows found.
///
/// A window's own process ties it to what it shows. When the window's process is its terminal's
/// and serves no other asked window, the tmux client or ssh under that process is the window's,
/// whatever its title. Titles tie the rest: windows that share one process (a terminal that serves
/// every window from one process), windows whose process is not their terminal's (an X11 window),
/// and the remote tmux session behind an ssh window. Each window takes one client with its title,
/// so two windows get one session only when two clients are on it:
/// - a window whose ssh command attaches a session (a restored window) takes a client on that
///   session;
/// - the other windows, oldest first, pair with the other clients in the order tmux lists them
///   (oldest first), and windows left over stay unmatched.
///
/// An ssh window no remote tmux session names reruns its ssh command: the ssh under its own
/// process, or the one whose host its title names when only one title fits.
pub fn resolve_in(world: &mut impl World, ask: &Ask) -> HashMap<u64, Vec<String>> {
    let host = world.host();
    let mut roots = Vec::new();
    for w in &ask.windows {
        if !roots.contains(&(w.pid, w.terminal)) {
            roots.push((w.pid, w.terminal));
        }
    }
    let shown = world.shown(&roots);
    let name = |s: &Shown| {
        s.argv
            .first()
            .map(|a| a.rsplit('/').next().unwrap_or(a).to_owned())
    };

    // Windows by the process that shows what is in them: their own, or `None` for those with no
    // process of their own, which share what was found by program name.
    let mut groups: Vec<(Option<i32>, Vec<&Window>)> = Vec::new();
    for w in &ask.windows {
        let root = w.pid.filter(|&p| shown.iter().any(|s| s.root == Some(p)));
        match groups.iter_mut().find(|(r, _)| *r == root) {
            Some((_, ws)) => ws.push(w),
            None => groups.push((root, vec![w])),
        }
    }
    for (_, ws) in &mut groups {
        ws.sort_by_key(|w| w.id);
    }
    let lone = |root: Option<i32>, ws: &[&Window]| root.is_some() && ws.len() == 1;
    let mut out = HashMap::new();

    // Local tmux clients: (root, socket, session, title), as tmux lists them.
    let mut by_socket: BTreeMap<Vec<String>, Vec<&Shown>> = BTreeMap::new();
    for s in shown.iter().filter(|s| name(s).as_deref() == Some("tmux")) {
        by_socket.entry(tmux_socket(&s.argv)).or_default().push(s);
    }
    let mut clients = Vec::new();
    for (socket, procs) in &by_socket {
        let Some(lines) = world.tmux_clients(socket) else {
            continue;
        };
        for line in lines.lines() {
            let mut f = line.splitn(3, '\t');
            let (Some(pid), Some(session), Some(title)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let Some(s) = procs
                .iter()
                .find(|s| pid.parse::<i32>().is_ok_and(|p| p == s.pid))
            else {
                continue;
            };
            clients.push((s.root, socket, session.to_owned(), title.to_owned()));
        }
    }
    for (root, ws) in &groups {
        let mine: Vec<_> = clients.iter().filter(|c| c.0 == *root).collect();
        let keys: Vec<_> = mine
            .iter()
            .map(|c| ((c.1, c.2.as_str()), c.3.as_str()))
            .collect();
        let picks = if lone(*root, ws) {
            vec![lone_client(ws[0], &keys)]
        } else {
            pair(ws, &keys, |_| None)
        };
        for (w, pick) in ws.iter().zip(picks) {
            if let Some(c) = pick {
                out.insert(w.id, tmux_command(mine[c].1, &mine[c].2));
            }
        }
    }

    // Remote tmux sessions behind ssh windows.
    let mut sshs = Vec::new();
    for s in shown.iter().filter(|s| name(s).as_deref() == Some("ssh")) {
        let Some(args) = SshArgs::parse(&s.argv[1..]) else {
            continue;
        };
        let conn = args.connection();
        let config = world.ssh_config(&conn);
        sshs.push(Ssh {
            root: s.root,
            argv: s.argv.clone(),
            args,
            conn,
            config,
            used: false,
        });
    }
    let mut conns: Vec<Vec<String>> = Vec::new();
    for s in sshs.iter().filter(|s| s.interactive()) {
        if !conns.contains(&s.conn) {
            conns.push(s.conn.clone());
        }
    }
    for conn in conns {
        let lines = world.remote_clients(&conn).unwrap_or_default();
        let remote: Vec<(&str, &str)> = lines.lines().filter_map(|l| l.split_once('\t')).collect();
        let over = |root: Option<i32>, sshs: &[Ssh]| {
            sshs.iter()
                .any(|s| s.root == root && !s.used && s.conn == conn && s.interactive())
        };
        let mut windows = Vec::new();
        for (root, ws) in &groups {
            if over(*root, &sshs) {
                windows.extend(
                    ws.iter()
                        .filter(|w| !out.contains_key(&w.id))
                        .map(|w| (*root, *w)),
                );
            }
        }
        windows.sort_by_key(|(_, w)| w.id);
        let ws: Vec<&Window> = windows.iter().map(|(_, w)| *w).collect();
        // A lone window's own ssh command names the session it attaches.
        let named = |w: &Window| {
            let (root, _) = windows.iter().find(|(_, x)| x.id == w.id)?;
            let ws = &groups.iter().find(|(r, _)| r == root)?.1;
            if !lone(*root, ws) {
                return None;
            }
            sshs.iter()
                .filter(|s| s.root == *root && s.conn == conn)
                .find_map(|s| attached_session(&s.args.command))
        };
        let picks = pair(&ws, &remote, named);
        for ((root, w), pick) in windows.iter().zip(picks) {
            let Some(c) = pick else {
                continue;
            };
            let session = remote[c].0;
            let mine = |s: &Ssh| s.root == *root && !s.used && s.conn == conn && s.interactive();
            let Some(s) = sshs
                .iter()
                .position(|s| mine(s) && attached_session(&s.args.command) == Some(session))
                .or_else(|| sshs.iter().position(mine))
            else {
                continue;
            };
            let s = &mut sshs[s];
            s.used = true;
            out.insert(
                w.id,
                ssh_command(&s.argv, &s.args, Some(session), s.config.multiplexed),
            );
        }
    }

    // ssh windows no remote tmux named: the window's own ssh, else by the host titles name.
    for (root, ws) in &groups {
        let free: Vec<usize> = (0..sshs.len())
            .filter(|&i| sshs[i].root == *root && !sshs[i].used)
            .collect();
        if free.is_empty() {
            continue;
        }
        if lone(*root, ws) {
            let s = &sshs[free[0]];
            out.entry(ws[0].id)
                .or_insert_with(|| ssh_command(&s.argv, &s.args, None, s.config.multiplexed));
            continue;
        }
        let remote = |out: &HashMap<u64, Vec<String>>| -> Vec<&Window> {
            ws.iter()
                .filter(|w| {
                    !out.contains_key(&w.id) && title_host(&w.title).is_some_and(|h| h != host)
                })
                .copied()
                .collect()
        };
        let mut left = Vec::new();
        for i in free {
            let s = &sshs[i];
            let short = s
                .config
                .hostname
                .as_deref()
                .map(|h| h.split('.').next().unwrap_or(h));
            let fits: Vec<&Window> = remote(&out)
                .into_iter()
                .filter(|w| {
                    title_host(&w.title).is_some_and(|h| h == s.args.dest || Some(h) == short)
                })
                .collect();
            match fits.first() {
                Some(w) if fits.iter().all(|x| x.title == w.title) => {
                    out.insert(
                        w.id,
                        ssh_command(&s.argv, &s.args, None, s.config.multiplexed),
                    );
                }
                _ => left.push(i),
            }
        }
        if let ([i], [w]) = (left.as_slice(), remote(&out).as_slice()) {
            let s = &sshs[*i];
            out.insert(
                w.id,
                ssh_command(&s.argv, &s.args, None, s.config.multiplexed),
            );
        }
    }
    out
}

/// The client a lone window shows, of those under its process: the first under its title, else
/// the first.
fn lone_client<K>(w: &Window, clients: &[(K, &str)]) -> Option<usize> {
    clients
        .iter()
        .position(|c| c.1 == w.title)
        .or_else(|| (!clients.is_empty()).then_some(0))
}

/// Pairs windows (oldest first) with clients (`(session, title)`, in the order tmux lists them)
/// by title, as [`resolve_in`] states; `named` is the session a window's own command attaches.
/// Gives each window's client, by index.
fn pair<K: PartialEq>(
    windows: &[&Window],
    clients: &[(K, &str)],
    named: impl Fn(&Window) -> Option<K>,
) -> Vec<Option<usize>> {
    let mut out = vec![None; windows.len()];
    let mut done: Vec<&str> = Vec::new();
    for title in windows.iter().map(|w| w.title.as_str()) {
        if title.is_empty() || done.contains(&title) {
            continue;
        }
        done.push(title);
        let mut free: Vec<usize> = (0..clients.len())
            .filter(|&c| clients[c].1 == title)
            .collect();
        let mut rest = Vec::new();
        for (i, w) in windows.iter().enumerate().filter(|(_, w)| w.title == title) {
            match named(w).and_then(|k| free.iter().position(|&c| clients[c].0 == k)) {
                Some(at) => out[i] = Some(free.remove(at)),
                None => rest.push(i),
            }
        }
        for (i, c) in rest.into_iter().zip(free) {
            out[i] = Some(c);
        }
    }
    out
}

/// The session a remote command attaches by name: `tmux new-session -A -s S`, `tmux attach -t S`.
fn attached_session(command: &[String]) -> Option<&str> {
    if !attaches_tmux(command) {
        return None;
    }
    let words: Vec<&str> = command.iter().flat_map(|c| c.split_whitespace()).collect();
    words
        .windows(2)
        .find(|w| w[0] == "-s" || w[0] == "-t")
        .map(|w| w[1].trim_end_matches(';'))
}

fn resolve(
    ask: &Ask,
    remote: &mut HashMap<Vec<String>, (Instant, String)>,
) -> HashMap<u64, Vec<String>> {
    let mut world = System {
        remote,
        configs: HashMap::new(),
        titles: ask.titles(),
    };
    resolve_in(&mut world, ask)
}

/// The real [`World`]: `/proc`, tmux and ssh.
struct System<'a> {
    /// Remote answers by connection, with when they were asked.
    remote: &'a mut HashMap<Vec<String>, (Instant, String)>,
    configs: HashMap<Vec<String>, SshConfig>,
    titles: Vec<String>,
}

impl World for System<'_> {
    fn host(&self) -> String {
        let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
        let host = host.trim();
        host.split('.').next().unwrap_or(host).to_owned()
    }

    fn shown(&self, windows: &[(Option<i32>, &'static Terminal)]) -> Vec<Shown> {
        // Each process to look under, and whether it is a window's own terminal process.
        let mut roots: Vec<(i32, bool)> = Vec::new();
        let mut scan: Vec<&str> = Vec::new();
        for &(pid, terminal) in windows {
            let own = pid
                .and_then(cmdline)
                .is_some_and(|argv| terminal.is_exe(&argv[0]));
            if let Some(pid) = pid {
                match roots.iter_mut().find(|r| r.0 == pid) {
                    Some(r) => r.1 |= own,
                    None => roots.push((pid, own)),
                }
            }
            // A window whose process is not its terminal's, such as an X11 window, which comes
            // from xwayland-satellite: the terminal's processes, by name.
            if !own {
                scan.extend(terminal.exes);
            }
        }
        if !scan.is_empty() {
            for pid in named(&scan) {
                if !roots.iter().any(|r| r.0 == pid) {
                    roots.push((pid, false));
                }
            }
        }
        let mut out = Vec::new();
        for (root, own) in roots {
            for shell in children(root) {
                for pid in std::iter::once(shell).chain(children(shell)) {
                    if let Some(argv) = cmdline(pid) {
                        out.push(Shown {
                            pid,
                            argv,
                            root: own.then_some(root),
                        });
                    }
                }
            }
        }
        out
    }

    fn tmux_clients(&mut self, socket: &[String]) -> Option<String> {
        let mut args: Vec<&str> = socket.iter().map(String::as_str).collect();
        args.extend(["list-clients", "-F", CLIENTS]);
        run("tmux", &args)
    }

    fn ssh_config(&mut self, conn: &[String]) -> SshConfig {
        if let Some(c) = self.configs.get(conn) {
            return c.clone();
        }
        let mut args = vec!["-G"];
        args.extend(conn.iter().map(String::as_str));
        let mut config = SshConfig::default();
        for line in run("ssh", &args).unwrap_or_default().lines() {
            match line.split_once(' ') {
                Some(("hostname", v)) => config.hostname = Some(v.to_owned()),
                Some(("controlmaster", v)) => config.multiplexed = v != "false" && v != "no",
                Some(("controlpath", v)) if v != "none" => {
                    config.master = fs::metadata(v).is_ok();
                }
                _ => {}
            }
        }
        config.master &= config.multiplexed;
        self.configs.insert(conn.to_vec(), config.clone());
        config
    }

    fn remote_clients(&mut self, conn: &[String]) -> Option<String> {
        if let Some((at, lines)) = self.remote.get(conn) {
            let has_all = self
                .titles
                .iter()
                .filter(|t| title_host(t).is_some())
                .all(|t| lines.lines().any(|l| l.ends_with(&format!("\t{t}"))));
            if at.elapsed() < REMOTE_FRESH && has_all {
                return Some(lines.clone());
            }
        }
        let mut args: Vec<&str> = QUERY_GUARD.to_vec();
        args.extend(conn.iter().map(String::as_str));
        args.push(REMOTE_CLIENTS);
        let lines = run("ssh", &args)?;
        self.remote
            .insert(conn.to_vec(), (Instant::now(), lines.clone()));
        Some(lines)
    }
}

/// The processes `pid`'s threads started.
fn children(pid: i32) -> Vec<i32> {
    let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    tasks
        .flatten()
        .filter_map(|t| fs::read_to_string(t.path().join("children")).ok())
        .flat_map(|s| {
            s.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<i32>>()
        })
        .collect()
}

/// The processes whose program is one of `exes` (see [`exe_name`]).
fn named(exes: &[&str]) -> Vec<i32> {
    let Ok(procs) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    procs
        .flatten()
        .filter_map(|p| p.file_name().to_str()?.parse().ok())
        .filter(|&pid| cmdline(pid).is_some_and(|argv| exes.contains(&exe_name(&argv[0]))))
        .collect()
}

fn cmdline(pid: i32) -> Option<Vec<String>> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let raw = raw.strip_suffix(&[0]).unwrap_or(&raw);
    let argv: Vec<String> = raw
        .split(|&b| b == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    argv.first().is_some_and(|a| !a.is_empty()).then_some(argv)
}

/// Runs `program` with `args` and returns its output when it succeeds within [`TIMEOUT`].
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    status.success().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn ssh_options_may_follow_the_destination_and_a_command_ends_them() {
        let a = SshArgs::parse(&s(&["build", "-A"])).unwrap();
        assert_eq!(
            (a.options, a.dest, a.command.len()),
            (s(&["-A"]), String::from("build"), 0)
        );
        let a = SshArgs::parse(&s(&["-p", "42042", "-At", "root@h", "htop", "-d", "5"])).unwrap();
        assert_eq!(a.options, s(&["-p", "42042", "-At"]));
        assert_eq!(a.dest, "root@h");
        assert_eq!(a.command, s(&["htop", "-d", "5"]));
        let a = SshArgs::parse(&s(&["-oBatchMode=yes", "-l", "u", "h"])).unwrap();
        assert_eq!(a.connection(), s(&["-oBatchMode=yes", "-l", "u", "h"]));
        assert!(SshArgs::parse(&s(&["-p"])).is_none());
        assert!(SshArgs::parse(&s(&["-A"])).is_none());
    }

    #[test]
    fn a_connection_drops_forwards_and_terminal_flags() {
        let a = SshArgs::parse(&s(&[
            "-A",
            "-t",
            "-L",
            "8080:localhost:80",
            "-p",
            "22",
            "-o",
            "User=x",
            "-J",
            "jump",
            "h",
        ]))
        .unwrap();
        assert_eq!(
            a.connection(),
            s(&["-p", "22", "-o", "User=x", "-J", "jump", "h"])
        );
    }

    #[test]
    fn tmux_sockets_come_from_its_global_options() {
        assert!(tmux_socket(&s(&["tmux", "a", "-t", "5"])).is_empty());
        assert_eq!(
            tmux_socket(&s(&["tmux", "-L", "work", "a"])),
            s(&["-L", "work"])
        );
        assert_eq!(tmux_socket(&s(&["tmux", "-Sx", "attach"])), s(&["-Sx"]));
        assert_eq!(
            tmux_socket(&s(&["tmux", "-f", "conf", "-S", "/run/t", "a"])),
            s(&["-S", "/run/t"])
        );
    }

    #[test]
    fn commands_keep_the_window_open_with_the_users_shell() {
        let c = tmux_command(&[], "claude-native-5");
        assert_eq!(c[..2], s(&["sh", "-c"]));
        assert_eq!(
            c[2],
            "tmux new-session -A -s claude-native-5; exec \"${SHELL:-sh}\""
        );
        let argv = s(&["ssh", "build", "-A"]);
        let args = SshArgs::parse(&argv[1..]).unwrap();
        let c = ssh_command(&argv, &args, Some("0"), false);
        assert_eq!(
            c[2],
            "ssh -t build -A tmux new-session -A -s 0; exec \"${SHELL:-sh}\""
        );
        let c = ssh_command(&argv, &args, Some("bad name;rm"), false);
        assert_eq!(c[2], "ssh build -A; exec \"${SHELL:-sh}\"");
        let c = ssh_command(&argv, &args, None, true);
        assert!(c[2].contains("flock 9; ssh -O check build >/dev/null 2>&1 || ssh -fN build;"));
        assert!(c[2].ends_with("ssh build -A; exec \"${SHELL:-sh}\""));
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn ssh_hosts_come_back_out_of_the_commands_that_reopen_windows() {
        let host = |argv: &[&str], session: Option<&str>, multiplexed: bool| {
            let argv = s(argv);
            let args = SshArgs::parse(&argv[1..]).unwrap();
            ssh_host(&ssh_command(&argv, &args, session, multiplexed))
        };
        assert_eq!(host(&["ssh", "far"], None, false).as_deref(), Some("far"));
        assert_eq!(
            host(&["ssh", "-A", "usr@lab.example"], Some("3"), false).as_deref(),
            Some("lab.example"),
            "without the login name"
        );
        assert_eq!(
            host(&["ssh", "-p", "2222", "far", "htop"], None, true).as_deref(),
            Some("far"),
            "past the lock that opens the master first"
        );
        assert_eq!(
            host(
                &["ssh", "-o", "ProxyCommand=nc %h 'a b'", "far"],
                Some("0"),
                true
            )
            .as_deref(),
            Some("far"),
            "past a quoted option"
        );
        assert_eq!(
            ssh_host(&tmux_command(&[], "5")),
            None,
            "a local tmux session"
        );
        assert_eq!(ssh_host(&s(&["ssh", "far"])), None, "not a command of ours");
        assert_eq!(ssh_host(&[]), None);
    }

    #[test]
    fn shell_words_split_as_they_were_quoted() {
        let words = s(&["ssh", "-o", "a b", "it's", "", "plain"]);
        assert_eq!(sh_split(&sh_words(&words)), words);
        assert_eq!(sh_split("  two   words "), s(&["two", "words"]));
    }

    #[test]
    fn title_hosts_are_the_first_word() {
        assert_eq!(title_host("far: ✳ alerts"), Some("far"));
        assert_eq!(title_host("desk: far: ~"), Some("desk"));
        assert_eq!(title_host("~/src"), None);
        assert_eq!(title_host("a b: c"), None);
    }

    #[derive(Default)]
    struct Fake {
        shown: Vec<Shown>,
        tmux: HashMap<Vec<String>, String>,
        configs: HashMap<Vec<String>, SshConfig>,
        remote: HashMap<Vec<String>, String>,
        remote_asks: usize,
    }

    impl World for Fake {
        fn host(&self) -> String {
            String::from("desk")
        }
        fn shown(&self, _: &[(Option<i32>, &'static Terminal)]) -> Vec<Shown> {
            self.shown.clone()
        }
        fn tmux_clients(&mut self, socket: &[String]) -> Option<String> {
            self.tmux.get(socket).cloned()
        }
        fn ssh_config(&mut self, conn: &[String]) -> SshConfig {
            self.configs.get(conn).cloned().unwrap_or_default()
        }
        fn remote_clients(&mut self, conn: &[String]) -> Option<String> {
            self.remote_asks += 1;
            self.remote.get(conn).cloned()
        }
    }

    /// A process under the Ghostty process 1, which serves every Ghostty window.
    fn shown(pid: i32, argv: &[&str]) -> Shown {
        under(1, pid, argv)
    }

    /// A process under the window process `root`.
    fn under(root: i32, pid: i32, argv: &[&str]) -> Shown {
        Shown {
            pid,
            argv: s(argv),
            root: Some(root),
        }
    }

    /// Ghostty windows, all of process 1, with IDs from 1.
    fn ghostty(titles: &[&str]) -> Ask {
        Ask {
            windows: (1..)
                .zip(titles)
                .map(|(id, title)| Window {
                    id,
                    pid: Some(1),
                    terminal: &GHOSTTY,
                    title: (*title).to_owned(),
                })
                .collect(),
        }
    }

    /// A foot window whose process `pid` is its own.
    fn foot(id: u64, pid: i32, title: &str) -> Window {
        Window {
            id,
            pid: Some(pid),
            terminal: find("foot").unwrap(),
            title: title.to_owned(),
        }
    }

    fn master(hostname: &str) -> SshConfig {
        SshConfig {
            hostname: Some(hostname.to_owned()),
            multiplexed: true,
            master: true,
        }
    }

    #[test]
    fn windows_resolve_to_their_tmux_sessions_and_ssh_commands() {
        let mut world = Fake {
            shown: vec![
                shown(10, &["/run/fish"]),
                shown(11, &["tmux", "a", "-t", "5"]),
                shown(20, &["fish"]),
                shown(21, &["tmux", "a"]),
                shown(30, &["fish"]),
                shown(31, &["ssh", "build", "-A"]),
                shown(40, &["fish"]),
                shown(41, &["ssh", "lab"]),
            ],
            ..Default::default()
        };
        world.tmux.insert(
            vec![],
            String::from("11\t5\tdesk: ✳ notes\n21\t0\tdesk: ✳ newm\n99\t7\tdesk: other client\n"),
        );
        world.configs.insert(s(&["build"]), master("192.0.2.10"));
        world.remote.insert(
            s(&["build"]),
            String::from("0\tfar: ✳ alerts\n3\tfar: elsewhere\n"),
        );
        world.configs.insert(
            s(&["lab"]),
            SshConfig {
                hostname: Some(String::from("lab.example")),
                multiplexed: true,
                master: false,
            },
        );
        let ask = ghostty(&[
            "desk: ✳ notes",
            "desk: ✳ newm",
            "far: ✳ alerts",
            "lab: ~/src",
            "desk: other client",
            "desk: ~",
        ]);
        let out = resolve_in(&mut world, &ask);
        assert_eq!(out[&1], tmux_command(&[], "5"));
        assert_eq!(out[&2], tmux_command(&[], "0"));
        assert!(!out.contains_key(&5), "not a Ghostty window's client");
        let build = s(&["ssh", "build", "-A"]);
        let build_args = SshArgs::parse(&build[1..]).unwrap();
        assert_eq!(out[&3], ssh_command(&build, &build_args, Some("0"), true));
        let lab = s(&["ssh", "lab"]);
        let lab_args = SshArgs::parse(&lab[1..]).unwrap();
        assert_eq!(
            out[&4],
            ssh_command(&lab, &lab_args, None, true),
            "no master: by the host its title names"
        );
        assert!(
            !out.contains_key(&6),
            "a plain shell keeps the old relaunch"
        );
        assert_eq!(world.remote_asks, 1, "only an open master is asked");
    }

    #[test]
    fn a_lone_ssh_window_takes_the_lone_remote_title() {
        let mut world = Fake {
            shown: vec![shown(31, &["ssh", "work"])],
            ..Default::default()
        };
        let out = resolve_in(&mut world, &ghostty(&["far: ~", "desk: ~"]));
        assert_eq!(out.len(), 1);
        assert!(out[&1][2].contains("ssh work"));
    }

    fn restored(session: &str) -> Vec<String> {
        s(&[
            "ssh",
            "-t",
            "far",
            "tmux",
            "new-session",
            "-A",
            "-s",
            session,
        ])
    }

    #[test]
    fn restored_ssh_windows_keep_their_remote_sessions() {
        let mut world = Fake {
            shown: vec![
                shown(10, &["sh", "-c", "..."]),
                Shown {
                    argv: restored("far"),
                    ..shown(11, &[])
                },
                shown(20, &["sh", "-c", "..."]),
                Shown {
                    argv: restored("far2"),
                    ..shown(21, &[])
                },
            ],
            ..Default::default()
        };
        world.configs.insert(s(&["far"]), master("far"));
        world
            .remote
            .insert(s(&["far"]), String::from("far\tfar: ~\nfar2\tfar: /tmp\n"));
        let out = resolve_in(&mut world, &ghostty(&["far: ~", "far: /tmp"]));
        let argv = restored("far");
        let args = SshArgs::parse(&argv[1..]).unwrap();
        assert!(attaches_tmux(&args.command));
        assert_eq!(out[&1], ssh_command(&argv, &args, Some("far"), true));
        assert!(
            out[&1][2].ends_with("ssh -t far tmux new-session -A -s far; exec \"${SHELL:-sh}\"")
        );
        assert!(
            out[&2][2].ends_with("ssh -t far tmux new-session -A -s far2; exec \"${SHELL:-sh}\"")
        );
        assert!(!attaches_tmux(&s(&["htop"])));
        assert!(!attaches_tmux(&s(&["tmux", "kill-server"])));
        assert!(attaches_tmux(&s(&["tmux a -t 0"])));
        assert_eq!(attached_session(&s(&["tmux a -t 0"])), Some("0"));
        assert_eq!(attached_session(&s(&["tmux", "attach"])), None);
        assert_eq!(attached_session(&s(&["htop", "-s", "x"])), None);
    }

    const ALIKE: &str = "desk: me@desk: ~";

    #[test]
    fn windows_with_one_title_on_three_tmux_sessions_keep_their_own() {
        // Three windows, each its own terminal process, whose tmux titles are the same; tmux
        // lists the clients in another order than the windows.
        let mut world = Fake {
            shown: vec![
                under(100, 101, &["fish"]),
                under(100, 102, &["tmux", "new-session", "-A", "-s", "work1"]),
                under(200, 201, &["fish"]),
                under(200, 202, &["tmux", "new-session", "-A", "-s", "work2"]),
                under(300, 301, &["tmux", "new-session", "-A", "-s", "work3"]),
            ],
            ..Default::default()
        };
        world.tmux.insert(
            vec![],
            format!("301\twork3\t{ALIKE}\n202\twork2\t{ALIKE}\n102\twork1\t{ALIKE}\n"),
        );
        let ask = Ask {
            windows: vec![
                foot(7, 100, ALIKE),
                foot(8, 200, ALIKE),
                foot(9, 300, ALIKE),
            ],
        };
        let out = resolve_in(&mut world, &ask);
        assert_eq!(out[&7], tmux_command(&[], "work1"));
        assert_eq!(out[&8], tmux_command(&[], "work2"));
        assert_eq!(out[&9], tmux_command(&[], "work3"));

        // One process serving three windows: only titles tell them apart, so they pair in order,
        // each with a session of its own.
        let mut world = Fake {
            shown: vec![
                shown(11, &["tmux", "a", "-t", "work1"]),
                shown(21, &["tmux", "a", "-t", "work2"]),
                shown(31, &["tmux", "a", "-t", "work3"]),
            ],
            ..Default::default()
        };
        world.tmux.insert(
            vec![],
            format!("11\twork1\t{ALIKE}\n21\twork2\t{ALIKE}\n31\twork3\t{ALIKE}\n"),
        );
        let out = resolve_in(&mut world, &ghostty(&[ALIKE, ALIKE, ALIKE]));
        assert_eq!(out[&1], tmux_command(&[], "work1"));
        assert_eq!(out[&2], tmux_command(&[], "work2"));
        assert_eq!(out[&3], tmux_command(&[], "work3"));
    }

    #[test]
    fn a_window_takes_the_tmux_client_under_it_whatever_its_title() {
        let mut world = Fake {
            shown: vec![under(100, 102, &["tmux"])],
            ..Default::default()
        };
        world
            .tmux
            .insert(vec![], String::from("102\twork1\tdesk: another title\n"));
        let out = resolve_in(
            &mut world,
            &Ask {
                windows: vec![foot(7, 100, "~")],
            },
        );
        assert_eq!(out[&7], tmux_command(&[], "work1"));
    }

    #[test]
    fn windows_on_one_tmux_session_all_get_it() {
        let mut world = Fake {
            shown: vec![
                under(100, 102, &["tmux", "a"]),
                under(200, 202, &["tmux", "a"]),
            ],
            ..Default::default()
        };
        world.tmux.insert(
            vec![],
            format!("102\twork1\t{ALIKE}\n202\twork1\t{ALIKE}\n"),
        );
        let ask = Ask {
            windows: vec![foot(7, 100, ALIKE), foot(8, 200, ALIKE)],
        };
        let out = resolve_in(&mut world, &ask);
        assert_eq!(out[&7], tmux_command(&[], "work1"));
        assert_eq!(out[&8], tmux_command(&[], "work1"));

        // Within one process: two clients on the session, so two windows; a third window with the
        // title has no client and keeps the plain relaunch.
        let mut world = Fake {
            shown: vec![shown(11, &["tmux", "a"]), shown(21, &["tmux", "a"])],
            ..Default::default()
        };
        world
            .tmux
            .insert(vec![], format!("11\twork1\t{ALIKE}\n21\twork1\t{ALIKE}\n"));
        let out = resolve_in(&mut world, &ghostty(&[ALIKE, ALIKE, ALIKE]));
        assert_eq!(out[&1], tmux_command(&[], "work1"));
        assert_eq!(out[&2], tmux_command(&[], "work1"));
        assert!(!out.contains_key(&3), "no client left for it");
    }

    #[test]
    fn ssh_windows_with_one_title_keep_their_own_remote_sessions() {
        const FAR: &str = "far: me@far: ~";
        let lines = |sessions: &[&str]| -> String {
            sessions.iter().map(|s| format!("{s}\t{FAR}\n")).collect()
        };
        let plain = s(&["ssh", "far"]);
        let args = SshArgs::parse(&plain[1..]).unwrap();
        let three = Ask {
            windows: vec![foot(7, 100, FAR), foot(8, 200, FAR), foot(9, 300, FAR)],
        };

        // Plain ssh windows: they pair with the remote clients in order.
        let mut world = Fake {
            shown: vec![
                under(100, 101, &["ssh", "far"]),
                under(200, 201, &["ssh", "far"]),
                under(300, 301, &["ssh", "far"]),
            ],
            ..Default::default()
        };
        world.configs.insert(s(&["far"]), master("far"));
        world.remote.insert(s(&["far"]), lines(&["A", "B", "C"]));
        let out = resolve_in(&mut world, &three);
        assert_eq!(out[&7], ssh_command(&plain, &args, Some("A"), true));
        assert_eq!(out[&8], ssh_command(&plain, &args, Some("B"), true));
        assert_eq!(out[&9], ssh_command(&plain, &args, Some("C"), true));
        assert_eq!(world.remote_asks, 1, "one question per connection");

        // Restored windows: each takes the session its command attaches, though other clients
        // with the title come first.
        let mut world = Fake {
            shown: vec![
                Shown {
                    argv: restored("C"),
                    ..under(100, 101, &[])
                },
                Shown {
                    argv: restored("A"),
                    ..under(200, 201, &[])
                },
                Shown {
                    argv: restored("B"),
                    ..under(300, 301, &[])
                },
            ],
            ..Default::default()
        };
        world.configs.insert(s(&["far"]), master("far"));
        world
            .remote
            .insert(s(&["far"]), lines(&["A", "B", "C", "C", "A", "B"]));
        let out = resolve_in(&mut world, &three);
        for (id, session) in [(7, "C"), (8, "A"), (9, "B")] {
            let argv = restored(session);
            let args = SshArgs::parse(&argv[1..]).unwrap();
            assert_eq!(out[&id], ssh_command(&argv, &args, Some(session), true));
        }

        // Fewer remote clients than windows: the windows left over rerun their own ssh.
        let mut world = Fake {
            shown: vec![
                under(100, 101, &["ssh", "far"]),
                under(200, 201, &["ssh", "far"]),
            ],
            ..Default::default()
        };
        world.configs.insert(s(&["far"]), master("far"));
        world.remote.insert(s(&["far"]), lines(&["A"]));
        let ask = Ask {
            windows: vec![foot(7, 100, FAR), foot(8, 200, FAR)],
        };
        let out = resolve_in(&mut world, &ask);
        assert_eq!(out[&7], ssh_command(&plain, &args, Some("A"), true));
        assert_eq!(out[&8], ssh_command(&plain, &args, None, true));

        // One process serving both windows, with one ssh: one window takes it, the other keeps
        // the plain relaunch.
        let mut world = Fake {
            shown: vec![shown(31, &["ssh", "far"])],
            ..Default::default()
        };
        world.configs.insert(s(&["far"]), master("far"));
        world.remote.insert(s(&["far"]), lines(&["A", "B"]));
        let out = resolve_in(&mut world, &ghostty(&[FAR, FAR]));
        assert_eq!(out[&1], ssh_command(&plain, &args, Some("A"), true));
        assert!(!out.contains_key(&2));

        // No master: windows of one process with one title pair with its ssh processes.
        let mut world = Fake {
            shown: vec![shown(31, &["ssh", "far"]), shown(41, &["ssh", "far", "-A"])],
            ..Default::default()
        };
        let out = resolve_in(&mut world, &ghostty(&[FAR, FAR]));
        let other = s(&["ssh", "far", "-A"]);
        let other_args = SshArgs::parse(&other[1..]).unwrap();
        assert_eq!(out[&1], ssh_command(&plain, &args, None, false));
        assert_eq!(out[&2], ssh_command(&other, &other_args, None, false));
    }

    #[test]
    fn a_window_reruns_the_ssh_under_it_whatever_its_title() {
        let mut world = Fake {
            shown: vec![under(100, 101, &["ssh", "lab"])],
            ..Default::default()
        };
        let out = resolve_in(
            &mut world,
            &Ask {
                windows: vec![foot(7, 100, "~")],
            },
        );
        let lab = s(&["ssh", "lab"]);
        let args = SshArgs::parse(&lab[1..]).unwrap();
        assert_eq!(out[&7], ssh_command(&lab, &args, None, false));
    }

    /// Each terminal's window in `/w`, and from `/h` attached to tmux session 5, exactly.
    fn launches(app_id: &str) -> (Vec<String>, Option<PathBuf>, Vec<String>) {
        let terminal = find(app_id).unwrap();
        let window = terminal.window(Path::new("/w"), &[]);
        let attach = s(&["tmux", "new-session", "-A", "-s", "5"]);
        let running = terminal.running(Path::new("/h"), &[], attach);
        (window.command, window.cwd, running.command)
    }

    #[test]
    fn every_terminal_is_found_by_its_app_id_or_x11_class() {
        for (app_id, program) in [
            ("com.mitchellh.ghostty", "ghostty"),
            ("Alacritty", "alacritty"),
            ("alacritty", "alacritty"),
            ("kitty", "kitty"),
            ("foot", "foot"),
            ("footclient", "footclient"),
            ("org.kde.konsole", "konsole"),
            ("konsole", "konsole"),
            ("org.wezfurlong.wezterm", "wezterm"),
            ("org.gnome.Terminal", "gnome-terminal"),
            ("Gnome-terminal", "gnome-terminal"),
            ("xfce4-terminal", "xfce4-terminal"),
            ("Xfce4-terminal", "xfce4-terminal"),
            ("org.xfce.terminal", "xfce4-terminal"),
            ("XTerm", "xterm"),
            ("xterm", "xterm"),
        ] {
            assert_eq!(
                find(app_id).map(|t| t.program[0]),
                Some(program),
                "{app_id}"
            );
        }
        assert_eq!(find("org.example.NotATerminal"), None);
        assert_eq!(find("chromium-browser"), None);
    }

    #[test]
    fn terminal_processes_are_known_by_their_program_name() {
        let foot = find("footclient").unwrap();
        assert!(foot.is_exe("/nix/store/x-foot-1.28.0/bin/foot"));
        assert!(find("kitty")
            .unwrap()
            .is_exe("/nix/store/x/bin/.kitty-wrapped"));
        assert!(find("org.gnome.Terminal")
            .unwrap()
            .is_exe("/x/libexec/gnome-terminal-server"));
        assert!(find("org.wezfurlong.wezterm")
            .unwrap()
            .is_exe("wezterm-gui"));
        assert!(!find("XTerm").unwrap().is_exe("xwayland-satellite"));
        assert_eq!(exe_name(".wrapped"), ".wrapped");
        let me = std::env::args().next().unwrap();
        assert!(
            named(&[exe_name(&me)]).contains(&(std::process::id() as i32)),
            "/proc names this test's own process"
        );
    }

    #[test]
    fn ghostty_opens_its_windows_as_before() {
        assert_eq!(
            launches("com.mitchellh.ghostty"),
            (
                s(&["ghostty", "+new-window", "--working-directory=/w"]),
                None,
                s(&["ghostty", "+new-window", "--working-directory=/h", "-e"])
                    .into_iter()
                    .chain(s(&["tmux", "new-session", "-A", "-s", "5"]))
                    .collect(),
            )
        );
    }

    #[test]
    fn each_terminal_opens_a_window_with_its_own_flags() {
        let attach = ["tmux", "new-session", "-A", "-s", "5"];
        let with = |words: &[&str]| s(&[words, &attach[..]].concat());
        assert_eq!(
            launches("Alacritty"),
            (
                s(&["alacritty", "--working-directory", "/w"]),
                None,
                with(&["alacritty", "--working-directory", "/h", "-e"]),
            )
        );
        assert_eq!(
            launches("kitty"),
            (
                s(&["kitty", "--directory=/w"]),
                None,
                with(&["kitty", "--directory=/h"]),
            )
        );
        assert_eq!(
            launches("foot"),
            (
                s(&["foot", "--working-directory=/w"]),
                None,
                with(&["foot", "--working-directory=/h"]),
            )
        );
        assert_eq!(
            launches("footclient"),
            (
                s(&["footclient", "--working-directory=/w"]),
                None,
                with(&["footclient", "--working-directory=/h"]),
            )
        );
        assert_eq!(
            launches("org.kde.konsole"),
            (
                s(&["konsole", "--workdir", "/w"]),
                None,
                with(&["konsole", "--workdir", "/h", "-e"]),
            )
        );
        assert_eq!(
            launches("org.wezfurlong.wezterm"),
            (
                s(&["wezterm", "start", "--cwd", "/w"]),
                None,
                with(&["wezterm", "start", "--cwd", "/h", "--"]),
            )
        );
        assert_eq!(
            launches("org.gnome.Terminal"),
            (
                s(&["gnome-terminal", "--working-directory=/w"]),
                None,
                with(&["gnome-terminal", "--working-directory=/h", "--"]),
            )
        );
        assert_eq!(
            launches("xfce4-terminal"),
            (
                s(&["xfce4-terminal", "--working-directory=/w"]),
                None,
                with(&["xfce4-terminal", "--working-directory=/h", "-x"]),
            )
        );
        assert_eq!(
            launches("XTerm"),
            (
                s(&["xterm"]),
                Some(PathBuf::from("/w")),
                with(&["xterm", "-e"])
            ),
            "xterm has no directory flag: the launch starts there"
        );
        let xterm = find("XTerm")
            .unwrap()
            .running(Path::new("/h"), &[], Vec::new());
        assert_eq!(xterm.cwd, Some(PathBuf::from("/h")));
    }

    #[test]
    fn kitty_keeps_the_single_instance_it_ran_in() {
        let kitty = find("kitty").unwrap();
        assert_eq!(kitty.instance(&s(&["kitty"])), Vec::<String>::new());
        assert_eq!(
            kitty.instance(&s(&["kitty", "-1"])),
            s(&["--single-instance"])
        );
        assert_eq!(
            kitty.instance(&s(&[
                "/bin/kitty",
                "--single-instance",
                "--instance-group",
                "w"
            ])),
            s(&["--single-instance", "--instance-group=w"])
        );
        assert_eq!(
            kitty.instance(&s(&["kitty", "--instance-group=w", "-1"])),
            s(&["--single-instance", "--instance-group=w"])
        );
        assert_eq!(
            kitty.instance(&s(&["kitty", "--instance-group=w"])),
            Vec::<String>::new(),
            "a group means nothing without --single-instance"
        );
        let instance = kitty.instance(&s(&["kitty", "-1"]));
        assert_eq!(
            kitty
                .running(Path::new("/h"), &instance, s(&["sh"]))
                .command,
            s(&["kitty", "--single-instance", "--directory=/h", "sh"])
        );
        assert_eq!(
            find("foot").unwrap().instance(&s(&["foot", "-1"])),
            Vec::<String>::new(),
            "no flag of its own"
        );
    }

    #[test]
    fn konsole_titles_drop_the_name_qt_appends() {
        let konsole = find("org.kde.konsole").unwrap();
        assert_eq!(
            konsole.title("desk: ✳ newm \u{2014} Konsole"),
            "desk: ✳ newm"
        );
        assert_eq!(konsole.title("desk: ~"), "desk: ~");
        assert_eq!(
            GHOSTTY.title("desk: ~ \u{2014} Konsole"),
            "desk: ~ \u{2014} Konsole"
        );
    }
}
