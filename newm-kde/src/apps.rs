//! One name per app, whatever form its windows give: an app ID differs by case between an app's
//! X11 class and its Wayland app ID (`Chromium-browser`, `chromium-browser`), and between toolkits
//! and hosts (`dolphin`, `org.kde.dolphin`). Session restore, session transfer, open-size memory
//! and the usage log compare apps by [`canonical`], so a window matches its app on every host.
//!
//! [`canonical`] lowercases an app ID, drops a `.desktop` suffix and, when the desktop files name
//! it, gives the desktop file's ID (lowercased). A desktop file names an app ID by, in order:
//! 1. its own ID (`chromium-browser.desktop`);
//! 2. its `StartupWMClass`, in a file that is shown in menus;
//! 3. the program its `Exec` runs, or the last part of a reverse-DNS ID (`dolphin` for
//!    `org.kde.dolphin`), in a file that is shown in menus;
//! 4. its `StartupWMClass`, in a hidden file (`NoDisplay` or `Hidden`).
//!
//! An earlier step wins over a later one. A name that two desktop files claim in the same step
//! names neither of them, nor any file in a later step: it stays itself. Window rules do not use
//! this: their regular expressions match the app ID as the app gives it.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::exec;

/// The lowercased app ID, without surrounding spaces nor a `.desktop` suffix.
pub fn key(app_id: &str) -> String {
    let id = app_id.trim();
    let id = match id.len().checked_sub(".desktop".len()) {
        Some(n) if id.is_char_boundary(n) && id[n..].eq_ignore_ascii_case(".desktop") => &id[..n],
        _ => id,
    };
    id.to_lowercase()
}

/// A desktop file, as far as app identity goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// The desktop file ID: its file name without `.desktop`.
    pub id: String,
    pub wm_class: Option<String>,
    pub exec: Option<String>,
    /// `NoDisplay=true` or `Hidden=true`.
    pub hidden: bool,
    pub path: Option<PathBuf>,
}

impl Entry {
    /// The entry in desktop file `text` with ID `id`.
    pub fn parse(id: &str, text: &str) -> Self {
        let flag = |name| entry_key(text, name).is_some_and(|v| v.eq_ignore_ascii_case("true"));
        Self {
            id: id.to_owned(),
            wm_class: entry_key(text, "StartupWMClass")
                .filter(|c| !c.is_empty())
                .map(str::to_owned),
            exec: entry_key(text, "Exec").map(str::to_owned),
            hidden: flag("NoDisplay") || flag("Hidden"),
            path: None,
        }
    }

    /// The names the entry gives an app ID in step `step` (2 to 4) of the module docs.
    fn names(&self, step: u8) -> Vec<String> {
        let class = || self.wm_class.as_deref().map(key).into_iter().collect();
        match (step, self.hidden) {
            (2, false) | (4, true) => class(),
            (3, false) => [self.program(), self.dns_tail()]
                .into_iter()
                .flatten()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The program `Exec` runs: its first argument's file name, past `env` and its variables.
    fn program(&self) -> Option<String> {
        let argv = exec::argv(self.exec.as_deref()?, &exec::Fields::default());
        let mut args = argv.iter().map(String::as_str);
        let mut first = args.next()?;
        if Path::new(first).file_name().is_some_and(|n| n == "env") {
            first = args.find(|a| !a.contains('=') && !a.starts_with('-'))?;
        }
        let name = Path::new(first).file_name()?.to_str()?;
        (!SHELLS.contains(&name)).then(|| key(name))
    }

    /// The last part of a reverse-DNS ID with at least three parts.
    fn dns_tail(&self) -> Option<String> {
        let id = key(&self.id);
        if id.matches('.').count() < 2 {
            return None;
        }
        id.rsplit('.')
            .next()
            .filter(|tail| !tail.is_empty())
            .map(str::to_owned)
    }
}

/// Programs that run other programs, which name no app.
const SHELLS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "fish", "python", "python3", "flatpak",
];

/// The names desktop files give apps.
#[derive(Debug, Clone, Default)]
pub struct Index {
    /// Each name a desktop file gives, to that file's ID (as [`key`]).
    aliases: HashMap<String, String>,
    paths: HashMap<String, PathBuf>,
}

impl Index {
    /// The index of `entries`; of two entries with the same ID, the first counts.
    pub fn new(entries: impl IntoIterator<Item = Entry>) -> Self {
        let mut ids = HashSet::new();
        let entries: Vec<(String, Entry)> = entries
            .into_iter()
            .map(|e| (key(&e.id), e))
            .filter(|(id, _)| !id.is_empty() && ids.insert(id.clone()))
            .collect();
        let mut index = Self::default();
        for (id, e) in &entries {
            index.aliases.insert(id.clone(), id.clone());
            if let Some(path) = &e.path {
                index.paths.insert(id.clone(), path.clone());
            }
        }
        let mut blocked = HashSet::new();
        for step in 2..=4 {
            let mut claims: HashMap<String, Option<&String>> = HashMap::new();
            for (id, e) in &entries {
                for name in e.names(step) {
                    if name.is_empty() || &name == id {
                        continue;
                    }
                    claims
                        .entry(name)
                        .and_modify(|claim| {
                            if *claim != Some(id) {
                                *claim = None;
                            }
                        })
                        .or_insert(Some(id));
                }
            }
            for (name, claim) in claims {
                if index.aliases.contains_key(&name) || blocked.contains(&name) {
                    continue;
                }
                match claim {
                    Some(id) => {
                        index.aliases.insert(name, id.clone());
                    }
                    None => {
                        blocked.insert(name);
                    }
                }
            }
        }
        index
    }

    /// The index of the desktop files in `<dir>/applications` of each of `dirs`; an earlier
    /// directory's file wins over a later one's with the same ID.
    pub fn read(dirs: &[PathBuf]) -> Self {
        let mut entries = Vec::new();
        for dir in dirs {
            let Ok(files) = fs::read_dir(dir.join("applications")) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = files.flatten().map(|f| f.path()).collect();
            paths.sort();
            for path in paths {
                let Some(id) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".desktop"))
                else {
                    continue;
                };
                if let Ok(text) = fs::read_to_string(&path) {
                    entries.push(Entry {
                        path: Some(path.clone()),
                        ..Entry::parse(id, &text)
                    });
                }
            }
        }
        Self::new(entries)
    }

    /// The app's one name: the ID of the desktop file that names `app_id`, else `app_id`, both
    /// as [`key`].
    pub fn canonical(&self, app_id: &str) -> String {
        let key = key(app_id);
        self.aliases.get(&key).cloned().unwrap_or(key)
    }

    /// Whether the desktop files name `app_id`.
    pub fn knows(&self, app_id: &str) -> bool {
        self.aliases.contains_key(&key(app_id))
    }

    /// The desktop file of the app `app_id` names.
    pub fn path(&self, app_id: &str) -> Option<&Path> {
        self.paths
            .get(&self.canonical(app_id))
            .map(PathBuf::as_path)
    }
}

/// How long the shared index stays fresh: a later app ID it does not know reads the desktop
/// files again.
const FRESH: Duration = Duration::from_secs(10);

/// App IDs the shared index remembers not knowing, before it forgets them all.
const MAX_UNKNOWN: usize = 4096;

struct Shared {
    index: Index,
    read_at: Instant,
    /// App IDs (as [`key`]) the index did not know when last asked.
    unknown: HashSet<String>,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

/// Makes [`canonical`] follow the desktop files in [`data_dirs`]. Until then (and in tests),
/// [`canonical`] is [`key`].
pub fn enable() {
    let shared = Shared {
        index: Index::read(&data_dirs()),
        read_at: Instant::now(),
        unknown: HashSet::new(),
    };
    *SHARED.lock().unwrap_or_else(PoisonError::into_inner) = Some(shared);
}

/// [`Index::canonical`] on the desktop files [`enable`] read. An app ID the index does not know,
/// first asked about when the index is older than [`FRESH`], reads them again, so an app installed
/// later is known.
pub fn canonical(app_id: &str) -> String {
    let mut guard = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(shared) = guard.as_mut() else {
        return key(app_id);
    };
    if !shared.index.knows(app_id) {
        let key = key(app_id);
        if shared.unknown.len() >= MAX_UNKNOWN {
            shared.unknown.clear();
        }
        if shared.unknown.insert(key) && shared.read_at.elapsed() >= FRESH {
            shared.index = Index::read(&data_dirs());
            shared.read_at = Instant::now();
        }
    }
    shared.index.canonical(app_id)
}

/// Whether app IDs `a` and `b` are one app ([`canonical`]).
pub fn same(a: &str, b: &str) -> bool {
    a == b || canonical(a) == canonical(b)
}

/// `$XDG_DATA_HOME` (`~/.local/share`), then `$XDG_DATA_DIRS` (`/usr/local/share:/usr/share`).
pub fn data_dirs() -> Vec<PathBuf> {
    let env = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let dirs = env("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(std::env::split_paths(&dirs))
        .collect()
}

/// A key of a desktop file's `[Desktop Entry]` group.
pub fn entry_key<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, wm_class: Option<&str>, exec: &str, hidden: bool) -> Entry {
        Entry {
            id: id.to_owned(),
            wm_class: wm_class.map(str::to_owned),
            exec: Some(exec.to_owned()),
            hidden,
            path: Some(PathBuf::from(format!("/apps/{id}.desktop"))),
        }
    }

    /// Desktop files as a desktop host ships them.
    fn host() -> Index {
        Index::new([
            entry(
                "chromium-browser",
                Some("chromium-browser"),
                "chromium %U",
                false,
            ),
            entry("codium", Some("vscodium"), "codium %F", false),
            entry("codium-url-handler", None, "codium --open-url %U", true),
            entry("Notely", Some("notely-desktop"), "notely-desktop %u", false),
            entry("org.kde.dolphin", Some("dolphin"), "dolphin %u", false),
            entry("org.kde.gwenview", None, "gwenview %U", false),
            entry(
                "org.kde.gwenview_importer",
                Some("gwenview"),
                "gwenview_importer %u",
                true,
            ),
            entry(
                "org.kde.spectacle",
                None,
                "/nix/store/x-spectacle/bin/spectacle",
                false,
            ),
            entry(
                "firefox",
                Some("firefox"),
                "firefox --name firefox %U",
                false,
            ),
            entry(
                "com.mitchellh.ghostty",
                Some("com.mitchellh.ghostty"),
                "ghostty",
                false,
            ),
            entry("Alacritty", Some("Alacritty"), "alacritty", false),
            entry("steam", None, "steam %U", false),
            entry("org.example.Recorder", None, "recorder-app", false),
            entry("okularApplication_pdf", None, "okular %U", true),
            entry("org.kde.okular", Some("okular"), "okular %U", false),
            entry("a-viewer", None, "env GDK_BACKEND=x11 xdg-open %f", false),
            entry("b-viewer", None, "xdg-open %f", false),
            entry("helper", None, "sh -c 'exec helper'", false),
        ])
    }

    #[test]
    fn key_folds_case_and_drops_the_desktop_suffix() {
        assert_eq!(key("Chromium-browser"), "chromium-browser");
        assert_eq!(key(" org.kde.Dolphin.desktop "), "org.kde.dolphin");
        assert_eq!(key("Notely.DESKTOP"), "notely");
        assert_eq!(key(".desktop"), "");
        assert_eq!(key("ÉCRAN"), "écran");
    }

    #[test]
    fn chromium_is_one_app_under_x11_and_wayland() {
        let index = host();
        for id in [
            "Chromium-browser",
            "chromium-browser",
            "chromium",
            "chromium-browser.desktop",
        ] {
            assert_eq!(index.canonical(id), "chromium-browser", "{id}");
        }
    }

    #[test]
    fn startup_wm_class_names_the_desktop_file() {
        let index = host();
        assert_eq!(index.canonical("VSCodium"), "codium");
        assert_eq!(index.canonical("codium"), "codium");
        assert_eq!(index.canonical("Notely-desktop"), "notely");
        assert_eq!(index.canonical("Notely"), "notely");
        assert_eq!(index.canonical("dolphin"), "org.kde.dolphin");
        assert_eq!(index.canonical("org.kde.dolphin"), "org.kde.dolphin");
        assert_eq!(index.canonical("Okular"), "org.kde.okular");
    }

    #[test]
    fn a_shown_file_wins_over_a_hidden_one() {
        let index = host();
        assert_eq!(index.canonical("gwenview"), "org.kde.gwenview");
        assert_eq!(index.canonical("gwenview_importer"), "gwenview_importer");
    }

    #[test]
    fn program_and_reverse_dns_tail_name_shown_files() {
        let index = host();
        assert_eq!(index.canonical("spectacle"), "org.kde.spectacle");
        assert_eq!(index.canonical("ghostty"), "com.mitchellh.ghostty");
        assert_eq!(index.canonical("recorder-app"), "org.example.recorder");
        assert_eq!(index.canonical("Recorder"), "org.example.recorder");
        assert_eq!(index.canonical("alacritty"), "alacritty");
    }

    #[test]
    fn a_name_two_files_claim_names_neither() {
        let index = host();
        assert_eq!(index.canonical("xdg-open"), "xdg-open");
        assert_eq!(index.canonical("sh"), "sh");
    }

    #[test]
    fn unknown_ids_only_fold_case() {
        let index = host();
        assert_eq!(index.canonical("Steam_App_570"), "steam_app_570");
        assert_eq!(index.canonical("game-player.x86_64"), "game-player.x86_64");
        assert!(!index.knows("steam_app_570"));
        assert!(index.knows("STEAM"));
    }

    #[test]
    fn the_first_file_with_an_id_counts() {
        let index = Index::new([
            entry("tool", Some("tool-x11"), "tool", false),
            entry("Tool", Some("other"), "other", false),
        ]);
        assert_eq!(index.canonical("tool-x11"), "tool");
        assert_eq!(index.canonical("other"), "other");
        assert_eq!(
            index.path("TOOL-X11"),
            Some(Path::new("/apps/tool.desktop"))
        );
    }

    #[test]
    fn a_desktop_id_wins_over_another_files_class() {
        let index = Index::new([
            entry("viewer", None, "viewer", false),
            entry("viewer-pro", Some("viewer"), "viewer-pro", false),
        ]);
        assert_eq!(index.canonical("viewer"), "viewer");
        assert_eq!(index.canonical("viewer-pro"), "viewer-pro");
    }

    #[test]
    fn parse_reads_the_desktop_entry_group_only() {
        let text =
            "[Desktop Entry]\nName=Chromium\nExec=chromium %U\nStartupWMClass=chromium-browser\n\
                    NoDisplay=false\n[Desktop Action new-window]\nExec=chromium --new-window\n";
        let e = Entry::parse("chromium-browser", text);
        assert_eq!(e.wm_class.as_deref(), Some("chromium-browser"));
        assert_eq!(e.exec.as_deref(), Some("chromium %U"));
        assert!(!e.hidden);
        assert!(Entry::parse("x", "[Desktop Entry]\nNoDisplay=true\n").hidden);
        assert!(Entry::parse("x", "[Desktop Entry]\nHidden=True\n").hidden);
    }

    #[test]
    fn read_takes_the_earlier_directory() {
        let root = std::env::temp_dir().join(format!("newm-kde-apps-{}", std::process::id()));
        let (high, low) = (root.join("high"), root.join("low"));
        for dir in [&high, &low] {
            fs::create_dir_all(dir.join("applications")).unwrap();
        }
        fs::write(
            high.join("applications/app.desktop"),
            "[Desktop Entry]\nExec=app\nStartupWMClass=App-X\n",
        )
        .unwrap();
        fs::write(
            low.join("applications/app.desktop"),
            "[Desktop Entry]\nExec=app\nStartupWMClass=Other\n",
        )
        .unwrap();
        fs::write(low.join("applications/notes.txt"), "not a desktop file").unwrap();
        let index = Index::read(&[high.clone(), low, root.join("missing")]);
        assert_eq!(index.canonical("app-x"), "app");
        assert_eq!(index.canonical("other"), "other");
        assert_eq!(
            index.path("App-X"),
            Some(high.join("applications/app.desktop").as_path())
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn without_enable_canonical_is_the_key() {
        assert_eq!(canonical("Chromium-browser"), "chromium-browser");
        assert!(same("Chromium-browser", "chromium-browser"));
        assert!(!same("chromium-browser", "firefox"));
    }
}
