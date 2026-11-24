//! Launch shortcuts from desktop files, as kglobalacceld's `KServiceActionComponent` serves them:
//! every `<id>.desktop` under `$XDG_DATA_DIRS/kglobalaccel` (Spectacle, Konsole, Dolphin and others
//! link theirs there) is a component named after the file. Its `X-KDE-Shortcuts` keys belong to the
//! action `_launch`, and each `[Desktop Action NAME]` with `X-KDE-Shortcuts` is an action `NAME`.
//! A press starts the app or the action whether it runs or not: over `org.freedesktop.Application`
//! for a `DBusActivatable` app, else through its `Exec` line.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{exec, qtkey};

/// The action a desktop file's main `X-KDE-Shortcuts` starts.
pub const LAUNCH: &str = "_launch";

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceAction {
    pub friendly: String,
    /// `Exec` as arguments ([`exec::argv`]).
    pub exec: Vec<String>,
    /// Single Qt key combinations.
    pub keys: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    /// The desktop file name, `org.kde.spectacle.desktop`: the component's unique name.
    pub id: String,
    pub friendly: String,
    pub dbus_activatable: bool,
    /// [`LAUNCH`] and the desktop actions that have keys.
    pub actions: BTreeMap<String, ServiceAction>,
}

/// How to start a service action.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// `org.freedesktop.Application.Activate` (`action` `None`) or `ActivateAction`.
    DBus {
        bus: String,
        path: String,
        action: Option<String>,
    },
    Exec(Vec<String>),
}

/// The directories kglobalacceld reads: `$XDG_DATA_HOME` then `$XDG_DATA_DIRS`, each with
/// `kglobalaccel` appended; earlier ones win.
pub fn dirs() -> Vec<PathBuf> {
    dirs_from(
        std::env::var_os("HOME"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("XDG_DATA_DIRS"),
    )
}

/// [`dirs`] from the values of `HOME`, `XDG_DATA_HOME` and `XDG_DATA_DIRS`. As the XDG Base
/// Directory spec says, an empty value is an unset one and a relative path is ignored.
fn dirs_from(
    home: Option<OsString>,
    data_home: Option<OsString>,
    data_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let absolute = |path: PathBuf| path.is_absolute().then_some(path);
    let data_home = data_home.map(PathBuf::from).and_then(absolute).or_else(|| {
        home.map(PathBuf::from)
            .and_then(absolute)
            .map(|h| h.join(".local/share"))
    });
    let mut data_dirs: Vec<PathBuf> = data_dirs
        .map(|d| std::env::split_paths(&d).filter_map(absolute).collect())
        .unwrap_or_default();
    if data_dirs.is_empty() {
        data_dirs = vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ];
    }
    data_home
        .into_iter()
        .chain(data_dirs)
        .map(|d| d.join("kglobalaccel"))
        .collect()
}

/// Every service in `dirs`. The first file of each desktop-file id is the one that counts, even
/// when it is no service: a `Hidden=true` file (or one without keys) hides the same id in later
/// directories.
pub fn scan(dirs: &[PathBuf]) -> Vec<Service> {
    let mut seen = BTreeSet::new();
    let mut found: BTreeMap<String, Service> = BTreeMap::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for path in files {
            let Some(id) = desktop_id(&path) else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(service) = fs::read_to_string(&path).ok().and_then(|t| parse(&id, &t)) {
                found.insert(id, service);
            }
        }
    }
    found.into_values().collect()
}

/// The desktop-file id of a file in a `kglobalaccel` directory, which kglobalacceld reads without
/// subdirectories: its name.
fn desktop_id(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    name.ends_with(".desktop").then(|| name.to_owned())
}

/// A desktop file's service, `None` when no group has keys or the entry is hidden.
pub fn parse(id: &str, text: &str) -> Option<Service> {
    let groups = groups(text);
    let entry = groups.get("Desktop Entry")?;
    if entry.get("Hidden").is_some_and(|v| v == "true") {
        return None;
    }
    let friendly = entry.get("Name").cloned().unwrap_or_else(|| id.to_owned());
    let icon = entry.get("Icon").map(|i| exec::unescape(i));
    let name = entry.get("Name").map(|n| exec::unescape(n));
    let codes = exec::Fields {
        icon: icon.as_deref(),
        name: name.as_deref(),
        location: None,
    };
    let argv = |group: &BTreeMap<String, String>| {
        group
            .get("Exec")
            .map(|e| exec::argv(e, &codes))
            .unwrap_or_default()
    };
    let exec = argv(entry);
    let mut actions = BTreeMap::new();
    if let Some(keys) = entry.get("X-KDE-Shortcuts").map(|k| parse_keys(k))
        && !keys.is_empty()
    {
        actions.insert(
            LAUNCH.to_owned(),
            ServiceAction {
                friendly: format!("Launch {friendly}"),
                exec: exec.clone(),
                keys,
            },
        );
    }
    for (group, fields) in &groups {
        let Some(name) = group.strip_prefix("Desktop Action ") else {
            continue;
        };
        let Some(keys) = fields.get("X-KDE-Shortcuts").map(|k| parse_keys(k)) else {
            continue;
        };
        if keys.is_empty() {
            continue;
        }
        actions.insert(
            name.to_owned(),
            ServiceAction {
                friendly: fields
                    .get("Name")
                    .cloned()
                    .unwrap_or_else(|| name.to_owned()),
                exec: argv(fields),
                keys,
            },
        );
    }
    if actions.is_empty() {
        return None;
    }
    Some(Service {
        id: id.to_owned(),
        friendly,
        dbus_activatable: entry.get("DBusActivatable").is_some_and(|v| v == "true"),
        actions,
    })
}

impl Service {
    /// How to start `action`; `None` for an action the file does not have.
    pub fn launch(&self, action: &str) -> Option<Launch> {
        let a = self.actions.get(action)?;
        if self.dbus_activatable {
            let bus = self.id.trim_end_matches(".desktop").to_owned();
            let path = format!("/{}", bus.replace('.', "/").replace('-', "_"));
            return Some(Launch::DBus {
                bus,
                path,
                action: (action != LAUNCH).then(|| action.to_owned()),
            });
        }
        (!a.exec.is_empty()).then(|| Launch::Exec(a.exec.clone()))
    }
}

/// `[group]` -> unlocalized `key=value`.
fn groups(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(name.to_owned());
            groups.entry(name.to_owned()).or_default();
            continue;
        }
        let (Some(group), Some((key, value))) = (&current, line.split_once('=')) else {
            continue;
        };
        let key = key.trim();
        if key.contains('[') {
            continue;
        }
        groups
            .entry(group.clone())
            .or_default()
            .entry(key.to_owned())
            .or_insert_with(|| value.trim().to_owned());
    }
    groups
}

/// `X-KDE-Shortcuts=Print,Meta+Shift+S` as single Qt key combinations; a key it cannot read is
/// left out.
pub fn parse_keys(value: &str) -> Vec<i32> {
    value
        .split(',')
        .filter_map(|k| parse_key(k.trim()))
        .collect()
}

/// One `QKeySequence::PortableText` combination: modifiers then a key name.
pub fn parse_key(text: &str) -> Option<i32> {
    if text.is_empty() {
        return None;
    }
    // "Meta++" names the plus key: split on '+' but keep a trailing lone '+'.
    let (mods, key) = match text.strip_suffix("++") {
        Some(rest) => (rest, "+"),
        None => match text.rsplit_once('+') {
            Some((mods, key)) => (mods, key),
            None => ("", text),
        },
    };
    let mut combined = qt_key(key)?;
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        combined |= match m.to_ascii_lowercase().as_str() {
            "meta" => qtkey::QT_META,
            "ctrl" => qtkey::QT_CTRL,
            "alt" => qtkey::QT_ALT,
            "shift" => qtkey::QT_SHIFT,
            _ => return None,
        };
    }
    Some(combined)
}

fn qt_key(name: &str) -> Option<i32> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return c.is_ascii_graphic().then(|| c.to_ascii_uppercase() as i32);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<i32>().ok())
        && (1..=35).contains(&n)
    {
        return Some(0x0100_0030 + n - 1);
    }
    Some(match name {
        "Space" => 0x20,
        "Esc" | "Escape" => 0x0100_0000,
        "Tab" => 0x0100_0001,
        "Backspace" => 0x0100_0003,
        "Return" => 0x0100_0004,
        "Enter" => 0x0100_0005,
        "Ins" | "Insert" => 0x0100_0006,
        "Del" | "Delete" => 0x0100_0007,
        "Pause" => 0x0100_0008,
        "Print" => 0x0100_0009,
        "SysReq" => 0x0100_000a,
        "Home" => 0x0100_0010,
        "End" => 0x0100_0011,
        "Left" => 0x0100_0012,
        "Up" => 0x0100_0013,
        "Right" => 0x0100_0014,
        "Down" => 0x0100_0015,
        "PgUp" | "Page Up" => 0x0100_0016,
        "PgDown" | "Page Down" => 0x0100_0017,
        "Menu" => 0x0100_0055,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPECTACLE: &str = "[Desktop Entry]
Name=Spectacle
Name[de]=Spectacle
Exec=/bin/spectacle
DBusActivatable=true
X-KDE-Shortcuts=Print,Meta+Shift+S

[Desktop Action FullScreenScreenShot]
Name=Capture Entire Desktop
Exec=/bin/spectacle -f
X-KDE-Shortcuts=Shift+Print

[Desktop Action CurrentMonitorScreenShot]
Name=Capture Current Monitor
Exec=/bin/spectacle -m
";

    #[test]
    fn spectacle_reads_as_kglobalacceld_serves_it() {
        let s = parse("org.kde.spectacle.desktop", SPECTACLE).unwrap();
        assert_eq!(s.friendly, "Spectacle");
        assert!(s.dbus_activatable);
        let keys: Vec<(&str, &[i32])> = s
            .actions
            .iter()
            .map(|(k, a)| (k.as_str(), a.keys.as_slice()))
            .collect();
        assert_eq!(
            keys,
            [
                ("FullScreenScreenShot", &[0x0200_0000 | 0x0100_0009][..]),
                (LAUNCH, &[0x0100_0009, 0x1000_0000 | 0x0200_0000 | 0x53][..]),
            ]
        );
        assert_eq!(
            s.launch("FullScreenScreenShot"),
            Some(Launch::DBus {
                bus: "org.kde.spectacle".into(),
                path: "/org/kde/spectacle".into(),
                action: Some("FullScreenScreenShot".into()),
            })
        );
        assert_eq!(
            s.launch(LAUNCH),
            Some(Launch::DBus {
                bus: "org.kde.spectacle".into(),
                path: "/org/kde/spectacle".into(),
                action: None,
            })
        );
        assert_eq!(s.launch("CurrentMonitorScreenShot"), None);
    }

    #[test]
    fn a_plain_app_runs_its_exec_line_without_field_codes() {
        let text = "[Desktop Entry]\nName=Term\nExec=term --title \"a b\" %U\nX-KDE-Shortcuts=Ctrl+Alt+T\n";
        let s = parse("term.desktop", text).unwrap();
        assert_eq!(
            s.launch(LAUNCH),
            Some(Launch::Exec(vec![
                "term".into(),
                "--title".into(),
                "a b".into()
            ]))
        );
    }

    #[test]
    fn exec_lines_expand_the_entrys_icon_and_name() {
        let text =
            "[Desktop Entry]\nName=My\\sTerm\nIcon=term\nExec=term %i --class=%c --file=%f\n\
                    X-KDE-Shortcuts=Ctrl+Alt+T\n\n[Desktop Action new]\nName=New\n\
                    Exec=term --new %c\nX-KDE-Shortcuts=Ctrl+Alt+N\n";
        let s = parse("term.desktop", text).unwrap();
        let exec = |action: &str| s.actions[action].exec.clone();
        assert_eq!(
            exec(LAUNCH),
            ["term", "--icon", "term", "--class=My Term", "--file="]
        );
        assert_eq!(exec("new"), ["term", "--new", "My Term"]);
    }

    #[test]
    fn a_hidden_file_hides_the_same_id_in_later_dirs() {
        let root = std::env::temp_dir().join(format!("newm-kde-scan-{}", std::process::id()));
        let (user, system) = (root.join("user"), root.join("system"));
        fs::create_dir_all(&user).unwrap();
        fs::create_dir_all(&system).unwrap();
        let keys = "[Desktop Entry]\nName=A\nExec=a\nX-KDE-Shortcuts=Meta+A\n";
        fs::write(user.join("a.desktop"), "[Desktop Entry]\nHidden=true\n").unwrap();
        fs::write(system.join("a.desktop"), keys).unwrap();
        fs::write(system.join("b.desktop"), keys.replace("Name=A", "Name=B")).unwrap();
        let ids: Vec<String> = scan(&[user, system]).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["b.desktop"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_or_relative_xdg_values_count_as_unset() {
        let os = |s: &str| Some(OsString::from(s));
        let defaults = [
            "/home/u/.local/share/kglobalaccel",
            "/usr/local/share/kglobalaccel",
            "/usr/share/kglobalaccel",
        ]
        .map(PathBuf::from);
        assert_eq!(dirs_from(os("/home/u"), os(""), os("")), defaults);
        assert_eq!(dirs_from(os("/home/u"), None, None), defaults);
        assert_eq!(
            dirs_from(os("/home/u"), os("rel"), os("rel:also")),
            defaults
        );
        assert_eq!(
            dirs_from(os("/home/u"), os("/data"), os("/a::rel:/b")),
            ["/data/kglobalaccel", "/a/kglobalaccel", "/b/kglobalaccel"].map(PathBuf::from)
        );
        assert_eq!(
            dirs_from(None, None, os("/a")),
            [PathBuf::from("/a/kglobalaccel")]
        );
    }

    #[test]
    fn keys_parse_like_portable_text() {
        assert_eq!(parse_key("Meta++"), Some(0x1000_0000 | 0x2b));
        assert_eq!(
            parse_key("Ctrl+Alt+F12"),
            Some(0x0400_0000 | 0x0800_0000 | 0x0100_003b)
        );
        assert_eq!(parse_key("Meta+."), Some(0x1000_0000 | 0x2e));
        assert_eq!(parse_key("Hyper+X"), None);
        assert_eq!(
            parse_keys("Print, Launch (5), Meta+E"),
            vec![0x0100_0009, 0x1000_0045]
        );
    }

    #[test]
    fn hidden_files_and_files_without_keys_are_not_services() {
        assert_eq!(
            parse("a.desktop", "[Desktop Entry]\nName=A\nExec=a\n"),
            None
        );
        assert_eq!(
            parse(
                "b.desktop",
                "[Desktop Entry]\nName=B\nHidden=true\nX-KDE-Shortcuts=Meta+B\n"
            ),
            None
        );
    }
}
