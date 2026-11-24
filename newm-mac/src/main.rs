//! newm-mac: newm-rs's tile canvas (`newm-canvas`) driving real macOS windows through the
//! Accessibility API, with newm's keys on the left Option key.
//!
//! ```text
//! newm-mac [run] [--keys PATH] [--tile WxH] [--top PT] [--bottom PT] [--state PATH]
//! newm-mac card <config.kdl> <keys.json> [out.md]
//! newm-mac frames
//! newm-mac inject key|hold|rkey|plain|cmd|drag|pan|wheel|text ...
//! ```

mod card;
#[cfg(any(target_os = "macos", test))]
mod children;
#[cfg(any(target_os = "macos", test))]
mod drag;
mod keys;
#[cfg(any(target_os = "macos", test))]
mod pointer;
#[cfg(any(target_os = "macos", test))]
mod usage;

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod inject;
#[cfg(target_os = "macos")]
mod tap;
#[cfg(target_os = "macos")]
mod wm;

#[cfg(target_os = "macos")]
fn run(args: &[String]) -> i32 {
    use std::path::PathBuf;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let terminal = std::env::var("NEWM_MAC_TERMINAL").unwrap_or_else(|_| {
        if std::path::Path::new("/Applications/Ghostty.app").exists() {
            "com.mitchellh.ghostty".into()
        } else {
            "com.apple.Terminal".into()
        }
    });
    let mut opts = wm::Opts {
        keys: home.join(".config/newm-mac/keys.json"),
        tile: (500.0, 380.0),
        top: 30.0,
        bottom: 0.0,
        state: None,
        terminal,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--keys" => opts.keys = v.into(),
            "--state" => opts.state = Some(v.into()),
            "--top" => opts.top = v.parse().unwrap_or(30.0),
            "--bottom" => opts.bottom = v.parse().unwrap_or(0.0),
            "--tile" => {
                if let Some((w, h)) = v.split_once('x') {
                    opts.tile = (w.parse().unwrap_or(500.0), h.parse().unwrap_or(380.0));
                }
            }
            _ => {
                eprintln!("newm-mac: unknown option {a}");
                return 2;
            }
        }
    }
    tap::run(opts)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    let code = match args.first().map(String::as_str) {
        Some("card") => card::main(rest),
        #[cfg(target_os = "macos")]
        Some("frames") => ax::print_frames(),
        #[cfg(target_os = "macos")]
        Some("inject") => inject::main(rest),
        #[cfg(target_os = "macos")]
        Some("run") => run(rest),
        #[cfg(target_os = "macos")]
        _ => run(&args),
        #[cfg(not(target_os = "macos"))]
        _ => {
            eprintln!("newm-mac runs on macOS; elsewhere only `card` works");
            2
        }
    };
    std::process::exit(code);
}
