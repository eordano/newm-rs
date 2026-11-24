//! The part of KWin's own interface that Spectacle calls to pick a window to record, and to name
//! the window in a screenshot.
//!
//! A window's "uuid" here is its niri window id, the same handle `ScreenShot2.CaptureWindow` and
//! `zkde_screencast_unstable_v1.stream_window` take.

use std::collections::HashMap;

use zbus::interface;
use zbus::zvariant::{OwnedValue, Value};

use super::{request_name, Start};
use crate::window::mapped::MappedId;

pub struct KWin {
    to_niri: calloop::channel::Sender<Request>,
}

pub enum Request {
    /// Lets the user click a window. `None` when they press Escape or click no window.
    Pick(async_channel::Sender<Option<MappedId>>),
    /// What the pick that just ended chose: its window, or else the output it clicked.
    Picked {
        window: Option<MappedId>,
        reply: async_channel::Sender<Option<WindowInfo>>,
    },
    Info {
        id: u64,
        reply: async_channel::Sender<Option<WindowInfo>>,
    },
    /// Every window, for KRunner's window search.
    Windows(async_channel::Sender<Vec<newm_kde::krunner::Window>>),
    /// Focus a window KRunner's search ran, as Alt+Tab does.
    Activate(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    /// `None` for a screen picked where no window was.
    pub id: Option<u64>,
    pub title: String,
    pub app_id: String,
    /// The window's geometry in the global space, in logical pixels.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.kde.KWin.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    UserCancel(String),
}

type Info = HashMap<String, OwnedValue>;

#[interface(name = "org.kde.KWin")]
impl KWin {
    #[zbus(name = "queryWindowInfo")]
    async fn query_window_info(&self) -> Result<Info, Error> {
        let (reply, from_niri) = async_channel::bounded(1);
        self.send(Request::Pick(reply))?;
        let window = from_niri.recv().await.ok().flatten();

        let (reply, from_niri) = async_channel::bounded(1);
        self.send(Request::Picked { window, reply })?;
        let info = Self::receive(from_niri).await?;
        let info = info.ok_or_else(|| Error::UserCancel("nothing was picked".to_owned()))?;
        Ok(info.to_map())
    }

    #[zbus(name = "getWindowInfo")]
    async fn get_window_info(&self, uuid: String) -> Result<Info, Error> {
        let Some(id) = parse_window_id(&uuid) else {
            return Ok(Info::new());
        };
        let info = self.info(id).await?;
        Ok(info.map(|info| info.to_map()).unwrap_or_default())
    }
}

impl KWin {
    pub fn new(to_niri: calloop::channel::Sender<Request>) -> Self {
        Self { to_niri }
    }

    fn send(&self, request: Request) -> Result<(), Error> {
        self.to_niri.send(request).map_err(|err| {
            warn!("error sending message to niri: {err:?}");
            Error::ZBus(zbus::Error::Failure("niri is not answering".to_owned()))
        })
    }

    async fn info(&self, id: u64) -> Result<Option<WindowInfo>, Error> {
        let (reply, from_niri) = async_channel::bounded(1);
        self.send(Request::Info { id, reply })?;
        Self::receive(from_niri).await
    }

    async fn receive<T>(from_niri: async_channel::Receiver<T>) -> Result<T, Error> {
        from_niri.recv().await.map_err(|err| {
            warn!("error receiving message from niri: {err:?}");
            Error::ZBus(zbus::Error::Failure("niri is not answering".to_owned()))
        })
    }
}

impl WindowInfo {
    fn to_map(&self) -> Info {
        let mut info = Info::new();
        let mut insert = |key: &str, value: Value<'_>| {
            info.insert(key.to_owned(), value.try_to_owned().unwrap());
        };
        if let Some(id) = self.id {
            insert("uuid", Value::from(id.to_string()));
        }
        insert("caption", Value::from(self.title.as_str()));
        insert("resourceClass", Value::from(self.app_id.as_str()));
        insert("resourceName", Value::from(self.app_id.as_str()));
        insert("desktopFile", Value::from(self.app_id.as_str()));
        insert("x", Value::from(self.x));
        insert("y", Value::from(self.y));
        insert("width", Value::from(self.width));
        insert("height", Value::from(self.height));
        info
    }
}

/// Reads a window id, alone or in the braces KWin puts around a uuid.
pub fn parse_window_id(uuid: &str) -> Option<u64> {
    uuid.trim_matches(['{', '}']).parse().ok()
}

impl Start for KWin {
    fn start(self, monitor: bool) -> anyhow::Result<zbus::blocking::Connection> {
        let conn = zbus::blocking::Connection::session()?;
        let runner = super::kde_krunner::WindowsRunner::new(self.to_niri.clone());
        conn.object_server().at("/KWin", self)?;
        conn.object_server().at("/WindowsRunner", runner)?;
        request_name(&conn, "org.kde.KWin", monitor)?;

        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_ids_read_plain_and_in_braces() {
        assert_eq!(parse_window_id("42"), Some(42));
        assert_eq!(parse_window_id("{42}"), Some(42));
        assert_eq!(parse_window_id("{0b3c-uuid}"), None);
        assert_eq!(parse_window_id(""), None);
    }

    #[test]
    fn the_map_carries_what_spectacle_reads() {
        let info = WindowInfo {
            id: Some(7),
            title: String::from("Kate"),
            app_id: String::from("org.kde.kate"),
            x: 10.,
            y: 20.,
            width: 300.,
            height: 200.,
        };
        let map = info.to_map();
        let string = |key: &str| String::try_from(map[key].clone()).unwrap();
        let double = |key: &str| f64::try_from(map[key].clone()).unwrap();
        assert_eq!(string("uuid"), "7");
        assert_eq!(string("caption"), "Kate");
        assert_eq!(string("desktopFile"), "org.kde.kate");
        assert_eq!(double("x"), 10.);
        assert_eq!(double("height"), 200.);
    }

    #[test]
    fn a_picked_screen_has_no_uuid() {
        let info = WindowInfo {
            id: None,
            title: String::from("DP-1"),
            app_id: String::new(),
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1200.,
        };
        let map = info.to_map();
        assert!(!map.contains_key("uuid"));
        assert_eq!(f64::try_from(map["width"].clone()).unwrap(), 1920.);
    }
}
