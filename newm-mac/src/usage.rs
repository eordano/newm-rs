//! The usage log, `~/.local/state/newm-mac/usage.jsonl`, for tuning placement: one JSON line
//! per action, naming the app and never a window title. Only a build with the `telemetry`
//! feature has it; without the feature nothing is written and no file or directory is made.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

pub const TELEMETRY: bool = cfg!(feature = "telemetry");

/// One line of the log: when, what happened, to which app.
pub fn line(op: &str, app: Option<&str>, extra: Value) -> Value {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    json!({"t": t, "op": op, "app": app, "extra": extra})
}

/// Without the `telemetry` feature the log is compiled out: nothing is written.
#[cfg(not(feature = "telemetry"))]
pub struct Log;

#[cfg(not(feature = "telemetry"))]
impl Log {
    pub fn open(_home: Option<&std::path::Path>) -> Log {
        Log
    }

    pub fn write(&mut self, _line: &Value) {}
}

#[cfg(feature = "telemetry")]
pub use file::Log;

/// The log file, built with the `telemetry` feature only.
#[cfg(feature = "telemetry")]
mod file {
    use std::fs::File;
    use std::io::Write as _;
    use std::path::Path;

    use serde_json::Value;

    pub struct Log(Option<File>);

    impl Log {
        /// Opens (appending) the log under `home`; a failure leaves the log off.
        pub fn open(home: Option<&Path>) -> Log {
            Log(home.and_then(|h| {
                let dir = h.join(".local/state/newm-mac");
                std::fs::create_dir_all(&dir).ok()?;
                File::options().create(true).append(true).open(dir.join("usage.jsonl")).ok()
            }))
        }

        pub fn write(&mut self, line: &Value) {
            if let Some(f) = self.0.as_mut() {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_is_written_only_with_telemetry() {
        let home = std::env::temp_dir().join(format!("newm-mac-usage-test-{}", std::process::id()));
        let mut log = Log::open(Some(&home));
        log.write(&line("open", Some("Finder"), json!({"startup": true})));
        if TELEMETRY {
            let text = std::fs::read_to_string(home.join(".local/state/newm-mac/usage.jsonl")).unwrap();
            let v: Value = serde_json::from_str(text.trim()).unwrap();
            assert_eq!((v["op"].as_str(), v["app"].as_str()), (Some("open"), Some("Finder")));
            assert_eq!(v["extra"]["startup"], true);
            std::fs::remove_dir_all(&home).unwrap();
        } else {
            assert!(!home.exists(), "nothing is written, not even the directory");
        }
    }
}
