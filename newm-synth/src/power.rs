//! Whether the machine runs on battery, from the kernel's power supplies under
//! `/sys/class/power_supply`. Both compositors read it the same way.

use std::fs;
use std::path::Path;

/// Where the kernel lists the power supplies.
pub const SYSFS: &str = "/sys/class/power_supply";

/// Whether the supplies under `root` say the machine runs on battery: it has an external supply
/// (`Mains` or `USB`) and none of them is online. A machine without one (a desktop) never does.
pub fn on_battery(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    let mut external = false;
    for entry in entries.flatten() {
        let dir = entry.path();
        let kind = fs::read_to_string(dir.join("type")).unwrap_or_default();
        if !matches!(kind.trim(), "Mains" | "USB") {
            continue;
        }
        external = true;
        let online = fs::read_to_string(dir.join("online")).unwrap_or_default();
        if online.trim() == "1" {
            return false;
        }
    }
    external
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(root: &Path, name: &str, kind: &str, online: Option<&str>) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("type"), format!("{kind}\n")).unwrap();
        if let Some(online) = online {
            fs::write(dir.join("online"), format!("{online}\n")).unwrap();
        }
    }

    #[test]
    fn reads_the_adapters() {
        let root = std::env::temp_dir().join(format!("newm-power-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(!on_battery(&root), "a desktop has no adapter");
        assert!(!on_battery(&root.join("missing")));

        supply(&root, "BAT0", "Battery", None);
        assert!(!on_battery(&root), "a battery alone says nothing");

        supply(&root, "AC", "Mains", Some("1"));
        assert!(!on_battery(&root));
        supply(&root, "AC", "Mains", Some("0"));
        assert!(on_battery(&root));

        supply(&root, "ucsi-source-psy-USBC000:001", "USB", Some("1"));
        assert!(!on_battery(&root), "charging over USB-C is mains");
        supply(&root, "ucsi-source-psy-USBC000:001", "USB", Some("0"));
        assert!(on_battery(&root));

        fs::remove_dir_all(&root).unwrap();
    }
}
