//! `org.kde.kglobalaccel`: KDE apps' global shortcuts. In Plasma 6 kglobalacceld runs only inside
//! KWin, so without this nothing answers KGlobalAccel (Spectacle logs "Couldn't start
//! kglobalaccel") and the KDE GlobalShortcuts portal fails. The rules live in
//! [`newm_kde::kglobalaccel`]; this module puts them on the bus and turns the keys apps hold into
//! key bindings, matched after the config's own (which apps can never take).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use newm_kde::kglobalaccel::{self as reg, Registry, Seq};
use newm_kde::qtkey;
use newm_kde::service::Launch;
use niri_config::{Action, Bind, BoundAction, Config, Key, ModKey, Modifiers, Trigger};
use smithay::backend::input::InputTime;
use smithay::input::keyboard::Keysym;
use zbus::interface;
use zbus::object_server::{ObjectServer, SignalEmitter};
use zbus::zvariant::{OwnedObjectPath, Value};

use super::{request_name, Start};

/// Action ids of kglobalaccel bindings carry this bit (the GNOME portal's count up from 1).
pub const KDE_ACTION: u32 = 0x8000_0000;

/// Whether a `GlobalShortcut*Action` id is one of ours.
pub fn owns(action: u32) -> bool {
    action & KDE_ACTION != 0
}

pub enum ToNiri {
    /// The keys apps hold changed.
    Bindings(reg::Snapshot),
}

/// One `QKeySequence` on the bus: `(ai)`, four combinations.
type Wire = (Vec<i32>,);
/// One `KGlobalShortcutInfo`: `(ssssssaiai)`.
type WireInfo = (
    String,
    String,
    String,
    String,
    String,
    String,
    Vec<i32>,
    Vec<i32>,
);

fn seq_from_wire((keys,): &Wire) -> Seq {
    let mut seq = reg::EMPTY;
    for (slot, key) in seq.iter_mut().zip(keys) {
        *slot = *key;
    }
    seq
}

fn seqs_from_wire(keys: &[Wire]) -> Vec<Seq> {
    keys.iter().map(seq_from_wire).collect()
}

fn seqs_to_wire(seqs: Vec<Seq>) -> Vec<Wire> {
    seqs.into_iter().map(|seq| (seq.to_vec(),)).collect()
}

fn info_to_wire(i: reg::Info) -> WireInfo {
    (
        i.context_unique,
        i.context_friendly,
        i.component_unique,
        i.component_friendly,
        i.unique,
        i.friendly,
        i.keys,
        i.default_keys,
    )
}

fn object_path(path: String) -> OwnedObjectPath {
    // component_path() only ever makes valid paths.
    OwnedObjectPath::try_from(path).unwrap()
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.kde.kglobalaccel")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    NoSuchComponent(String),
}

/// The `/kglobalaccel` object.
#[derive(Clone)]
pub struct KGlobalAccel {
    registry: Arc<Mutex<Registry>>,
    /// The components with an object on the bus.
    served: Arc<Mutex<BTreeSet<String>>>,
    to_niri: calloop::channel::Sender<ToNiri>,
}

/// A `/component/<name>` object.
pub struct ComponentObject {
    unique: String,
    service: KGlobalAccel,
}

impl KGlobalAccel {
    pub fn new(registry: Arc<Mutex<Registry>>, to_niri: calloop::channel::Sender<ToNiri>) -> Self {
        Self {
            registry,
            served: Arc::new(Mutex::new(BTreeSet::new())),
            to_niri,
        }
    }

    fn read<T>(&self, f: impl FnOnce(&Registry) -> T) -> T {
        f(&self.registry.lock().unwrap())
    }

    fn write<T>(&self, f: impl FnOnce(&mut Registry) -> T) -> T {
        f(&mut self.registry.lock().unwrap())
    }

    /// After a call that may have changed the registry: save it, give every component an object
    /// and drop the objects of the components that went, and hand niri the keys to bind.
    async fn changed(&self, server: &ObjectServer) {
        let (now, snapshot) = self.write(|r| {
            if let Err(err) = r.flush() {
                warn!("error saving the global shortcuts: {err:?}");
            }
            let now: BTreeSet<String> = r.components().map(|(u, _)| u.clone()).collect();
            (now, r.snapshot())
        });
        let (added, removed) = {
            let mut served = self.served.lock().unwrap();
            let added: Vec<String> = now.difference(&served).cloned().collect();
            let removed: Vec<String> = served.difference(&now).cloned().collect();
            *served = now;
            (added, removed)
        };
        for unique in added {
            let object = ComponentObject {
                unique: unique.clone(),
                service: self.clone(),
            };
            if let Err(err) = server.at(reg::component_path(&unique), object).await {
                warn!("error serving the kglobalaccel component {unique}: {err:?}");
            }
        }
        for unique in removed {
            let path = reg::component_path(&unique);
            if let Err(err) = server.remove::<ComponentObject, _>(path).await {
                warn!("error removing the kglobalaccel component {unique}: {err:?}");
            }
        }
        // Concurrent calls reach this send in any order; niri drops a snapshot older than the
        // one it bound.
        if let Err(err) = self.to_niri.send(ToNiri::Bindings(snapshot)) {
            warn!("error sending global shortcuts to niri: {err:?}");
        }
    }

    async fn set_keys(
        &self,
        server: &ObjectServer,
        id: &[String],
        keys: &[Seq],
        flags: u32,
    ) -> Vec<Seq> {
        let keys = self.write(|r| r.set_shortcut_keys(id, keys, flags));
        self.changed(server).await;
        keys
    }

    async fn set_foreign(
        &self,
        server: &ObjectServer,
        emitter: &SignalEmitter<'_>,
        id: &[String],
        keys: &[Seq],
    ) {
        let Some(keys) = self.write(|r| r.set_foreign_shortcut_keys(id, keys)) else {
            return;
        };
        self.changed(server).await;
        if let Err(err) = Self::your_shortcuts_changed(emitter, id, &seqs_to_wire(keys)).await {
            warn!("error emitting yourShortcutsChanged: {err:?}");
        }
    }
}

#[interface(name = "org.kde.KGlobalAccel")]
impl KGlobalAccel {
    #[zbus(name = "allComponents")]
    fn all_components(&self) -> Vec<OwnedObjectPath> {
        self.read(|r| r.all_component_paths())
            .into_iter()
            .map(object_path)
            .collect()
    }

    #[zbus(name = "allMainComponents")]
    fn all_main_components(&self) -> Vec<Vec<String>> {
        self.read(|r| r.all_main_components())
    }

    #[zbus(name = "allActionsForComponent")]
    fn all_actions_for_component(&self, action_id: Vec<String>) -> Vec<Vec<String>> {
        self.read(|r| r.all_actions_for_component(&action_id))
    }

    #[zbus(name = "action")]
    fn action(&self, key: i32) -> Vec<String> {
        self.read(|r| r.action_for_key(&[key, 0, 0, 0]))
    }

    #[zbus(name = "getComponent")]
    fn get_component(&self, component_unique: &str) -> Result<OwnedObjectPath, Error> {
        self.read(|r| r.component_path(component_unique))
            .map(object_path)
            .map_err(Error::NoSuchComponent)
    }

    #[zbus(name = "shortcut")]
    fn shortcut(&self, action_id: Vec<String>) -> Vec<i32> {
        reg::keys_from_seqs(&self.read(|r| r.shortcut_keys(&action_id)))
    }

    #[zbus(name = "defaultShortcut")]
    fn default_shortcut(&self, action_id: Vec<String>) -> Vec<i32> {
        reg::keys_from_seqs(&self.read(|r| r.default_shortcut_keys(&action_id)))
    }

    #[zbus(name = "setShortcut")]
    async fn set_shortcut(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        action_id: Vec<String>,
        keys: Vec<i32>,
        flags: u32,
    ) -> Vec<i32> {
        let seqs = reg::seqs_from_keys(&keys);
        reg::keys_from_seqs(&self.set_keys(server, &action_id, &seqs, flags).await)
    }

    #[zbus(name = "setForeignShortcut")]
    async fn set_foreign_shortcut(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        action_id: Vec<String>,
        keys: Vec<i32>,
    ) {
        let seqs = reg::seqs_from_keys(&keys);
        self.set_foreign(server, &emitter, &action_id, &seqs).await;
    }

    #[zbus(name = "setInactive")]
    async fn set_inactive(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        action_id: Vec<String>,
    ) {
        self.write(|r| r.set_inactive(&action_id));
        self.changed(server).await;
    }

    #[zbus(name = "doRegister")]
    async fn do_register(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        action_id: Vec<String>,
    ) {
        self.write(|r| r.do_register(&action_id));
        self.changed(server).await;
    }

    #[zbus(name = "unRegister")]
    async fn un_register(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        action_id: Vec<String>,
    ) {
        if let [component, action, ..] = action_id.as_slice() {
            self.write(|r| r.unregister(component, action));
            self.changed(server).await;
        }
    }

    #[zbus(name = "activateGlobalShortcutContext")]
    fn activate_global_shortcut_context(&self, _component: &str, _context: &str) {}

    #[zbus(name = "getGlobalShortcutsByKey")]
    fn get_global_shortcuts_by_key(&self, key: i32) -> Vec<WireInfo> {
        let infos = self.read(|r| r.infos_for_key(&[key, 0, 0, 0]));
        infos.into_iter().map(info_to_wire).collect()
    }

    #[zbus(name = "isGlobalShortcutAvailable")]
    fn is_global_shortcut_available(&self, key: i32, component: &str) -> bool {
        self.read(|r| r.is_available(&[key, 0, 0, 0], component))
    }

    #[zbus(name = "unregister")]
    async fn unregister(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        component_unique: &str,
        shortcut_unique: &str,
    ) -> bool {
        let removed = self.write(|r| r.unregister(component_unique, shortcut_unique));
        self.changed(server).await;
        removed
    }

    #[zbus(name = "blockGlobalShortcuts")]
    async fn block_global_shortcuts(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        block: bool,
    ) {
        self.write(|r| r.block(block));
        self.changed(server).await;
    }

    #[zbus(signal, name = "yourShortcutsChanged")]
    async fn your_shortcuts_changed(
        emitter: &SignalEmitter<'_>,
        action_id: &[String],
        new_keys: &[Wire],
    ) -> zbus::Result<()>;

    #[zbus(name = "shortcutKeys")]
    fn shortcut_keys(&self, action_id: Vec<String>) -> Vec<Wire> {
        seqs_to_wire(self.read(|r| r.shortcut_keys(&action_id)))
    }

    #[zbus(name = "defaultShortcutKeys")]
    fn default_shortcut_keys(&self, action_id: Vec<String>) -> Vec<Wire> {
        seqs_to_wire(self.read(|r| r.default_shortcut_keys(&action_id)))
    }

    #[zbus(name = "actionList")]
    fn action_list(&self, key: Wire) -> Vec<String> {
        self.read(|r| r.action_for_key(&seq_from_wire(&key)))
    }

    #[zbus(name = "setShortcutKeys")]
    async fn set_shortcut_keys(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        action_id: Vec<String>,
        keys: Vec<Wire>,
        flags: u32,
    ) -> Vec<Wire> {
        let seqs = seqs_from_wire(&keys);
        seqs_to_wire(self.set_keys(server, &action_id, &seqs, flags).await)
    }

    #[zbus(name = "setForeignShortcutKeys")]
    async fn set_foreign_shortcut_keys(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        action_id: Vec<String>,
        keys: Vec<Wire>,
    ) {
        let seqs = seqs_from_wire(&keys);
        self.set_foreign(server, &emitter, &action_id, &seqs).await;
    }

    #[zbus(name = "globalShortcutsByKey")]
    fn global_shortcuts_by_key(&self, key: Wire, _match_type: (i32,)) -> Vec<WireInfo> {
        let infos = self.read(|r| r.infos_for_key(&seq_from_wire(&key)));
        infos.into_iter().map(info_to_wire).collect()
    }

    #[zbus(name = "globalShortcutAvailable")]
    fn global_shortcut_available(&self, key: Wire, component: &str) -> bool {
        self.read(|r| r.is_available(&seq_from_wire(&key), component))
    }

    #[zbus(name = "setInverseShortcutActions")]
    fn set_inverse_shortcut_actions(
        &self,
        _component_unique: &str,
        _forward_action_unique: &str,
        _backward_action_unique: &str,
        _flags: u32,
    ) -> bool {
        true
    }
}

#[interface(name = "org.kde.kglobalaccel.Component")]
impl ComponentObject {
    #[zbus(property, name = "friendlyName")]
    fn friendly_name(&self) -> String {
        self.service
            .read(|r| r.component(&self.unique).map(|c| c.friendly.clone()))
            .unwrap_or_default()
    }

    #[zbus(property, name = "uniqueName")]
    fn unique_name(&self) -> String {
        self.unique.clone()
    }

    #[zbus(signal, name = "globalShortcutPressed")]
    pub async fn global_shortcut_pressed(
        emitter: &SignalEmitter<'_>,
        component_unique: &str,
        action_unique: &str,
        timestamp: i64,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "globalShortcutRepeated")]
    pub async fn global_shortcut_repeated(
        emitter: &SignalEmitter<'_>,
        component_unique: &str,
        action_unique: &str,
        timestamp: i64,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "globalShortcutReleased")]
    pub async fn global_shortcut_released(
        emitter: &SignalEmitter<'_>,
        component_unique: &str,
        action_unique: &str,
        timestamp: i64,
    ) -> zbus::Result<()>;

    #[zbus(name = "cleanUp")]
    async fn clean_up(&self, #[zbus(object_server)] server: &ObjectServer) -> bool {
        let changed = self.service.write(|r| r.clean_up(&self.unique));
        self.service.changed(server).await;
        changed
    }

    #[zbus(name = "isActive")]
    fn is_active(&self) -> bool {
        self.service.read(|r| r.is_active(&self.unique))
    }

    /// Qt declares this twice, with a context and without one, and System Settings' Shortcuts
    /// page calls the second. A zbus method without arguments never reads the body, so one method
    /// answers both forms; every action sits in the default context anyway.
    #[zbus(name = "shortcutNames")]
    fn shortcut_names(&self) -> Vec<String> {
        self.service.read(|r| r.shortcut_names(&self.unique))
    }

    /// Both of Qt's forms, as [`Self::shortcut_names`].
    #[zbus(name = "allShortcutInfos")]
    fn all_shortcut_infos(&self) -> Vec<WireInfo> {
        let infos = self.service.read(|r| r.infos(&self.unique));
        infos.into_iter().map(info_to_wire).collect()
    }

    #[zbus(name = "getShortcutContexts")]
    fn get_shortcut_contexts(&self) -> Vec<String> {
        vec![reg::DEFAULT_CONTEXT.to_owned()]
    }

    #[zbus(name = "invokeShortcut")]
    async fn invoke_shortcut(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        action_name: &str,
    ) {
        if !self
            .service
            .read(|r| r.has_action(&self.unique, action_name))
        {
            return;
        }
        let timestamp = i64::from(InputTime::now().millis());
        if let Err(err) =
            Self::global_shortcut_pressed(&emitter, &self.unique, action_name, timestamp).await
        {
            warn!("error emitting globalShortcutPressed: {err:?}");
        }
    }
}

impl KGlobalAccel {
    /// Serves `/kglobalaccel` and one object per saved component on `conn`.
    pub fn serve(&self, conn: &zbus::blocking::Connection) -> zbus::Result<()> {
        let server = conn.object_server();
        server.at("/kglobalaccel", self.clone())?;
        let saved: Vec<String> = self.read(|r| r.components().map(|(u, _)| u.clone()).collect());
        for unique in &saved {
            let object = ComponentObject {
                unique: unique.clone(),
                service: self.clone(),
            };
            server.at(reg::component_path(unique), object)?;
        }
        *self.served.lock().unwrap() = saved.into_iter().collect();
        Ok(())
    }
}

impl Start for KGlobalAccel {
    fn start(self, monitor: bool) -> anyhow::Result<zbus::blocking::Connection> {
        let conn = zbus::blocking::Connection::session()?;
        self.serve(&conn)?;
        request_name(&conn, "org.kde.kglobalaccel", monitor)?;
        Ok(conn)
    }
}

/// Where the global shortcuts are saved: `$XDG_STATE_HOME/newm-rs/kglobalshortcuts.json`.
pub fn registry_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(state.join("newm-rs").join("kglobalshortcuts.json"))
}

/// The niri side: the keys apps hold as bindings, and the signals their presses send.
#[derive(Default)]
pub struct Shortcuts {
    pub registry: Option<Arc<Mutex<Registry>>>,
    pub conn: Option<zbus::blocking::Connection>,
    binds: Vec<Bind>,
    /// Component and action per binding, by the index in its action id.
    actions: Vec<(String, String)>,
    /// The generation of the snapshot `binds` come from.
    generation: u64,
}

/// The Qt keys the config's own key bindings hold, Mod resolved to `mod_key`.
pub fn reserved_keys(config: &Config, mod_key: ModKey) -> Vec<i32> {
    config
        .binds
        .0
        .iter()
        .filter_map(|bind| {
            let Trigger::Keysym(keysym) = bind.key.trigger else {
                return None;
            };
            let mut mods = bind.key.modifiers;
            if mods.contains(Modifiers::COMPOSITOR) {
                mods.remove(Modifiers::COMPOSITOR);
                mods |= mod_key.to_modifiers();
            }
            let plain = Modifiers::CTRL | Modifiers::SHIFT | Modifiers::ALT | Modifiers::SUPER;
            if !plain.contains(mods) {
                return None;
            }
            qtkey::to_qt(keysym.raw(), mods.bits())
        })
        .collect()
}

impl Shortcuts {
    pub fn binds(&self) -> &[Bind] {
        &self.binds
    }

    /// The config (re)loaded: its bindings are reserved, and apps' keys rebound around them.
    pub fn set_reserved(&mut self, keys: Vec<i32>) {
        let Some(registry) = &self.registry else {
            return;
        };
        let snapshot = {
            let mut r = registry.lock().unwrap();
            r.set_reserved(keys);
            r.snapshot()
        };
        self.set_bindings(snapshot);
    }

    /// Binds the keys of `snapshot`, unless `binds` come from a newer one: the D-Bus thread
    /// sends its snapshots after awaits, so an older one can arrive after a newer one.
    pub fn set_bindings(&mut self, snapshot: reg::Snapshot) {
        if snapshot.generation <= self.generation {
            return;
        }
        self.generation = snapshot.generation;
        self.binds.clear();
        self.actions.clear();
        for binding in snapshot.bindings {
            let Some((keysym, mods)) = qtkey::from_qt(binding.key) else {
                continue;
            };
            let Ok(index) = u32::try_from(self.actions.len()) else {
                break;
            };
            let id = KDE_ACTION | index;
            self.binds.push(Bind {
                key: Key {
                    trigger: Trigger::Keysym(Keysym::new(keysym)),
                    modifiers: Modifiers::from_bits_truncate(mods),
                },
                action: BoundAction::Both {
                    press: Action::GlobalShortcutActivateAction(id),
                    release: Action::GlobalShortcutDeactivateAction(id),
                },
                repeat: false,
                pass_through: None,
                cooldown: None,
                allow_when_locked: false,
                allow_inhibiting: true,
                hotkey_overlay_title: None,
            });
            self.actions.push((binding.component, binding.action));
        }
    }

    /// A binding's key went down (`pressed`) or up: tells its app, or starts a desktop-file
    /// component's action on the press.
    pub fn activate(&self, action: u32, pressed: bool, time: InputTime) {
        let Some((component, name)) = self.actions.get((action & !KDE_ACTION) as usize) else {
            return;
        };
        let launch = self
            .registry
            .as_ref()
            .and_then(|r| r.lock().unwrap().service_launch(component, name));
        if let Some(launch) = launch {
            if pressed {
                start_service(self.conn.as_ref(), launch);
            }
            return;
        }
        let Some(conn) = &self.conn else {
            return;
        };
        let emitter = match SignalEmitter::new(conn.inner(), reg::component_path(component)) {
            Ok(emitter) => emitter,
            Err(err) => {
                warn!("error making a kglobalaccel signal: {err:?}");
                return;
            }
        };
        let timestamp = i64::from(time.millis());
        let res = async_io::block_on(async {
            if pressed {
                ComponentObject::global_shortcut_pressed(&emitter, component, name, timestamp).await
            } else {
                ComponentObject::global_shortcut_released(&emitter, component, name, timestamp)
                    .await
            }
        });
        if let Err(err) = res {
            warn!("error emitting a kglobalaccel shortcut: {err:?}");
        }
    }
}

/// Starts a desktop-file component's action as kglobalacceld does: `org.freedesktop.Application`
/// for a D-Bus activatable app (activating it if it does not run, on zbus's thread so a slow start
/// never holds the compositor), else its `Exec` line.
fn start_service(conn: Option<&zbus::blocking::Connection>, launch: Launch) {
    match launch {
        Launch::Exec(argv) => crate::utils::spawning::spawn(argv, None),
        Launch::DBus { bus, path, action } => {
            let Some(conn) = conn else {
                return;
            };
            let conn = conn.inner().clone();
            let executor = conn.clone();
            let task = executor.executor().spawn(
                async move {
                    let platform: HashMap<&str, Value<'_>> = HashMap::new();
                    let iface = Some("org.freedesktop.Application");
                    let res = match &action {
                        Some(a) => {
                            let body = (a.as_str(), Vec::<Value<'_>>::new(), platform);
                            conn.call_method(
                                Some(bus.as_str()),
                                path.as_str(),
                                iface,
                                "ActivateAction",
                                &body,
                            )
                            .await
                        }
                        None => {
                            conn.call_method(
                                Some(bus.as_str()),
                                path.as_str(),
                                iface,
                                "Activate",
                                &(platform,),
                            )
                            .await
                        }
                    };
                    if let Err(err) = res {
                        warn!("error starting {bus} {action:?} for a global shortcut: {err:?}");
                    }
                },
                "kglobalaccel service action",
            );
            task.detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_keys_become_bindings_after_the_portals() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set_bindings(reg::Snapshot {
            generation: 1,
            bindings: vec![reg::Binding {
                key: 0x53 | 0x1000_0000 | 0x0200_0000,
                component: String::from("org.kde.spectacle.desktop"),
                action: String::from("RectangularRegionScreenShot"),
            }],
        });
        let bind = &shortcuts.binds()[0];
        assert_eq!(bind.key.trigger, Trigger::Keysym(Keysym::new(0x73)));
        assert_eq!(bind.key.modifiers, Modifiers::SUPER | Modifiers::SHIFT);
        let BoundAction::Both {
            press: Action::GlobalShortcutActivateAction(id),
            ..
        } = bind.action
        else {
            panic!("a press and release binding");
        };
        assert!(owns(id));
        assert!(!owns(1), "the GNOME portal's ids stay its own");
        assert!(!bind.allow_when_locked);
        assert!(bind.allow_inhibiting);
    }

    #[test]
    fn a_stale_snapshot_never_replaces_a_newer_one() {
        const PRINT: Seq = [0x0100_0009, 0, 0, 0];
        let registry = Arc::new(Mutex::new(Registry::default()));
        let id: Vec<String> = ["app", "one", "App", "One"].map(String::from).to_vec();
        let older = {
            let mut r = registry.lock().unwrap();
            r.do_register(&id);
            r.set_shortcut_keys(&id, &[PRINT], reg::SET_PRESENT);
            r.snapshot()
        };
        let newer = {
            let mut r = registry.lock().unwrap();
            r.set_inactive(&id);
            r.snapshot()
        };
        let mut shortcuts = Shortcuts {
            registry: Some(registry.clone()),
            ..Shortcuts::default()
        };
        shortcuts.set_bindings(newer);
        shortcuts.set_bindings(older);
        assert!(
            shortcuts.binds().is_empty(),
            "the older snapshot came late and is dropped"
        );

        // A config reload reads the registry afresh: a snapshot sent before it is stale too.
        let queued = {
            let mut r = registry.lock().unwrap();
            r.set_shortcut_keys(&id, &[PRINT], reg::SET_PRESENT);
            r.snapshot()
        };
        assert_eq!(queued.bindings.len(), 1);
        shortcuts.set_reserved(vec![PRINT[0]]);
        shortcuts.set_bindings(queued);
        assert!(shortcuts.binds().is_empty(), "the reload reserved Print");
    }

    #[test]
    fn the_configs_mod_binds_are_reserved_as_super() {
        let config = Config::parse_mem(
            r#"
            binds {
                Mod+Shift+S { screenshot; }
                Print { screenshot-screen; }
                Mod+WheelScrollDown { focus-workspace-down; }
            }
            "#,
        )
        .unwrap();
        let mut keys = reserved_keys(&config, ModKey::Super);
        keys.sort_unstable();
        assert_eq!(keys, vec![0x0100_0009, 0x53 | 0x1000_0000 | 0x0200_0000]);
    }
}
