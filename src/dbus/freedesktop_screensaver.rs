use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use anyhow::Context;
use futures_util::StreamExt;
use zbus::message::Header;
use zbus::names::{OwnedUniqueName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::NoneValue;
use zbus::{fdo, interface, Task};

use super::{request_name, Start};

#[derive(Clone)]
pub struct ScreenSaver {
    is_inhibited: Arc<AtomicBool>,
    is_broken: Arc<AtomicBool>,
    inhibitors: Arc<Mutex<HashMap<u32, Inhibitor>>>,
    counter: Arc<AtomicU32>,
    monitor_task: Arc<OnceLock<Task<()>>>,
    activity: Arc<Activity>,
    to_niri: calloop::channel::Sender<ToNiri>,
}

/// The interface an idle inhibition came through. Both share one cookie space, and a release
/// through one interface never drops an inhibition of the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    ScreenSaver,
    PolicyAgent,
}

/// One idle inhibition: the bus name that holds it and the interface it came through.
#[derive(Debug)]
pub struct Inhibitor {
    owner: OwnedUniqueName,
    source: Source,
}

/// What `GetActive`, `GetActiveTime` and `GetSessionIdleTime` read; niri keeps it current.
#[derive(Debug)]
pub struct Activity {
    locked: AtomicBool,
    locked_since: Mutex<Option<Instant>>,
    last_input: Mutex<Instant>,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            locked: AtomicBool::new(false),
            locked_since: Mutex::new(None),
            last_input: Mutex::new(Instant::now()),
        }
    }
}

impl Activity {
    /// The session lock changed; answers whether it did (and ActiveChanged is due).
    pub fn set_locked(&self, locked: bool) -> bool {
        if self.locked.swap(locked, Ordering::SeqCst) == locked {
            return false;
        }
        *self.locked_since.lock().unwrap() = locked.then(Instant::now);
        true
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    /// Seconds the screen saver (the lock screen) has been on, 0 while off.
    pub fn active_secs(&self) -> u32 {
        let since = *self.locked_since.lock().unwrap();
        since.map_or(0, |t| {
            u32::try_from(t.elapsed().as_secs()).unwrap_or(u32::MAX)
        })
    }

    /// The user touched an input device.
    pub fn input(&self) {
        *self.last_input.lock().unwrap() = Instant::now();
    }

    pub fn idle_secs(&self) -> u32 {
        let elapsed = self.last_input.lock().unwrap().elapsed().as_secs();
        u32::try_from(elapsed).unwrap_or(u32::MAX)
    }
}

pub enum ToNiri {
    /// `SimulateUserActivity`: as if the user touched an input device.
    SimulateActivity,
}

/// Asks logind to lock this session, as `loginctl lock-session` does: swayidle starts the
/// locker on logind's Lock signal.
async fn lock_session() -> fdo::Result<()> {
    let id = std::env::var("XDG_SESSION_ID")
        .map_err(|_| fdo::Error::Failed(String::from("XDG_SESSION_ID is unset")))?;
    let conn = zbus::Connection::system()
        .await
        .map_err(|err| fdo::Error::Failed(format!("no system bus: {err}")))?;
    conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        "LockSession",
        &(id,),
    )
    .await
    .map_err(|err| fdo::Error::Failed(format!("logind refused to lock: {err}")))?;
    Ok(())
}

/// Tells screen saver clients the lock screen came or went, on both object paths.
pub fn emit_active_changed(conn: &zbus::blocking::Connection, active: bool) {
    for path in ["/org/freedesktop/ScreenSaver", "/ScreenSaver"] {
        let emitter = match SignalEmitter::new(conn.inner(), path) {
            Ok(emitter) => emitter,
            Err(err) => {
                warn!("error making ActiveChanged for {path}: {err:?}");
                continue;
            }
        };
        if let Err(err) = async_io::block_on(ScreenSaver::active_changed(&emitter, active)) {
            warn!("error emitting ActiveChanged on {path}: {err:?}");
        }
    }
}

#[interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    async fn inhibit(
        &mut self,
        #[zbus(header)] hdr: Header<'_>,
        application_name: &str,
        reason_for_inhibit: &str,
    ) -> fdo::Result<u32> {
        trace!(
            "fdo inhibit, app: `{application_name}`, reason: `{reason_for_inhibit}`, owner: {:?}",
            hdr.sender()
        );

        let Some(name) = hdr.sender() else {
            return Err(fdo::Error::Failed(String::from("no sender")));
        };
        let name = OwnedUniqueName::from(name.to_owned());

        self.add_inhibitor(name, Source::ScreenSaver)
            .ok_or_else(|| fdo::Error::Failed(String::from("no available cookie")))
    }

    async fn un_inhibit(
        &mut self,
        #[zbus(header)] hdr: Header<'_>,
        cookie: u32,
    ) -> fdo::Result<()> {
        trace!("fdo uninhibit, cookie: {cookie}");

        let Some(caller) = hdr.sender() else {
            return Err(fdo::Error::Failed(String::from("no sender")));
        };
        if self.remove_inhibitor(cookie, Source::ScreenSaver, caller) {
            Ok(())
        } else {
            Err(fdo::Error::Failed(String::from("invalid cookie")))
        }
    }

    /// `true` locks the session; the lock screen cannot be turned off from here.
    async fn set_active(&self, active: bool) -> fdo::Result<bool> {
        if !active {
            return Ok(false);
        }
        lock_session().await?;
        Ok(true)
    }

    async fn lock(&self) -> fdo::Result<()> {
        lock_session().await
    }

    async fn get_active(&self) -> bool {
        self.activity.is_locked()
    }

    async fn get_active_time(&self) -> u32 {
        self.activity.active_secs()
    }

    async fn get_session_idle_time(&self) -> u32 {
        self.activity.idle_secs()
    }

    async fn simulate_user_activity(&self) {
        if let Err(err) = self.to_niri.send(ToNiri::SimulateActivity) {
            warn!("error sending SimulateUserActivity to niri: {err:?}");
        }
    }

    /// Throttling the screen saver's animation means nothing here: newm-rs draws none.
    async fn throttle(&self, _application_name: &str, _reason_for_inhibit: &str) -> u32 {
        self.counter.fetch_add(1, Ordering::SeqCst).max(1)
    }

    async fn un_throttle(&self, _cookie: u32) {}

    #[zbus(signal)]
    async fn active_changed(emitter: &SignalEmitter<'_>, active: bool) -> zbus::Result<()>;
}

impl ScreenSaver {
    /// Holds idle off for `owner` until it lets go or leaves the bus; answers the cookie.
    pub fn add_inhibitor(&self, owner: OwnedUniqueName, source: Source) -> Option<u32> {
        let mut inhibitors = self.inhibitors.lock().unwrap();

        for _ in 0..3 {
            let mut inhibitor_key = self.counter.fetch_add(1, Ordering::SeqCst);
            if inhibitor_key == 0 {
                // Some clients don't like 0, add one more.
                inhibitor_key = self.counter.fetch_add(1, Ordering::SeqCst);
            }

            if let Entry::Vacant(entry) = inhibitors.entry(inhibitor_key) {
                entry.insert(Inhibitor { owner, source });
                self.is_inhibited.store(true, Ordering::SeqCst);
                return Some(inhibitor_key);
            }
        }

        None
    }

    /// Drops the inhibition `cookie` names if `caller` holds it through `source`; any other
    /// caller's or interface's cookie stays.
    pub fn remove_inhibitor(&self, cookie: u32, source: Source, caller: &UniqueName<'_>) -> bool {
        let mut inhibitors = self.inhibitors.lock().unwrap();
        let own = inhibitors
            .get(&cookie)
            .is_some_and(|i| i.source == source && i.owner == *caller);

        if own && inhibitors.remove(&cookie).is_some() {
            if inhibitors.is_empty() {
                self.is_inhibited.store(false, Ordering::SeqCst);
            }
            true
        } else {
            false
        }
    }

    /// A cookie no inhibitor holds, for an inhibition that does not hold idle off.
    pub fn spare_cookie(&self) -> u32 {
        let inhibitors = self.inhibitors.lock().unwrap();
        loop {
            let cookie = self.counter.fetch_add(1, Ordering::SeqCst);
            if cookie != 0 && !inhibitors.contains_key(&cookie) {
                return cookie;
            }
        }
    }

    pub fn holds(&self, cookie: u32) -> bool {
        self.inhibitors.lock().unwrap().contains_key(&cookie)
    }
}

impl ScreenSaver {
    pub fn new(
        is_inhibited: Arc<AtomicBool>,
        activity: Arc<Activity>,
        to_niri: calloop::channel::Sender<ToNiri>,
    ) -> Self {
        Self {
            is_inhibited,
            is_broken: Arc::new(AtomicBool::new(false)),
            inhibitors: Arc::new(Mutex::new(HashMap::new())),
            // Start from 1 because some clients don't like 0.
            counter: Arc::new(AtomicU32::new(1)),
            monitor_task: Arc::new(OnceLock::new()),
            activity,
            to_niri,
        }
    }
}

async fn monitor_disappeared_clients(
    conn: &zbus::Connection,
    is_inhibited: Arc<AtomicBool>,
    inhibitors: Arc<Mutex<HashMap<u32, Inhibitor>>>,
) -> anyhow::Result<()> {
    let proxy = fdo::DBusProxy::new(conn)
        .await
        .context("error creating a DBusProxy")?;

    let mut stream = proxy
        .receive_name_owner_changed_with_args(&[(2, UniqueName::null_value())])
        .await
        .context("error creating a NameOwnerChanged stream")?;

    while let Some(signal) = stream.next().await {
        let args = signal
            .args()
            .context("error retrieving NameOwnerChanged args")?;

        let Some(name) = &**args.old_owner() else {
            continue;
        };

        if args.new_owner().is_none() {
            trace!("fdo ScreenSaver client disappeared: {name}");

            let mut inhibitors = inhibitors.lock().unwrap();
            inhibitors.retain(|_, i| i.owner != *name);
            is_inhibited.store(!inhibitors.is_empty(), Ordering::SeqCst);
        } else {
            error!("non-null new_owner should've been filtered out");
        }
    }

    Ok(())
}

impl Start for ScreenSaver {
    fn start(self, monitor: bool) -> anyhow::Result<zbus::blocking::Connection> {
        let is_inhibited = self.is_inhibited.clone();
        let is_broken = self.is_broken.clone();
        let inhibitors = self.inhibitors.clone();
        let monitor_task = self.monitor_task.clone();

        let conn = zbus::blocking::Connection::session()?;
        let org_fd_ss_registered = conn
            .object_server()
            .at("/org/freedesktop/ScreenSaver", self.clone())?;
        let ss_registered = conn.object_server().at("/ScreenSaver", self)?;

        if !org_fd_ss_registered && !ss_registered {
            anyhow::bail!("failed to register any org.freedesktop.ScreenSaver interface")
        }

        request_name(&conn, "org.freedesktop.ScreenSaver", monitor)?;

        let async_conn = conn.inner();
        let future = {
            let conn = async_conn.clone();
            async move {
                if let Err(err) =
                    monitor_disappeared_clients(&conn, is_inhibited.clone(), inhibitors.clone())
                        .await
                {
                    warn!("error monitoring org.freedesktop.ScreenSaver clients: {err:?}");
                    is_broken.store(true, Ordering::SeqCst);
                    is_inhibited.store(false, Ordering::SeqCst);
                    inhibitors.lock().unwrap().clear();
                }
            }
        };
        let task = async_conn
            .executor()
            .spawn(future, "monitor disappearing clients");
        monitor_task.set(task).unwrap();

        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_time_counts_from_the_lock_and_stops_at_the_unlock() {
        let activity = Activity::default();
        assert!(!activity.is_locked());
        assert_eq!(activity.active_secs(), 0);
        assert!(activity.set_locked(true));
        assert!(!activity.set_locked(true), "no ActiveChanged twice");
        assert!(activity.is_locked());
        *activity.locked_since.lock().unwrap() =
            Instant::now().checked_sub(std::time::Duration::from_secs(90));
        assert_eq!(activity.active_secs(), 90);
        assert!(activity.set_locked(false));
        assert_eq!(activity.active_secs(), 0);
    }

    #[test]
    fn idle_time_counts_from_the_last_input() {
        let activity = Activity::default();
        if let Some(t) = Instant::now().checked_sub(std::time::Duration::from_secs(30)) {
            *activity.last_input.lock().unwrap() = t;
            assert_eq!(activity.idle_secs(), 30);
        }
        activity.input();
        assert_eq!(activity.idle_secs(), 0);
    }

    #[test]
    fn policy_cookies_and_screensaver_cookies_share_one_space() {
        let (tx, _rx) = calloop::channel::channel();
        let ss = ScreenSaver::new(Arc::new(AtomicBool::new(false)), Arc::default(), tx);
        let owner = OwnedUniqueName::try_from(":1.42").unwrap();
        let a = ss
            .add_inhibitor(owner.clone(), Source::ScreenSaver)
            .unwrap();
        assert!(ss.is_inhibited.load(Ordering::SeqCst));
        let b = ss.spare_cookie();
        assert_ne!(a, b);
        assert!(ss.holds(a) && !ss.holds(b));
        assert!(ss.remove_inhibitor(a, Source::ScreenSaver, &owner));
        assert!(!ss.is_inhibited.load(Ordering::SeqCst));
        assert!(!ss.remove_inhibitor(a, Source::ScreenSaver, &owner));
    }

    #[test]
    fn a_release_drops_only_the_callers_own_inhibition() {
        let (tx, _rx) = calloop::channel::channel();
        let ss = ScreenSaver::new(Arc::new(AtomicBool::new(false)), Arc::default(), tx);
        let app = OwnedUniqueName::try_from(":1.42").unwrap();
        let other = OwnedUniqueName::try_from(":1.43").unwrap();
        let a = ss.add_inhibitor(app.clone(), Source::ScreenSaver).unwrap();
        assert!(!ss.remove_inhibitor(a, Source::ScreenSaver, &other));
        assert!(!ss.remove_inhibitor(a, Source::PolicyAgent, &app));
        assert!(ss.holds(a));
        assert!(ss.remove_inhibitor(a, Source::ScreenSaver, &app));
    }
}
