//! logind's `LockedHint` on the caller's session, so `loginctl` and session watchers (idle
//! daemons, screen-time trackers) see the lock that ext-session-lock-v1 keeps to the compositor.

use std::process::{Command, Stdio};

fn args(locked: bool) -> [&'static str; 8] {
    [
        "--system",
        "call",
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
        "SetLockedHint",
        "b",
        if locked { "true" } else { "false" },
    ]
}

pub fn set(locked: bool) {
    let status = Command::new("busctl")
        .args(args(locked))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("newm-rs-lock: busctl SetLockedHint {locked}: {s}"),
        Err(e) => eprintln!("newm-rs-lock: cannot run busctl for SetLockedHint: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::args;

    #[test]
    fn the_hint_goes_to_the_callers_session() {
        let on = args(true);
        assert_eq!(on[0], "--system");
        assert_eq!(on[3], "/org/freedesktop/login1/session/auto");
        assert_eq!(on[5..], ["SetLockedHint", "b", "true"]);
        assert_eq!(args(false)[7], "false");
    }
}
