//! PowerDevil's `org.kde.Solid.PowerManagement.PolicyAgent`, which the KDE Inhibit portal calls
//! for Flatpak and GTK4 apps, on PowerDevil's main name `org.kde.Solid.PowerManagement`. PowerDevil
//! does not run in newm-rs's session (it would fight swayidle), so nothing answered and those apps
//! could not hold the screen on. Inhibitions with
//! the suspend or screen policy hold idle off through the same table as
//! `org.freedesktop.ScreenSaver.Inhibit`, so a client leaving the bus drops them too.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use newm_kde::inhibit::holds_off_idle;
use zbus::message::Header;
use zbus::names::{OwnedUniqueName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

use super::freedesktop_screensaver::{ScreenSaver, Source};
use super::{request_name, Start};

/// The PolicyAgent's own bus name.
pub const AGENT_NAME: &str = "org.kde.Solid.PowerManagement.PolicyAgent";
/// PowerDevil's main name, which xdg-desktop-portal-kde sends AddInhibition to (inhibit.cpp).
pub const POWERDEVIL_NAME: &str = "org.kde.Solid.PowerManagement";

/// Every inhibition by cookie: the bus name that added it, its policies, app and reason.
type Held = BTreeMap<u32, (OwnedUniqueName, u32, String, String)>;

#[derive(Clone)]
pub struct PolicyAgent {
    screen_saver: ScreenSaver,
    held: Arc<Mutex<Held>>,
}

impl PolicyAgent {
    pub fn new(screen_saver: ScreenSaver) -> Self {
        Self {
            screen_saver,
            held: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// The inhibitions still in force: an idle one whose client left the bus is gone.
    fn live(&self) -> Vec<(u32, u32, String, String)> {
        let mut held = self.held.lock().unwrap();
        held.retain(|cookie, (_, policies, ..)| {
            !holds_off_idle(*policies) || self.screen_saver.holds(*cookie)
        });
        held.iter()
            .map(|(c, (_, p, a, r))| (*c, *p, a.clone(), r.clone()))
            .collect()
    }

    /// Adds `owner`'s inhibition and answers its cookie.
    fn add(
        &self,
        owner: OwnedUniqueName,
        types: u32,
        app_name: String,
        reason: String,
    ) -> Option<u32> {
        let cookie = if holds_off_idle(types) {
            self.screen_saver
                .add_inhibitor(owner.clone(), Source::PolicyAgent)?
        } else {
            self.screen_saver.spare_cookie()
        };
        self.held
            .lock()
            .unwrap()
            .insert(cookie, (owner, types, app_name, reason));
        Some(cookie)
    }

    /// Drops the inhibition `cookie` names if `caller` added it here, and answers its app. A
    /// cookie of `org.freedesktop.ScreenSaver` or of another caller stays.
    fn release(&self, cookie: u32, caller: &UniqueName<'_>) -> Option<String> {
        let mut held = self.held.lock().unwrap();
        if held
            .get(&cookie)
            .is_none_or(|(owner, ..)| *owner != *caller)
        {
            return None;
        }
        let (_, _, app_name, _) = held.remove(&cookie)?;
        self.screen_saver
            .remove_inhibitor(cookie, Source::PolicyAgent, caller);
        Some(app_name)
    }
}

#[interface(name = "org.kde.Solid.PowerManagement.PolicyAgent")]
impl PolicyAgent {
    async fn add_inhibition(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        types: u32,
        app_name: String,
        reason: String,
    ) -> fdo::Result<u32> {
        debug!("PolicyAgent inhibition {types:#x} from {app_name:?}: {reason:?}");
        let owner = hdr
            .sender()
            .ok_or_else(|| fdo::Error::Failed(String::from("no sender")))?;
        let cookie = self
            .add(
                OwnedUniqueName::from(owner.to_owned()),
                types,
                app_name.clone(),
                reason.clone(),
            )
            .ok_or_else(|| fdo::Error::Failed(String::from("no available cookie")))?;
        let added = vec![(app_name, reason)];
        if let Err(err) = Self::inhibitions_changed(&emitter, &added, &[]).await {
            warn!("error emitting InhibitionsChanged: {err:?}");
        }
        Ok(cookie)
    }

    async fn release_inhibition(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        cookie: u32,
    ) {
        let removed = hdr.sender().and_then(|caller| self.release(cookie, caller));
        if let Some(app_name) = removed {
            if let Err(err) = Self::inhibitions_changed(&emitter, &[], &[app_name]).await {
                warn!("error emitting InhibitionsChanged: {err:?}");
            }
        }
    }

    async fn list_inhibitions(&self) -> Vec<(String, String)> {
        self.live().into_iter().map(|(_, _, a, r)| (a, r)).collect()
    }

    async fn has_inhibition(&self, types: u32) -> bool {
        self.live().iter().any(|(_, p, ..)| p & types != 0)
    }

    #[zbus(signal)]
    async fn inhibitions_changed(
        emitter: &SignalEmitter<'_>,
        added: &[(String, String)],
        removed: &[String],
    ) -> zbus::Result<()>;
}

impl Start for PolicyAgent {
    fn start(self, monitor: bool) -> anyhow::Result<zbus::blocking::Connection> {
        let conn = zbus::blocking::Connection::session()?;
        conn.object_server()
            .at("/org/kde/Solid/PowerManagement/PolicyAgent", self)?;
        request_name(&conn, AGENT_NAME, monitor)?;
        // xdg-desktop-portal-kde calls AddInhibition on PowerDevil's main name, at the same path,
        // not on the PolicyAgent name: without it the Inhibit portal held nothing.
        if let Err(err) = request_name(&conn, POWERDEVIL_NAME, monitor) {
            warn!("error owning {POWERDEVIL_NAME}: {err:?}");
        }
        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn a_release_drops_only_the_callers_own_policy_inhibition() {
        let (tx, _rx) = calloop::channel::channel();
        let ss = ScreenSaver::new(Arc::new(AtomicBool::new(false)), Arc::default(), tx);
        let agent = PolicyAgent::new(ss.clone());
        let app = OwnedUniqueName::try_from(":1.42").unwrap();
        let portal = OwnedUniqueName::try_from(":1.7").unwrap();

        // A ScreenSaver inhibit survives a PolicyAgent release of its cookie, even by its owner.
        let saver = ss.add_inhibitor(app.clone(), Source::ScreenSaver).unwrap();
        assert_eq!(agent.release(saver, &portal), None);
        assert_eq!(agent.release(saver, &app), None);
        assert!(ss.holds(saver));

        // Policy 4 (screen) holds idle off; 0 does not.
        let screen = agent
            .add(portal.clone(), 4, "Video".into(), "playing".into())
            .unwrap();
        let other = agent
            .add(portal.clone(), 0, "Mail".into(), "sync".into())
            .unwrap();
        assert!(ss.holds(screen) && !ss.holds(other));
        assert_eq!(agent.release(screen, &app), None, "another caller's cookie");
        assert!(ss.holds(screen));
        assert!(!ss.remove_inhibitor(screen, Source::ScreenSaver, &portal));
        assert_eq!(agent.release(screen, &portal).as_deref(), Some("Video"));
        assert!(!ss.holds(screen));
        assert_eq!(agent.release(other, &portal).as_deref(), Some("Mail"));
        assert!(ss.holds(saver));
    }
}
