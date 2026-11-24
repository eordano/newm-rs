//! Command line parsing.

use std::path::PathBuf;

use crate::render::{Colors, Rgb};

pub const USAGE: &str = "usage: newm-rs-lock [-i|--wallpaper PATH] [--bg #RRGGBB] [--fg #RRGGBB] \
                         [--accent #RRGGBB] [--error #RRGGBB] [-f|--daemonize]";

#[derive(Debug, PartialEq)]
pub struct Options {
    pub wallpaper: Option<PathBuf>,
    pub colors: Colors,
    pub daemonize: bool,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Lock(Options),
    Help,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, String> {
    let mut opts = Options {
        wallpaper: None,
        colors: Colors::default(),
        daemonize: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => {
                (name.to_owned(), Some(value.to_owned()))
            }
            _ => (arg.clone(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or(format!("{name} needs a value"))
        };
        match name.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-f" | "--daemonize" if inline.is_none() => opts.daemonize = true,
            "-i" | "--wallpaper" => opts.wallpaper = Some(PathBuf::from(value()?)),
            "--bg" => opts.colors.bg = color(&name, &value()?)?,
            "--fg" => opts.colors.fg = color(&name, &value()?)?,
            "--accent" => opts.colors.accent = color(&name, &value()?)?,
            "--error" => opts.colors.error = color(&name, &value()?)?,
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(Command::Lock(opts))
}

fn color(name: &str, value: &str) -> Result<Rgb, String> {
    Rgb::parse(value).ok_or(format!("{name}: expected #RRGGBB, got {value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    fn opts(args: &[&str]) -> Options {
        match run(args) {
            Ok(Command::Lock(o)) => o,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn defaults() {
        let o = opts(&[]);
        assert_eq!(
            o,
            Options {
                wallpaper: None,
                colors: Colors::default(),
                daemonize: false
            }
        );
    }

    #[test]
    fn all_options_both_spellings() {
        let o = opts(&[
            "--wallpaper",
            "/w.png",
            "--bg=#000000",
            "--fg",
            "ffffff",
            "--accent=#112233",
            "--error",
            "#445566",
            "-f",
        ]);
        assert_eq!(o.wallpaper.as_deref(), Some(std::path::Path::new("/w.png")));
        assert_eq!(o.colors.bg, Rgb(0, 0, 0));
        assert_eq!(o.colors.fg, Rgb(255, 255, 255));
        assert_eq!(o.colors.accent, Rgb(0x11, 0x22, 0x33));
        assert_eq!(o.colors.error, Rgb(0x44, 0x55, 0x66));
        assert!(o.daemonize);
        assert_eq!(
            opts(&["-i", "a.png", "--daemonize"]).wallpaper,
            Some(PathBuf::from("a.png"))
        );
    }

    #[test]
    fn errors() {
        assert!(run(&["--bg"]).is_err());
        assert!(run(&["--bg", "red"]).is_err());
        assert!(run(&["--nope"]).is_err());
        assert!(run(&["--daemonize=yes"]).is_err());
        assert!(run(&["extra"]).is_err());
    }

    #[test]
    fn help() {
        assert_eq!(run(&["--bg", "#000000", "-h"]), Ok(Command::Help));
    }
}
