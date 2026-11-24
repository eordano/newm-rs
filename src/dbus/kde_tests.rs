//! The KDE services on a private bus, called in the shapes KDE's clients send: KGlobalAccel (KF6
//! kglobalaccel), the KDE Inhibit portal (PolicyAgent), KRunner and screen saver clients. Each
//! test starts its own `dbus-daemon`; without one on PATH it says so and passes.

use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use newm_kde::kglobalaccel::{self as reg, Registry};
use smithay::backend::input::InputTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue};

use super::freedesktop_screensaver::{Activity, ScreenSaver};
use super::kde_kglobalaccel::{KGlobalAccel, Shortcuts, ToNiri, KDE_ACTION};
use super::kde_krunner::WindowsRunner;
use super::kde_kwin::Request;
use super::kde_policy_agent::{PolicyAgent, AGENT_NAME, POWERDEVIL_NAME};

/// `KGlobalShortcutInfo` as `(ssssssaiai)`.
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

struct Bus {
    daemon: Child,
    address: String,
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn private_bus() -> Option<Bus> {
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address"])
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let mut line = String::new();
    BufReader::new(daemon.stdout.as_mut()?)
        .read_line(&mut line)
        .ok()?;
    Some(Bus {
        daemon,
        address: line.trim().to_owned(),
    })
}

fn connect(bus: &Bus) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .unwrap()
}

fn call<B>(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> zbus::Message
where
    B: serde::Serialize + DynamicType,
{
    conn.call_method(Some(dest), path, Some(iface), method, body)
        .unwrap_or_else(|err| panic!("{iface}.{method}: {err}"))
}

/// The next message of a signal stream, or `None` after `wait`.
fn next_signal(
    mut stream: zbus::blocking::proxy::SignalIterator<'static>,
    wait: Duration,
) -> Option<zbus::Message> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(stream.next());
    });
    rx.recv_timeout(wait).ok().flatten()
}

macro_rules! bus_or_skip {
    () => {
        match private_bus() {
            Some(bus) => bus,
            None => {
                eprintln!("skipped: no dbus-daemon on PATH");
                return;
            }
        }
    };
}

#[test]
fn a_kde_app_registers_its_shortcut_and_hears_the_press() {
    let bus = bus_or_skip!();
    let server = connect(&bus);
    let registry = Arc::new(Mutex::new(Registry::default()));
    registry.lock().unwrap().set_reserved([0x53 | 0x1000_0000]); // the config's Mod+S
    let (to_niri, from_service) = calloop::channel::channel();
    let service = KGlobalAccel::new(registry.clone(), to_niri);
    service.serve(&server).unwrap();
    server.request_name("org.kde.kglobalaccel").unwrap();

    let app = connect(&bus);
    let accel = |method: &str, body: &dyn erased::Body| body.call(&app, method);
    let id: Vec<String> = [
        "org.kde.spectacle.desktop",
        "FullScreenScreenShot",
        "Spectacle",
        "Capture Entire Desktop",
    ]
    .map(String::from)
    .to_vec();
    let print: Vec<(Vec<i32>,)> = vec![(vec![0x0100_0009, 0, 0, 0],)];

    // KGlobalAccel::setGlobalShortcut: register, set the default, set autoloading and present.
    accel("doRegister", &(id.clone(),));
    accel(
        "setShortcutKeys",
        &(id.clone(), print.clone(), reg::IS_DEFAULT),
    );
    let reply = accel(
        "setShortcutKeys",
        &(id.clone(), print.clone(), reg::SET_PRESENT),
    );
    let got: Vec<(Vec<i32>,)> = reply.body().deserialize().unwrap();
    assert_eq!(got, print);

    // A key the config binds comes back unassigned.
    let other: Vec<String> = ["kate", "find", "Kate", "Find"].map(String::from).to_vec();
    let mod_s: Vec<(Vec<i32>,)> = vec![(vec![0x53 | 0x1000_0000, 0, 0, 0],)];
    accel("doRegister", &(other.clone(),));
    let reply = accel("setShortcutKeys", &(other, mod_s, reg::SET_PRESENT));
    let got: Vec<(Vec<i32>,)> = reply.body().deserialize().unwrap();
    assert_eq!(got, vec![(vec![0, 0, 0, 0],)]);

    let reply = accel("getComponent", &("org.kde.spectacle.desktop",));
    let path: OwnedObjectPath = reply.body().deserialize().unwrap();
    assert_eq!(path.as_str(), "/component/org_kde_spectacle_desktop");
    let err = app
        .call_method(
            Some("org.kde.kglobalaccel"),
            "/kglobalaccel",
            Some("org.kde.KGlobalAccel"),
            "getComponent",
            &("nobody",),
        )
        .unwrap_err();
    assert!(err.to_string().contains("NoSuchComponent"), "{err}");

    // The component object answers as kglobalacceld's does.
    let component = Proxy::new(
        &app,
        "org.kde.kglobalaccel",
        path.as_str().to_owned(),
        "org.kde.kglobalaccel.Component",
    )
    .unwrap();
    let friendly: String = component.get_property("friendlyName").unwrap();
    assert_eq!(friendly, "Spectacle");
    let names: Vec<String> = component.call("shortcutNames", &("default",)).unwrap();
    assert_eq!(names, vec!["FullScreenScreenShot"]);
    let infos: Vec<WireInfo> = component.call("allShortcutInfos", &("default",)).unwrap();
    assert_eq!(infos[0].6, vec![0x0100_0009]);
    // Qt's no-context overloads, which System Settings' Shortcuts page (kcm_keys) calls.
    let names: Vec<String> = component.call("shortcutNames", &()).unwrap();
    assert_eq!(names, vec!["FullScreenScreenShot"]);
    let infos: Vec<WireInfo> = component.call("allShortcutInfos", &()).unwrap();
    assert_eq!(infos[0].6, vec![0x0100_0009]);
    let active: bool = component.call("isActive", &()).unwrap();
    assert!(active);

    // niri was handed the bindings: the Print key for Spectacle, nothing for Kate.
    let mut event_loop = calloop::EventLoop::<reg::Snapshot>::try_new().unwrap();
    event_loop
        .handle()
        .insert_source(from_service, |event, _, last| {
            if let calloop::channel::Event::Msg(ToNiri::Bindings(b)) = event {
                *last = b;
            }
        })
        .unwrap();
    let mut snapshot = reg::Snapshot::default();
    event_loop
        .dispatch(Duration::from_millis(200), &mut snapshot)
        .unwrap();
    assert_eq!(snapshot.bindings.len(), 1);
    assert_eq!(snapshot.bindings[0].key, 0x0100_0009);

    // A press of the bound key reaches the app as globalShortcutPressed on its component.
    let stream = component.receive_signal("globalShortcutPressed").unwrap();
    let mut shortcuts = Shortcuts::default();
    shortcuts.conn = Some(server.clone());
    shortcuts.set_bindings(snapshot);
    shortcuts.activate(KDE_ACTION, true, InputTime::now());
    let msg = next_signal(stream, Duration::from_secs(5)).expect("globalShortcutPressed");
    let (c, a, _t): (String, String, i64) = msg.body().deserialize().unwrap();
    assert_eq!(
        (c.as_str(), a.as_str()),
        ("org.kde.spectacle.desktop", "FullScreenScreenShot")
    );
}

#[test]
fn the_inhibit_portals_policy_call_holds_idle_off_until_released() {
    let bus = bus_or_skip!();
    let server = connect(&bus);
    let inhibited = Arc::new(AtomicBool::new(false));
    let activity = Arc::new(Activity::default());
    let (to_niri, _from) = calloop::channel::channel();
    let ss = ScreenSaver::new(inhibited.clone(), activity.clone(), to_niri);
    server
        .object_server()
        .at("/org/freedesktop/ScreenSaver", ss.clone())
        .unwrap();
    server
        .object_server()
        .at(
            "/org/kde/Solid/PowerManagement/PolicyAgent",
            PolicyAgent::new(ss),
        )
        .unwrap();
    server.request_name(AGENT_NAME).unwrap();
    server.request_name(POWERDEVIL_NAME).unwrap();
    server.request_name("org.freedesktop.ScreenSaver").unwrap();

    let portal = connect(&bus);
    // The portal calls PowerDevil's main name (xdg-desktop-portal-kde inhibit.cpp).
    let agent = |method: &str, body: &dyn erased::Body| {
        body.call_at(
            &portal,
            POWERDEVIL_NAME,
            "/org/kde/Solid/PowerManagement/PolicyAgent",
            "org.kde.Solid.PowerManagement.PolicyAgent",
            method,
        )
    };
    // xdg-desktop-portal-kde: Idle -> ChangeScreenSettings (4).
    let reply = agent(
        "AddInhibition",
        &(
            4u32,
            String::from("org.gnome.Totem"),
            String::from("Playing"),
        ),
    );
    let cookie: u32 = reply.body().deserialize().unwrap();
    assert!(cookie != 0);
    assert!(inhibited.load(Ordering::SeqCst));
    let reply = agent("HasInhibition", &(4u32,));
    assert!(reply.body().deserialize::<bool>().unwrap());
    // A profile change alone holds nothing off.
    let reply = agent(
        "AddInhibition",
        &(2u32, String::from("app"), String::from("profile")),
    );
    let profile: u32 = reply.body().deserialize().unwrap();
    assert_ne!(profile, cookie);
    let reply = agent("ListInhibitions", &());
    assert_eq!(
        reply
            .body()
            .deserialize::<Vec<(String, String)>>()
            .unwrap()
            .len(),
        2
    );
    agent("ReleaseInhibition", &(cookie,));
    assert!(!inhibited.load(Ordering::SeqCst));

    // The screen saver answers the lock state niri keeps.
    let saver = |method: &str| {
        call(
            &portal,
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
            method,
            &(),
        )
    };
    assert!(!saver("GetActive").body().deserialize::<bool>().unwrap());
    activity.set_locked(true);
    assert!(saver("GetActive").body().deserialize::<bool>().unwrap());
    assert_eq!(
        saver("GetActiveTime").body().deserialize::<u32>().unwrap(),
        0
    );
    assert!(saver("GetSessionIdleTime")
        .body()
        .deserialize::<u32>()
        .is_ok());
}

#[test]
fn active_changed_reaches_screen_saver_clients() {
    let bus = bus_or_skip!();
    let server = connect(&bus);
    let (to_niri, _from) = calloop::channel::channel();
    let ss = ScreenSaver::new(Arc::default(), Arc::default(), to_niri);
    server
        .object_server()
        .at("/org/freedesktop/ScreenSaver", ss)
        .unwrap();
    server.request_name("org.freedesktop.ScreenSaver").unwrap();
    let client = connect(&bus);
    let proxy = Proxy::new(
        &client,
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
    )
    .unwrap();
    let stream = proxy.receive_signal("ActiveChanged").unwrap();
    super::freedesktop_screensaver::emit_active_changed(&server, true);
    let msg = next_signal(stream, Duration::from_secs(5)).expect("ActiveChanged");
    assert!(msg.body().deserialize::<bool>().unwrap());
}

#[test]
fn krunner_finds_a_window_and_running_it_focuses_it() {
    let bus = bus_or_skip!();
    let server = connect(&bus);
    let (to_niri, from_runner) = calloop::channel::channel::<Request>();
    server
        .object_server()
        .at("/WindowsRunner", WindowsRunner::new(to_niri))
        .unwrap();
    server.request_name("org.kde.KWin").unwrap();

    // niri's side: answers the window list and records activations.
    let activated = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));
    let niri = {
        let activated = activated.clone();
        let done = done.clone();
        thread::spawn(move || {
            let mut event_loop = calloop::EventLoop::<()>::try_new().unwrap();
            event_loop
                .handle()
                .insert_source(from_runner, move |event, _, ()| {
                    if let calloop::channel::Event::Msg(request) = event {
                        match request {
                            Request::Windows(reply) => {
                                let _ = reply.send_blocking(vec![newm_kde::krunner::Window {
                                    id: 7,
                                    title: String::from("Inbox - Mail"),
                                    app_id: String::from("mail"),
                                }]);
                            }
                            Request::Activate(id) => activated.lock().unwrap().push(id),
                            _ => (),
                        }
                    }
                })
                .unwrap();
            while !done.load(Ordering::SeqCst) {
                event_loop
                    .dispatch(Duration::from_millis(50), &mut ())
                    .unwrap();
            }
        })
    };

    let krunner = connect(&bus);
    let runner = |method: &str, body: &dyn erased::Body| {
        body.call_at(
            &krunner,
            "org.kde.KWin",
            "/WindowsRunner",
            "org.kde.krunner1",
            method,
        )
    };
    let reply = runner("Match", &(String::from("mail"),));
    type WireMatch = (
        String,
        String,
        String,
        i32,
        f64,
        std::collections::HashMap<String, OwnedValue>,
    );
    let found: Vec<WireMatch> = reply.body().deserialize().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "window-7");
    assert_eq!(found[0].1, "Inbox - Mail");
    let actions: Vec<(String, String, String)> =
        runner("Actions", &()).body().deserialize().unwrap();
    assert!(actions.is_empty());
    runner("Run", &(String::from("window-7"), String::new()));
    thread::sleep(Duration::from_millis(200));
    done.store(true, Ordering::SeqCst);
    niri.join().unwrap();
    assert_eq!(*activated.lock().unwrap(), vec![7]);
}

/// Method calls with bodies of any shape behind one closure type.
mod erased {
    use zbus::blocking::Connection;

    pub trait Body {
        fn call(&self, conn: &Connection, method: &str) -> zbus::Message {
            self.call_at(
                conn,
                "org.kde.kglobalaccel",
                "/kglobalaccel",
                "org.kde.KGlobalAccel",
                method,
            )
        }
        fn call_at(
            &self,
            conn: &Connection,
            dest: &str,
            path: &str,
            iface: &str,
            method: &str,
        ) -> zbus::Message;
    }

    impl<B: serde::Serialize + zbus::zvariant::DynamicType> Body for B {
        fn call_at(
            &self,
            conn: &Connection,
            dest: &str,
            path: &str,
            iface: &str,
            method: &str,
        ) -> zbus::Message {
            super::call(conn, dest, path, iface, method, self)
        }
    }
}
