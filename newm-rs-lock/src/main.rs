//! newm-rs-lock: a screen locker for newm-rs over ext-session-lock-v1.
//!
//! Shows the wallpaper and a password field drawn as dots on every output; the password is
//! checked through the PAM service `newm-lock`. It never exits while the session is locked
//! except after a successful unlock, so a crash leaves the session locked.

mod app;
mod args;
mod auth;
mod field;
mod hint;
mod keyboard;
mod pam;
mod render;
mod secret;
mod wallpaper;

use std::process::ExitCode;
use std::sync::Arc;

const PAM_SERVICE: &str = "newm-lock";

fn main() -> ExitCode {
    let opts = match args::parse(std::env::args().skip(1)) {
        Ok(args::Command::Lock(opts)) => opts,
        Ok(args::Command::Help) => {
            println!("{}", args::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("newm-rs-lock: {e}\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };
    let auth = match pam::Pam::new(PAM_SERVICE) {
        Ok(pam) => Arc::new(pam),
        Err(e) => {
            eprintln!("newm-rs-lock: {e}");
            return ExitCode::FAILURE;
        }
    };
    let wallpaper = opts.wallpaper.as_deref().and_then(|path| {
        wallpaper::load_png(path, opts.colors.bg)
            .inspect_err(|e| eprintln!("newm-rs-lock: {e}; using the background colour"))
            .ok()
    });
    match app::run(opts, wallpaper, auth) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("newm-rs-lock: {e}");
            ExitCode::FAILURE
        }
    }
}
