//! Inhibition cookies: `org.freedesktop.ScreenSaver.Inhibit` and PowerDevil's
//! `org.kde.Solid.PowerManagement.PolicyAgent.AddInhibition`, which the KDE Inhibit portal calls.

use std::collections::BTreeMap;

/// PolicyAgent policies (PowerDevil's `RequiredPolicy`): the portal asks `INTERRUPT_SESSION` to
/// hold off suspend and `CHANGE_SCREEN_SETTINGS` to hold off idle (dimming, locking, screen off).
pub const INTERRUPT_SESSION: u32 = 1;
pub const CHANGE_PROFILE: u32 = 2;
pub const CHANGE_SCREEN_SETTINGS: u32 = 4;

/// Whether a PolicyAgent inhibition holds off idle: newm has one idle path (swayidle locks, turns
/// the screens off and suspends from it), so suspend and screen inhibitions both hold it.
pub fn holds_off_idle(policies: u32) -> bool {
    policies & (INTERRUPT_SESSION | CHANGE_SCREEN_SETTINGS) != 0
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hold<O> {
    /// The D-Bus client that holds it: its holds go when it leaves the bus.
    pub owner: O,
    pub app: String,
    pub reason: String,
    /// Whether it holds off idle (a PolicyAgent `CHANGE_PROFILE` alone does not).
    pub idle: bool,
}

/// The inhibitions held, by cookie. Cookies start at 1 and skip 0, which some clients take for
/// a failure.
#[derive(Clone, Debug)]
pub struct Cookies<O> {
    next: u32,
    held: BTreeMap<u32, Hold<O>>,
}

impl<O> Default for Cookies<O> {
    fn default() -> Self {
        Self {
            next: 1,
            held: BTreeMap::new(),
        }
    }
}

impl<O: PartialEq> Cookies<O> {
    /// Holds an inhibition and answers its cookie; `None` only when every cookie is taken.
    pub fn add(&mut self, hold: Hold<O>) -> Option<u32> {
        for _ in 0..=self.held.len() {
            let cookie = self.next;
            self.next = self.next.wrapping_add(1).max(1);
            if cookie != 0 && !self.held.contains_key(&cookie) {
                self.held.insert(cookie, hold);
                return Some(cookie);
            }
        }
        None
    }

    pub fn remove(&mut self, cookie: u32) -> bool {
        self.held.remove(&cookie).is_some()
    }

    /// Drops every inhibition `owner` held; answers how many.
    pub fn remove_owner(&mut self, owner: &O) -> usize {
        let before = self.held.len();
        self.held.retain(|_, hold| hold.owner != *owner);
        before - self.held.len()
    }

    /// The clients holding inhibitions, to drop the holds of any that left the bus.
    pub fn owners(&self) -> Vec<O>
    where
        O: Clone + Ord,
    {
        let mut owners: Vec<O> = self.held.values().map(|hold| hold.owner.clone()).collect();
        owners.sort();
        owners.dedup();
        owners
    }

    pub fn idle_inhibited(&self) -> bool {
        self.held.values().any(|hold| hold.idle)
    }

    /// `(app, reason)` per inhibition, as PolicyAgent's `ListInhibitions` answers.
    pub fn list(&self) -> Vec<(String, String)> {
        self.held
            .values()
            .map(|hold| (hold.app.clone(), hold.reason.clone()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(owner: &str, idle: bool) -> Hold<String> {
        Hold {
            owner: owner.to_owned(),
            app: String::from("mpv"),
            reason: String::from("video"),
            idle,
        }
    }

    #[test]
    fn the_portal_policies_hold_off_idle_and_a_profile_change_does_not() {
        assert!(holds_off_idle(INTERRUPT_SESSION));
        assert!(holds_off_idle(CHANGE_SCREEN_SETTINGS));
        assert!(holds_off_idle(INTERRUPT_SESSION | CHANGE_SCREEN_SETTINGS));
        assert!(!holds_off_idle(CHANGE_PROFILE));
        assert!(!holds_off_idle(0));
    }

    #[test]
    fn cookies_start_at_one_and_idle_follows_the_holds() {
        let mut cookies = Cookies::default();
        let a = cookies.add(hold(":1.5", true)).unwrap();
        let b = cookies.add(hold(":1.6", false)).unwrap();
        assert_eq!((a, b), (1, 2));
        assert!(cookies.idle_inhibited());
        assert!(cookies.remove(a));
        assert!(!cookies.remove(a));
        assert!(!cookies.idle_inhibited());
        assert_eq!(
            cookies.list(),
            vec![(String::from("mpv"), String::from("video"))]
        );
    }

    #[test]
    fn a_client_leaving_drops_its_holds_only() {
        let mut cookies = Cookies::default();
        cookies.add(hold(":1.5", true));
        cookies.add(hold(":1.5", true));
        cookies.add(hold(":1.7", true));
        assert_eq!(
            cookies.owners(),
            vec![String::from(":1.5"), String::from(":1.7")]
        );
        assert_eq!(cookies.remove_owner(&String::from(":1.5")), 2);
        assert_eq!(cookies.len(), 1);
        assert!(cookies.idle_inhibited());
    }

    #[test]
    fn a_wrapped_counter_skips_zero_and_held_cookies() {
        let mut cookies = Cookies {
            next: u32::MAX,
            held: BTreeMap::new(),
        };
        assert_eq!(cookies.add(hold("a", true)), Some(u32::MAX));
        assert_eq!(cookies.add(hold("b", true)), Some(1));
        cookies.next = 1;
        assert_eq!(cookies.add(hold("c", true)), Some(2));
    }
}
