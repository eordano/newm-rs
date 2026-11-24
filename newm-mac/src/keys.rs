//! The keymap newm-mac reads: `[{ combo, linux, action }]`, generated from the bind list of
//! newm-rs's `config.kdl`
//! from the same bind list as newm-rs's `config.kdl`. Combos use KDL key names, mapped to macOS
//! virtual keycodes by physical position on an ANSI keyboard, so the same fingers press the same
//! keys. `Mod` is the left Option key only; a combo with `Alt` (the Mac's Command position) or
//! without `Mod` is native and never taken.

use std::collections::HashMap;

use serde_json::Value;

/// CGEventFlags bits, for the event tap, `inject` and [`shortcut`] (macOS and tests only).
#[cfg(any(target_os = "macos", test))]
pub const LEFT_OPT: u64 = 0x20;
#[cfg(target_os = "macos")]
pub const RIGHT_OPT: u64 = 0x40;
#[cfg(any(target_os = "macos", test))]
pub const LEFT_CTRL_DEV: u64 = 0x01;
#[cfg(any(target_os = "macos", test))]
pub const LEFT_SHIFT_DEV: u64 = 0x02;
#[cfg(any(target_os = "macos", test))]
pub const LEFT_CMD_DEV: u64 = 0x08;
#[cfg(any(target_os = "macos", test))]
pub const SHIFT: u64 = 0x20000;
#[cfg(any(target_os = "macos", test))]
pub const CTRL: u64 = 0x40000;
#[cfg(any(target_os = "macos", test))]
pub const ALT: u64 = 0x80000;
#[cfg(any(target_os = "macos", test))]
pub const CMD: u64 = 0x100000;

/// The ANSI virtual keycode of a KDL (xkb) key name.
pub fn keycode(name: &str) -> Option<u16> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "equal" => 24,
        "9" => 25,
        "7" => 26,
        "minus" => 27,
        "8" => 28,
        "0" => 29,
        "bracketright" => 30,
        "o" => 31,
        "u" => 32,
        "bracketleft" => 33,
        "i" => 34,
        "p" => 35,
        "return" => 36,
        "l" => 37,
        "j" => 38,
        "apostrophe" => 39,
        "k" => 40,
        "semicolon" => 41,
        "backslash" => 42,
        "comma" => 43,
        "slash" => 44,
        "n" => 45,
        "m" => 46,
        "period" => 47,
        "tab" => 48,
        "space" => 49,
        "grave" => 50,
        "backspace" => 51,
        "escape" => 53,
        "kp_decimal" => 65,
        "kp_multiply" => 67,
        "kp_add" => 69,
        "kp_divide" => 75,
        "kp_enter" => 76,
        "kp_subtract" => 78,
        "kp_0" => 82,
        "kp_1" => 83,
        "kp_2" => 84,
        "kp_3" => 85,
        "kp_4" => 86,
        "kp_5" => 87,
        "kp_6" => 88,
        "kp_7" => 89,
        "kp_8" => 91,
        "kp_9" => 92,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        // Print sits where an extended Apple keyboard has F13.
        "print" | "f13" => 105,
        "home" => 115,
        "prior" | "page_up" => 116,
        "delete" => 117,
        "end" => 119,
        "next" | "page_down" => 121,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        _ => return None,
    })
}

/// What fires a binding: a key, or a wheel step.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Trigger {
    Key(u16),
    WheelUp,
    WheelDown,
}

/// A newm-mac chord: left Option plus Shift/Control as given.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub trigger: Trigger,
    pub shift: bool,
    pub ctrl: bool,
}

/// Splits a KDL combo into its modifiers and key.
pub fn split(combo: &str) -> (Vec<&str>, &str) {
    let parts: Vec<&str> = combo.split('+').collect();
    let (key, mods) = parts.split_last().unwrap();
    (mods.to_vec(), key)
}

/// The chord newm-mac takes for a combo; None when the combo is native on the Mac (no Mod, or
/// Alt in it). Err for a Mod combo whose key has no Mac keycode.
pub fn chord(combo: &str) -> Result<Option<Chord>, String> {
    let (mods, key) = split(combo);
    if !mods.contains(&"Mod") || mods.contains(&"Alt") {
        return Ok(None);
    }
    let trigger = match key {
        "WheelScrollUp" => Trigger::WheelUp,
        "WheelScrollDown" => Trigger::WheelDown,
        k => Trigger::Key(keycode(k).ok_or_else(|| format!("{combo}: no Mac key for {k}"))?),
    };
    Ok(Some(Chord {
        trigger,
        shift: mods.contains(&"Shift"),
        ctrl: mods.contains(&"Ctrl"),
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A canvas action: `focus-column-left`, `resize-edge left 1`, `quit`, ...
    Wm(String),
    Spawn(String),
    /// A native shortcut newm-mac posts, after activating `app` when given.
    Send { keys: String, app: Option<String> },
    Native,
    LinuxOnly,
}

impl Action {
    pub fn from_json(v: &Value) -> Result<Action, String> {
        match v {
            Value::String(s) if s == "native" => Ok(Action::Native),
            Value::String(s) if s == "linux-only" => Ok(Action::LinuxOnly),
            Value::String(s) => Ok(Action::Wm(s.clone())),
            Value::Object(o) => {
                if let Some(Value::String(c)) = o.get("spawn") {
                    Ok(Action::Spawn(c.clone()))
                } else if let Some(Value::String(k)) = o.get("send") {
                    Ok(Action::Send {
                        keys: k.clone(),
                        app: o.get("app").and_then(Value::as_str).map(str::to_owned),
                    })
                } else {
                    Err(format!("unknown action {v}"))
                }
            }
            _ => Err(format!("unknown action {v}")),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Action::Wm(s) => s.clone(),
            Action::Spawn(c) => format!("spawn `{c}`"),
            Action::Send { keys, app: None } => format!("posts {}", mac_label(keys)),
            Action::Send { keys, app: Some(a) } => format!("{a}: posts {}", mac_label(keys)),
            Action::Native => "native".into(),
            Action::LinuxOnly => "linux-only".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub combo: String,
    pub linux: String,
    pub action: Action,
}

pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let list = v.as_array().ok_or("the keymap is not a list")?;
    list.iter()
        .map(|e| {
            Ok(Entry {
                combo: e["combo"].as_str().ok_or("an entry has no combo")?.to_owned(),
                linux: e["linux"].as_str().unwrap_or("").to_owned(),
                action: Action::from_json(&e["action"])?,
            })
        })
        .collect()
}

/// The chords newm-mac takes, each with its action. A combo with no Mac key is an error; native
/// and linux-only entries are left to macOS.
pub fn bindings(entries: &[Entry]) -> Result<HashMap<Chord, Action>, String> {
    let mut m = HashMap::new();
    for e in entries {
        if matches!(e.action, Action::Native | Action::LinuxOnly) {
            continue;
        }
        let Some(c) = chord(&e.combo)? else {
            return Err(format!("{}: bound on the Mac but not a Mod combo", e.combo));
        };
        m.entry(c).or_insert_with(|| e.action.clone());
    }
    Ok(m)
}

/// A shortcut to post (`Cmd+Shift+4`, `Ctrl+Cmd+space`): its keycode and flags.
#[cfg(any(target_os = "macos", test))]
pub fn shortcut(keys: &str) -> Result<(u16, u64), String> {
    let (mods, key) = split(keys);
    let code = keycode(key).ok_or_else(|| format!("{keys}: no Mac key for {key}"))?;
    let mut flags = 0;
    for m in mods {
        flags |= match m {
            "Cmd" => CMD | LEFT_CMD_DEV,
            "Shift" => SHIFT | LEFT_SHIFT_DEV,
            "Ctrl" => CTRL | LEFT_CTRL_DEV,
            "Opt" | "Mod" => ALT | LEFT_OPT,
            _ => return Err(format!("{keys}: unknown modifier {m}")),
        };
    }
    Ok((code, flags))
}

fn key_label(k: &str) -> String {
    match k {
        "Return" => "Return".into(),
        "space" => "Space".into(),
        "Left" => "←".into(),
        "Right" => "→".into(),
        "Up" => "↑".into(),
        "Down" => "↓".into(),
        "minus" => "-".into(),
        "equal" => "=".into(),
        "period" => ".".into(),
        "slash" => "/".into(),
        "Escape" => "Esc".into(),
        "Delete" => "Delete".into(),
        "KP_Add" => "keypad +".into(),
        "KP_Subtract" => "keypad -".into(),
        "Print" => "F13 (Print)".into(),
        "WheelScrollUp" => "wheel up".into(),
        "WheelScrollDown" => "wheel down".into(),
        k if k.starts_with("XF86") => format!("media key {}", &k[4..]),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.into(),
    }
}

/// How a combo reads on a Mac keyboard: `Mod` is left ⌥, `Alt` sits where ⌘ is.
pub fn mac_label(combo: &str) -> String {
    let (mods, key) = split(combo);
    let mut s = String::new();
    for m in ["Ctrl", "Mod", "Opt", "Alt", "Cmd", "Shift"] {
        if mods.contains(&m) {
            s.push_str(match m {
                "Ctrl" => "⌃",
                "Mod" => "left ⌥",
                "Opt" => "⌥",
                "Alt" | "Cmd" => "⌘",
                _ => "⇧",
            });
        }
    }
    if !s.is_empty() {
        s.push(' ');
    }
    s + &key_label(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_map_by_position() {
        let c = chord("Mod+Shift+h").unwrap().unwrap();
        assert_eq!(c.trigger, Trigger::Key(4));
        assert!(c.shift && !c.ctrl);
        assert_eq!(chord("Mod+Ctrl+H").unwrap().unwrap().trigger, Trigger::Key(4));
        assert_eq!(chord("Mod+KP_Add").unwrap().unwrap().trigger, Trigger::Key(69));
        assert_eq!(chord("Alt+q").unwrap(), None);
        assert_eq!(chord("Mod+Alt+r").unwrap(), None);
        assert_eq!(chord("XF86AudioMute").unwrap(), None);
        assert!(chord("Mod+XF86Nothing").is_err());
        assert_eq!(shortcut("Ctrl+Cmd+space").unwrap().0, 49);
    }

    #[test]
    fn actions_parse() {
        let e = parse(
            r#"[{"combo":"Mod+h","linux":"focus-column-left","action":"focus-column-left"},
                {"combo":"Mod+e","linux":"x","action":{"send":"Cmd+n","app":"com.apple.finder"}},
                {"combo":"Alt+q","linux":"close-window","action":"native"}]"#,
        )
        .unwrap();
        let b = bindings(&e).unwrap();
        assert_eq!(b.len(), 2);
    }
}
