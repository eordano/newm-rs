//! The registry behind `org.kde.kglobalaccel`: components (an app, or the KDE portal's session
//! for one), their actions and the keys each holds, as kglobalacceld keeps them inside KWin.
//!
//! The rules follow kglobalacceld (Plasma 6): `doRegister` creates a fresh action; a
//! `setShortcutKeys` with `IS_DEFAULT` sets its default keys; an autoloading one hands back the
//! saved keys of an action that is not fresh (it came from the saved file) and otherwise takes
//! the keys asked, each key some other action holds becoming empty. On top of that the
//! compositor's own key bindings are reserved: an app never gets one of them.
//!
//! An action id is the four strings KGlobalAccel sends: component unique name, action unique
//! name, component friendly name, action friendly name. A key is a `QKeySequence` as up to four
//! `QKeyCombination`s (`(ai)` on the bus, padded with zeros).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::service::{Launch, Service};

/// One key sequence: up to four key combinations, the unused ones 0.
pub type Seq = [i32; 4];
pub const EMPTY: Seq = [0; 4];

/// `setShortcutKeys` flags (KGlobalAccel's `SetShortcutFlag`).
pub const SET_PRESENT: u32 = 2;
pub const NO_AUTOLOADING: u32 = 4;
pub const IS_DEFAULT: u32 = 8;

/// The only shortcut context newm keeps (KDE's contexts are a KWin-scripting leftover).
pub const DEFAULT_CONTEXT: &str = "default";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub friendly: String,
    pub keys: Vec<Seq>,
    #[serde(default)]
    pub default_keys: Vec<Seq>,
    /// An app that runs registered it (`doRegister` + a `SET_PRESENT` set): its keys are bound.
    #[serde(skip)]
    pub present: bool,
    /// Created by `doRegister` this session, never set: an autoloading set takes the keys asked.
    #[serde(skip)]
    pub fresh: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub friendly: String,
    pub actions: BTreeMap<String, Action>,
}

/// A bound key: what the compositor matches key presses against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// One Qt key combination ([`crate::qtkey::from_qt`] reads it).
    pub key: i32,
    pub component: String,
    pub action: String,
}

/// The keys to bind as of one registry state, numbered in the order the states were read: a
/// consumer that gets snapshots out of order applies only one newer than the last it applied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub generation: u64,
    pub bindings: Vec<Binding>,
}

/// `KGlobalShortcutInfo` as `(ssssssaiai)`: contexts, component, action, keys, default keys
/// (the first combination of each sequence).
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub context_unique: String,
    pub context_friendly: String,
    pub component_unique: String,
    pub component_friendly: String,
    pub unique: String,
    pub friendly: String,
    pub keys: Vec<i32>,
    pub default_keys: Vec<i32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    components: BTreeMap<String, Component>,
}

#[derive(Debug, Default)]
pub struct Registry {
    components: BTreeMap<String, Component>,
    /// Keys (single Qt combinations) the compositor's own bindings hold.
    reserved: BTreeSet<i32>,
    /// `blockGlobalShortcuts(true)`: the shortcuts KCM records a key, nothing fires.
    blocked: bool,
    path: Option<PathBuf>,
    dirty: bool,
    /// The number of the last [`Snapshot`] taken.
    generation: u64,
    /// Desktop-file components ([`crate::service`]) by unique name: bound whether or not their
    /// app runs, and started rather than signalled.
    services: BTreeMap<String, Service>,
}

/// A key sequence of one combination.
pub fn single(seq: &Seq) -> Option<i32> {
    (seq[0] != 0 && seq[1..].iter().all(|&k| k == 0)).then_some(seq[0])
}

/// A sequence from the `ai` the deprecated calls carry: one combination each.
pub fn seqs_from_keys(keys: &[i32]) -> Vec<Seq> {
    keys.iter().map(|&k| [k, 0, 0, 0]).collect()
}

/// The first combination of each sequence, as the deprecated calls and `KGlobalShortcutInfo`
/// carry keys.
pub fn keys_from_seqs(seqs: &[Seq]) -> Vec<i32> {
    seqs.iter().map(|seq| seq[0]).collect()
}

/// A component's object path: `/component/` and its unique name with every character that is
/// not an ASCII letter or digit turned into `_`, as kglobalacceld names it.
pub fn component_path(unique: &str) -> String {
    let name: String = unique
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("/component/{name}")
}

impl Registry {
    /// The registry saved at `path`; a missing file is an empty registry, a damaged one too, with
    /// the error to log.
    pub fn load(path: PathBuf) -> (Self, Option<String>) {
        let (components, error) = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<File>(&text) {
                Ok(file) => (file.components, None),
                Err(err) => (BTreeMap::new(), Some(format!("{path:?}: {err}"))),
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => (BTreeMap::new(), None),
            Err(err) => (BTreeMap::new(), Some(format!("{path:?}: {err}"))),
        };
        let registry = Self {
            components,
            path: Some(path),
            ..Self::default()
        };
        (registry, error)
    }

    /// Saves the registry when a call changed it (0600, through a rename).
    pub fn flush(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = &self.path else {
            self.dirty = false;
            return Ok(());
        };
        let file = File {
            components: self.components.clone(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
        write_private(path, &text)?;
        self.dirty = false;
        Ok(())
    }

    /// The compositor's own bindings changed: their keys are reserved.
    pub fn set_reserved(&mut self, keys: impl IntoIterator<Item = i32>) {
        self.reserved = keys.into_iter().collect();
    }

    pub fn component(&self, unique: &str) -> Option<&Component> {
        self.components.get(unique)
    }

    pub fn components(&self) -> impl Iterator<Item = (&String, &Component)> {
        self.components.iter()
    }

    fn action(&self, id: &[String]) -> Option<&Action> {
        let [component, action, ..] = id else {
            return None;
        };
        self.components.get(component)?.actions.get(action)
    }

    fn action_mut(&mut self, id: &[String]) -> Option<&mut Action> {
        let [component, action, ..] = id else {
            return None;
        };
        self.components.get_mut(component)?.actions.get_mut(action)
    }

    /// `doRegister`: creates the action (fresh, not present) or renews its friendly names.
    pub fn do_register(&mut self, id: &[String]) {
        let [component, action, component_friendly, action_friendly, ..] = id else {
            return;
        };
        if component.is_empty() || action.is_empty() {
            return;
        }
        let c = self.components.entry(component.clone()).or_insert_with(|| {
            self.dirty = true;
            Component {
                friendly: if component_friendly.is_empty() {
                    component.clone()
                } else {
                    component_friendly.clone()
                },
                actions: BTreeMap::new(),
            }
        });
        if !component_friendly.is_empty() && c.friendly != *component_friendly {
            c.friendly = component_friendly.clone();
            self.dirty = true;
        }
        match c.actions.get_mut(action) {
            Some(a) => {
                if !action_friendly.is_empty() && a.friendly != *action_friendly {
                    a.friendly = action_friendly.clone();
                    self.dirty = true;
                }
            }
            None => {
                c.actions.insert(
                    action.clone(),
                    Action {
                        friendly: action_friendly.clone(),
                        fresh: true,
                        ..Action::default()
                    },
                );
                self.dirty = true;
            }
        }
    }

    /// Whether `seq` is free for the action `id`: no compositor binding and no other action
    /// holds it.
    fn free_for(&self, seq: &Seq, id: &[String]) -> bool {
        if single(seq).is_some_and(|k| self.reserved.contains(&k)) {
            return false;
        }
        !self.held_by_other(seq, id)
    }

    /// Whether an action other than `id` holds `seq`.
    fn held_by_other(&self, seq: &Seq, id: &[String]) -> bool {
        self.components.iter().any(|(cu, c)| {
            c.actions.iter().any(|(au, a)| {
                let same = id.first() == Some(cu) && id.get(1) == Some(au);
                !same && a.keys.contains(seq)
            })
        })
    }

    /// `setShortcutKeys`: answers the keys the action holds afterwards.
    pub fn set_shortcut_keys(&mut self, id: &[String], keys: &[Seq], flags: u32) -> Vec<Seq> {
        let Some(action) = self.action(id) else {
            return Vec::new();
        };
        if flags & IS_DEFAULT != 0 {
            if action.default_keys != keys {
                self.action_mut(id).unwrap().default_keys = keys.to_vec();
                self.dirty = true;
            }
            return keys.to_vec();
        }
        if flags & NO_AUTOLOADING == 0 && !action.fresh {
            let action = self.action_mut(id).unwrap();
            if flags & SET_PRESENT != 0 {
                action.present = true;
            }
            return action.keys.clone();
        }
        let taken: Vec<Seq> = keys
            .iter()
            .map(|seq| {
                if *seq == EMPTY || self.free_for(seq, id) {
                    *seq
                } else {
                    EMPTY
                }
            })
            .collect();
        let action = self.action_mut(id).unwrap();
        let changed = action.keys != taken;
        action.keys = taken;
        if flags & SET_PRESENT != 0 {
            action.present = true;
        }
        action.fresh = false;
        let keys = action.keys.clone();
        self.dirty |= changed;
        keys
    }

    /// `setForeignShortcutKeys`: another program (the shortcuts KCM) sets an action's keys. The
    /// answer is what the owning app gets in `yourShortcutsChanged`; `None` for no such action.
    pub fn set_foreign_shortcut_keys(&mut self, id: &[String], keys: &[Seq]) -> Option<Vec<Seq>> {
        self.action(id)?;
        Some(self.set_shortcut_keys(id, keys, NO_AUTOLOADING))
    }

    pub fn shortcut_keys(&self, id: &[String]) -> Vec<Seq> {
        self.action(id).map(|a| a.keys.clone()).unwrap_or_default()
    }

    pub fn default_shortcut_keys(&self, id: &[String]) -> Vec<Seq> {
        self.action(id)
            .map(|a| a.default_keys.clone())
            .unwrap_or_default()
    }

    /// `setInactive`: the app is going away; its keys stay saved but are not bound.
    pub fn set_inactive(&mut self, id: &[String]) {
        if let Some(action) = self.action_mut(id) {
            action.present = false;
        }
    }

    /// `unregister`: forgets the action (and its component once empty).
    pub fn unregister(&mut self, component: &str, action: &str) -> bool {
        let Some(c) = self.components.get_mut(component) else {
            return false;
        };
        let removed = c.actions.remove(action).is_some();
        if c.actions.is_empty() {
            self.components.remove(component);
        }
        self.dirty |= removed;
        removed
    }

    /// `getComponent`: the component's object path.
    pub fn component_path(&self, unique: &str) -> Result<String, String> {
        if self.components.contains_key(unique) {
            Ok(component_path(unique))
        } else {
            Err(format!("The component '{unique}' doesn't exist."))
        }
    }

    pub fn all_component_paths(&self) -> Vec<String> {
        self.components.keys().map(|u| component_path(u)).collect()
    }

    /// `allMainComponents`: each component as an action id with empty action names.
    pub fn all_main_components(&self) -> Vec<Vec<String>> {
        self.components
            .iter()
            .map(|(u, c)| vec![u.clone(), c.friendly.clone(), String::new(), String::new()])
            .collect()
    }

    /// `allActionsForComponent`: the full action ids of the component `id` names.
    pub fn all_actions_for_component(&self, id: &[String]) -> Vec<Vec<String>> {
        let Some((cu, c)) = id.first().and_then(|u| self.components.get_key_value(u)) else {
            return Vec::new();
        };
        c.actions
            .iter()
            .map(|(au, a)| {
                vec![
                    cu.clone(),
                    au.clone(),
                    c.friendly.clone(),
                    a.friendly.clone(),
                ]
            })
            .collect()
    }

    fn holders(&self, seq: &Seq) -> impl Iterator<Item = (&String, &Component, &String, &Action)> {
        self.components.iter().flat_map(move |(cu, c)| {
            c.actions
                .iter()
                .filter(move |(_, a)| a.keys.contains(seq))
                .map(move |(au, a)| (cu, c, au, a))
        })
    }

    /// `actionList` / `action`: the action id holding a key, or nothing.
    pub fn action_for_key(&self, seq: &Seq) -> Vec<String> {
        self.holders(seq)
            .next()
            .map(|(cu, c, au, a)| {
                vec![
                    cu.clone(),
                    au.clone(),
                    c.friendly.clone(),
                    a.friendly.clone(),
                ]
            })
            .unwrap_or_default()
    }

    /// `globalShortcutsByKey`: who holds a key.
    pub fn infos_for_key(&self, seq: &Seq) -> Vec<Info> {
        self.holders(seq)
            .map(|(cu, c, au, a)| info(cu, c, au, a))
            .collect()
    }

    /// `globalShortcutAvailable`: whether `component` could take a key.
    pub fn is_available(&self, seq: &Seq, component: &str) -> bool {
        if single(seq).is_some_and(|k| self.reserved.contains(&k)) {
            return false;
        }
        self.holders(seq).all(|(cu, ..)| cu == component)
    }

    pub fn block(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    /// `Component.shortcutNames`.
    pub fn shortcut_names(&self, component: &str) -> Vec<String> {
        self.components
            .get(component)
            .map(|c| c.actions.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// `Component.allShortcutInfos`.
    pub fn infos(&self, component: &str) -> Vec<Info> {
        let Some((cu, c)) = self.components.get_key_value(component) else {
            return Vec::new();
        };
        c.actions.iter().map(|(au, a)| info(cu, c, au, a)).collect()
    }

    /// `Component.cleanUp`: forgets the actions no running app registered; answers whether any
    /// went.
    pub fn clean_up(&mut self, component: &str) -> bool {
        let Some(c) = self.components.get_mut(component) else {
            return false;
        };
        let before = c.actions.len();
        let service = self.services.get(component);
        c.actions
            .retain(|au, a| a.present || service.is_some_and(|s| s.actions.contains_key(au)));
        let changed = c.actions.len() != before;
        if c.actions.is_empty() {
            self.components.remove(component);
        }
        self.dirty |= changed;
        changed
    }

    /// `Component.isActive`: some action of it is registered by a running app, or it is a
    /// desktop-file component, which a key starts.
    pub fn is_active(&self, component: &str) -> bool {
        self.services.contains_key(component)
            || self
                .components
                .get(component)
                .is_some_and(|c| c.actions.values().any(|a| a.present))
    }

    /// Adds the desktop-file components: an action not saved yet takes the file's keys, each key
    /// another action holds becoming empty (as kglobalacceld's `GlobalShortcut::setKeys` does); a
    /// saved one keeps its keys; both take the file's keys as their defaults.
    pub fn add_services(&mut self, services: Vec<Service>) {
        for s in services {
            for (name, sa) in &s.actions {
                let defaults = seqs_from_keys(&sa.keys);
                let id = [s.id.clone(), name.clone()];
                let keys: Vec<Seq> = if self.action(&id).is_none() {
                    defaults
                        .iter()
                        .map(|seq| {
                            if self.held_by_other(seq, &id) {
                                EMPTY
                            } else {
                                *seq
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let c = self
                    .components
                    .entry(s.id.clone())
                    .or_insert_with(|| Component {
                        friendly: s.friendly.clone(),
                        actions: BTreeMap::new(),
                    });
                let a = c.actions.entry(name.clone()).or_insert_with(|| Action {
                    friendly: sa.friendly.clone(),
                    keys,
                    ..Action::default()
                });
                a.default_keys = defaults;
            }
            self.services.insert(s.id.clone(), s);
        }
    }

    /// How to start a desktop-file component's action; `None` for an app's own action, which is
    /// signalled instead.
    pub fn service_launch(&self, component: &str, action: &str) -> Option<Launch> {
        self.services.get(component)?.launch(action)
    }

    pub fn has_action(&self, component: &str, action: &str) -> bool {
        self.components
            .get(component)
            .is_some_and(|c| c.actions.contains_key(action))
    }

    /// The keys to bind: one-combination keys of present actions, none of them reserved, nothing
    /// while blocked.
    pub fn bindings(&self) -> Vec<Binding> {
        if self.blocked {
            return Vec::new();
        }
        let mut bindings = Vec::new();
        for (cu, c) in &self.components {
            let service = self.services.get(cu);
            for (au, a) in &c.actions {
                if !a.present && !service.is_some_and(|s| s.actions.contains_key(au)) {
                    continue;
                }
                for key in a.keys.iter().filter_map(single) {
                    if !self.reserved.contains(&key) {
                        bindings.push(Binding {
                            key,
                            component: cu.clone(),
                            action: au.clone(),
                        });
                    }
                }
            }
        }
        bindings
    }

    /// [`Self::bindings`] under the next generation number.
    pub fn snapshot(&mut self) -> Snapshot {
        self.generation += 1;
        Snapshot {
            generation: self.generation,
            bindings: self.bindings(),
        }
    }
}

fn info(cu: &str, c: &Component, au: &str, a: &Action) -> Info {
    Info {
        context_unique: DEFAULT_CONTEXT.to_owned(),
        context_friendly: String::from("Default Context"),
        component_unique: cu.to_owned(),
        component_friendly: c.friendly.clone(),
        unique: au.to_owned(),
        friendly: a.friendly.clone(),
        keys: keys_from_seqs(&a.keys),
        default_keys: keys_from_seqs(&a.default_keys),
    }
}

fn write_private(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINT: i32 = 0x0100_0009;
    const META: i32 = 0x1000_0000;
    const KEY_S: i32 = 0x53;

    fn id(component: &str, action: &str) -> Vec<String> {
        vec![
            component.to_owned(),
            action.to_owned(),
            format!("{component} app"),
            format!("{action} action"),
        ]
    }

    fn seq(key: i32) -> Seq {
        [key, 0, 0, 0]
    }

    /// What `KGlobalAccel::setGlobalShortcut` sends: the default, then an autoloading set.
    fn set_global(r: &mut Registry, id: &[String], key: i32) -> Vec<Seq> {
        r.do_register(id);
        r.set_shortcut_keys(id, &[seq(key)], IS_DEFAULT);
        r.set_shortcut_keys(id, &[seq(key)], SET_PRESENT)
    }

    #[test]
    fn an_app_registering_its_default_gets_it_bound() {
        let mut r = Registry::default();
        let spectacle = id("org.kde.spectacle.desktop", "RectangularRegionScreenShot");
        assert_eq!(
            set_global(&mut r, &spectacle, META | KEY_S),
            vec![seq(META | KEY_S)]
        );
        assert_eq!(r.default_shortcut_keys(&spectacle), vec![seq(META | KEY_S)]);
        assert_eq!(
            r.bindings(),
            vec![Binding {
                key: META | KEY_S,
                component: spectacle[0].clone(),
                action: spectacle[1].clone(),
            }]
        );
        assert_eq!(
            r.component_path(&spectacle[0]).unwrap(),
            "/component/org_kde_spectacle_desktop"
        );
    }

    #[test]
    fn a_compositor_binding_wins_and_the_app_is_told_unassigned() {
        let mut r = Registry::default();
        r.set_reserved([META | KEY_S]);
        let a = id("kate", "find");
        assert_eq!(set_global(&mut r, &a, META | KEY_S), vec![EMPTY]);
        assert!(r.bindings().is_empty());
        assert!(!r.is_available(&seq(META | KEY_S), "kate"));
        assert!(r.is_available(&seq(PRINT), "kate"));
    }

    #[test]
    fn a_key_another_app_holds_is_refused() {
        let mut r = Registry::default();
        set_global(&mut r, &id("spectacle", "full"), PRINT);
        assert_eq!(
            set_global(&mut r, &id("flameshot", "gui"), PRINT),
            vec![EMPTY]
        );
        assert_eq!(r.infos_for_key(&seq(PRINT)).len(), 1);
        assert_eq!(r.action_for_key(&seq(PRINT))[0], "spectacle");
        // The holder setting its own key again keeps it.
        let full = id("spectacle", "full");
        assert_eq!(
            r.set_shortcut_keys(&full, &[seq(PRINT)], NO_AUTOLOADING),
            vec![seq(PRINT)]
        );
    }

    #[test]
    fn saved_keys_come_back_and_beat_the_apps_default() {
        let dir = std::env::temp_dir().join(format!("newm-kde-{}", std::process::id()));
        let path = dir.join("kglobalshortcuts.json");
        let a = id("spectacle", "full");
        {
            let (mut r, err) = Registry::load(path.clone());
            assert!(err.is_none());
            set_global(&mut r, &a, PRINT);
            // The user moves it in the shortcuts KCM.
            assert_eq!(
                r.set_foreign_shortcut_keys(&a, &[seq(META | PRINT)]),
                Some(vec![seq(META | PRINT)])
            );
            r.flush().unwrap();
        }
        let (mut r, err) = Registry::load(path.clone());
        assert!(err.is_none());
        assert!(
            r.bindings().is_empty(),
            "nothing is bound before the app registers"
        );
        assert_eq!(set_global(&mut r, &a, PRINT), vec![seq(META | PRINT)]);
        assert_eq!(r.default_shortcut_keys(&a), vec![seq(PRINT)]);
        assert_eq!(r.bindings()[0].key, META | PRINT);
        let mode =
            std::os::unix::fs::PermissionsExt::mode(&fs::metadata(&path).unwrap().permissions());
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_autoloading_set_binds_a_saved_action_only_with_set_present() {
        let mut r = Registry::default();
        let a = id("app", "one");
        set_global(&mut r, &a, PRINT);
        r.set_inactive(&a);
        // The shortcuts KCM reads the saved keys back: autoloading, no SET_PRESENT.
        assert_eq!(r.set_shortcut_keys(&a, &[seq(PRINT)], 0), vec![seq(PRINT)]);
        assert!(
            r.bindings().is_empty(),
            "a set without SET_PRESENT binds nothing"
        );
        assert!(!r.is_active("app"));
        assert_eq!(
            r.set_shortcut_keys(&a, &[seq(PRINT)], SET_PRESENT),
            vec![seq(PRINT)]
        );
        assert_eq!(r.bindings()[0].key, PRINT);
    }

    #[test]
    fn a_damaged_file_starts_empty_with_an_error() {
        let dir = std::env::temp_dir().join(format!("newm-kde-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kglobalshortcuts.json");
        fs::write(&path, "{ not json").unwrap();
        let (r, err) = Registry::load(path);
        assert!(err.is_some());
        assert!(r.all_main_components().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn inactive_blocked_and_sequences_bind_nothing() {
        let mut r = Registry::default();
        let a = id("app", "one");
        set_global(&mut r, &a, PRINT);
        r.block(true);
        assert!(r.bindings().is_empty());
        r.block(false);
        r.set_inactive(&a);
        assert!(r.bindings().is_empty());
        assert!(!r.is_active("app"));
        let b = id("app", "chord");
        r.do_register(&b);
        r.set_shortcut_keys(&b, &[[META | KEY_S, KEY_S, 0, 0]], SET_PRESENT);
        assert!(
            r.bindings().is_empty(),
            "a two-key sequence is saved but not bound"
        );
        assert_eq!(r.shortcut_keys(&b), vec![[META | KEY_S, KEY_S, 0, 0]]);
    }

    #[test]
    fn snapshots_number_up_in_the_order_they_are_taken() {
        let mut r = Registry::default();
        let first = r.snapshot();
        set_global(&mut r, &id("app", "one"), PRINT);
        let second = r.snapshot();
        assert!(second.generation > first.generation);
        assert!(first.bindings.is_empty());
        assert_eq!(second.bindings, r.bindings());
    }

    #[test]
    fn listing_unregistering_and_cleaning_up() {
        let mut r = Registry::default();
        set_global(&mut r, &id("app", "one"), PRINT);
        r.do_register(&id("app", "two"));
        assert_eq!(r.shortcut_names("app"), vec!["one", "two"]);
        assert_eq!(r.all_actions_for_component(&id("app", "")).len(), 2);
        assert_eq!(r.all_main_components()[0][1], "app app");
        assert_eq!(r.infos("app")[0].keys, vec![PRINT]);
        assert!(r.clean_up("app"), "two never became present");
        assert_eq!(r.shortcut_names("app"), vec!["one"]);
        assert!(r.unregister("app", "one"));
        assert!(r.component_path("app").is_err());
        assert!(r.all_component_paths().is_empty());
    }

    #[test]
    fn friendly_names_follow_the_latest_register() {
        let mut r = Registry::default();
        r.do_register(&id("app", "one"));
        let renamed = vec![
            String::from("app"),
            String::from("one"),
            String::from("App"),
            String::from("One"),
        ];
        r.do_register(&renamed);
        assert_eq!(r.component("app").unwrap().friendly, "App");
        assert_eq!(r.infos("app")[0].friendly, "One");
        r.do_register(&[String::from("short")]);
        assert!(r.component("short").is_none());
    }

    #[test]
    fn a_desktop_file_key_another_action_holds_comes_in_empty() {
        let mut r = Registry::default();
        set_global(&mut r, &id("flameshot", "gui"), PRINT);
        let text = "[Desktop Entry]\nName=Spectacle\nExec=spectacle\nX-KDE-Shortcuts=Print\n";
        let service = crate::service::parse("org.kde.spectacle.desktop", text).unwrap();
        r.add_services(vec![service]);
        let launch = id("org.kde.spectacle.desktop", crate::service::LAUNCH);
        assert_eq!(r.shortcut_keys(&launch), vec![EMPTY]);
        assert_eq!(r.default_shortcut_keys(&launch), vec![seq(PRINT)]);
        let bound: Vec<String> = r.bindings().into_iter().map(|b| b.component).collect();
        assert_eq!(bound, ["flameshot"], "one action per key");
    }

    #[test]
    fn desktop_file_components_are_bound_without_their_app_and_survive_clean_up() {
        let text = "[Desktop Entry]\nName=Spectacle\nExec=spectacle\nDBusActivatable=true\n\
                    X-KDE-Shortcuts=Print\n\n[Desktop Action FullScreenScreenShot]\n\
                    Name=Full\nExec=spectacle -f\nX-KDE-Shortcuts=Shift+Print\n";
        let service = crate::service::parse("org.kde.spectacle.desktop", text).unwrap();
        let mut r = Registry::default();
        // The config binds Print itself: that key stays the compositor's.
        r.set_reserved([0x0100_0009]);
        r.add_services(vec![service]);
        let bound: Vec<(i32, String)> = r
            .bindings()
            .into_iter()
            .map(|b| (b.key, b.action))
            .collect();
        assert_eq!(
            bound,
            vec![(
                0x0200_0009 | 0x0100_0000,
                String::from("FullScreenScreenShot")
            )]
        );
        assert!(r.is_active("org.kde.spectacle.desktop"));
        assert!(matches!(
            r.service_launch("org.kde.spectacle.desktop", "FullScreenScreenShot"),
            Some(crate::service::Launch::DBus { .. })
        ));
        // Spectacle quitting cleans up its component: the file's actions stay.
        r.clean_up("org.kde.spectacle.desktop");
        assert_eq!(r.shortcut_names("org.kde.spectacle.desktop").len(), 2);
        // A key the user saved wins over the file's.
        let saved = id("org.kde.spectacle.desktop", "FullScreenScreenShot");
        r.set_shortcut_keys(&saved, &[[0x1000_0046, 0, 0, 0]], NO_AUTOLOADING);
        let again = crate::service::parse("org.kde.spectacle.desktop", text).unwrap();
        r.add_services(vec![again]);
        assert_eq!(r.shortcut_keys(&saved), vec![[0x1000_0046, 0, 0, 0]]);
        assert_eq!(
            r.default_shortcut_keys(&saved),
            vec![[0x0300_0009, 0, 0, 0]]
        );
    }
}
