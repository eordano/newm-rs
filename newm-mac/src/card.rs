//! The parity card: every bind of a live newm-rs `config.kdl` next to the Mac keys newm-mac
//! gives it and its decision. Fails when a Linux bind has no entry in the Mac keymap, or when a
//! Mac-bound combo has no Mac key.

use crate::keys::{self, Action};

/// `(combo, action)` for every line of the `binds {}` block.
pub fn kdl_binds(kdl: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in kdl.lines() {
        if line.starts_with("binds {") {
            inside = true;
            continue;
        }
        if inside && line.starts_with('}') {
            break;
        }
        let t = line.trim();
        if !inside || t.is_empty() || t.starts_with("//") {
            continue;
        }
        let Some(combo) = t.split_whitespace().next() else {
            continue;
        };
        let action = match (t.find('{'), t.rfind('}')) {
            (Some(a), Some(b)) if b > a => t[a + 1..b].trim().trim_end_matches(';').to_owned(),
            _ => String::new(),
        };
        out.push((combo.to_owned(), action));
    }
    out
}

/// Drops `/nix/store/<hash>-` so a command reads the same across deploys.
pub fn unstore(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(k) = rest.find("/nix/store/") {
        out.push_str(&rest[..k]);
        let tail = &rest[k + 11..];
        let skip = tail.find('-').map_or(0, |d| d + 1);
        rest = &tail[skip..];
    }
    out.push_str(rest);
    out
}

pub struct Card {
    pub markdown: String,
    pub missing: Vec<String>,
    pub drift: Vec<String>,
    pub mapped: usize,
    pub native: usize,
    pub linux_only: usize,
}

pub fn card(kdl: &str, keymap: &str) -> Result<Card, String> {
    let entries = keys::parse(keymap)?;
    keys::bindings(&entries)?;
    let binds = kdl_binds(kdl);
    let mut md = String::from(
        "| Linux keys | Mac keys | newm-rs action | Mac decision |\n|---|---|---|---|\n",
    );
    let (mut missing, mut drift) = (Vec::new(), Vec::new());
    let (mut mapped, mut native, mut linux_only) = (0, 0, 0);
    for (combo, action) in &binds {
        let linux = unstore(action).replace('|', "\\|");
        let Some(e) = entries.iter().find(|e| &e.combo == combo) else {
            missing.push(combo.clone());
            md += &format!("| `{combo}` | **MISSING** | `{linux}` | none |\n");
            continue;
        };
        if unstore(&e.linux) != unstore(action) {
            drift.push(format!("{combo}: live `{}` vs keymap `{}`", unstore(action), unstore(&e.linux)));
        }
        let mac = match &e.action {
            Action::Native => {
                native += 1;
                if combo.contains("Alt") {
                    format!("{} (native)", keys::mac_label(combo))
                } else {
                    "(native)".into()
                }
            }
            Action::LinuxOnly => {
                linux_only += 1;
                "—".into()
            }
            _ => {
                mapped += 1;
                keys::mac_label(combo)
            }
        };
        md += &format!(
            "| `{combo}` | {mac} | `{linux}` | {} |\n",
            e.action.describe().replace('|', "\\|")
        );
    }
    Ok(Card {
        markdown: md,
        missing,
        drift,
        mapped,
        native,
        linux_only,
    })
}

pub fn main(args: &[String]) -> i32 {
    let [kdl, keymap, rest @ ..] = args else {
        eprintln!("usage: newm-mac card <config.kdl> <keys.json> [out.md]");
        return 2;
    };
    let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
    let c = match read(kdl).and_then(|k| read(keymap).and_then(|m| card(&k, &m))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("newm-mac card: {e}");
            return 1;
        }
    };
    let total = c.mapped + c.native + c.linux_only + c.missing.len();
    let mut md = format!(
        "# newm-rs → newm-mac keymap card\n\n{total} Linux binds: {} mapped, {} native, {} linux-only, {} missing.\n\
         `Mod` is the left ⌥ key on the Mac (right ⌥ keeps typing characters); a Linux `Alt` bind is the \
         Mac's own ⌘ shortcut and stays native.\n\n",
        c.mapped,
        c.native,
        c.linux_only,
        c.missing.len()
    );
    md += &c.markdown;
    if !c.drift.is_empty() {
        md += "\n## Live config vs generated keymap\n\n";
        for d in &c.drift {
            md += &format!("- {d}\n");
        }
    }
    match rest.first() {
        Some(out) => {
            if let Err(e) = std::fs::write(out, &md) {
                eprintln!("newm-mac card: {out}: {e}");
                return 1;
            }
            println!("{total} binds: {} mapped, {} native, {} linux-only, {} missing", c.mapped, c.native, c.linux_only, c.missing.len());
        }
        None => print!("{md}"),
    }
    if c.missing.is_empty() {
        0
    } else {
        eprintln!("newm-mac card: no Mac decision for {}", c.missing.join(", "));
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_linux_bind_fails() {
        let kdl = "binds {\n    Mod+h { focus-column-left; }\n    Mod+x { spawn-sh \"/nix/store/abc-x/bin/x\"; }\n}\n";
        let km = r#"[{"combo":"Mod+h","linux":"focus-column-left","action":"focus-column-left"}]"#;
        let c = card(kdl, km).unwrap();
        assert_eq!(c.missing, vec!["Mod+x".to_string()]);
        assert_eq!(c.mapped, 1);
        assert_eq!(unstore("spawn-sh \"/nix/store/abc-x/bin/x\""), "spawn-sh \"x/bin/x\"");
    }
}
