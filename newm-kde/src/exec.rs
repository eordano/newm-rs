//! A desktop entry's `Exec` value as the program and its arguments, read as the XDG Desktop Entry
//! Specification's "The Exec key" says, for a launch that opens no files or URLs.
//!
//! The order is the spec's: the string escapes of the value (`\s`, `\n`, `\t`, `\r`, `\\`) come
//! off first, then the quoting, then each argument's field codes expand.

/// What the field codes `%i`, `%c` and `%k` expand to: the entry's `Icon` and `Name` values and
/// the desktop file's location, each with its string escapes undone ([`unescape`]). A missing
/// one expands to nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fields<'a> {
    pub icon: Option<&'a str>,
    pub name: Option<&'a str>,
    pub location: Option<&'a str>,
}

/// A desktop entry string value with its escapes undone: `\s` is a space, `\n`, `\t` and `\r` the
/// control characters, `\\` one backslash. Any other backslash stays for the quoting rules.
pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// The arguments of `exec`, with no files to open:
/// - A double-quoted argument keeps its spaces; inside the quotes `\"`, `` \` ``, `\$` and `\\` are
///   the character itself. Outside quotes a backslash makes the next character literal.
/// - `%f`, `%F`, `%u`, `%U` and the deprecated `%d`, `%D`, `%n`, `%N`, `%v`, `%m` expand to
///   nothing; `%c` to the name and `%k` to the location; `%i` alone as an argument to `--icon` and
///   the icon (nothing without one), inside a longer argument to nothing; `%%` to `%`.
/// - An argument that field codes empty is dropped (`app %U` is `app`); a field code inside a word
///   leaves the rest of it (`--file=%f` is `--file=`).
///
/// A value the spec calls invalid, an open quote or a field code it does not list, has no
/// arguments: it must not run.
pub fn argv(exec: &str, fields: &Fields<'_>) -> Vec<String> {
    let Some(words) = split(&unescape(exec)) else {
        return Vec::new();
    };
    let mut args = Vec::new();
    for word in words {
        if word == "%i" {
            if let Some(icon) = fields.icon.filter(|icon| !icon.is_empty()) {
                args.push(String::from("--icon"));
                args.push(icon.to_owned());
            }
            continue;
        }
        let Some((arg, expanded)) = expand(&word, fields) else {
            return Vec::new();
        };
        if expanded && arg.is_empty() {
            continue;
        }
        args.push(arg);
    }
    args
}

/// The words of an unescaped value with their quoting undone; `None` for an open quote.
fn split(value: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let (mut word, mut started, mut quoted) = (String::new(), false, false);
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.next()? {
                e @ ('"' | '`' | '$' | '\\') => word.push(e),
                other => {
                    word.push('\\');
                    word.push(other);
                }
            },
            '\\' => {
                word.push(chars.next().unwrap_or('\\'));
                started = true;
            }
            ' ' | '\t' | '\n' if !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

/// `word` with its field codes expanded, and whether it had any; `None` for an unknown code.
fn expand(word: &str, fields: &Fields<'_>) -> Option<(String, bool)> {
    let mut out = String::with_capacity(word.len());
    let mut expanded = false;
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('f' | 'F' | 'u' | 'U' | 'i' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {
                expanded = true
            }
            Some('c') => {
                out.push_str(fields.name.unwrap_or_default());
                expanded = true;
            }
            Some('k') => {
                out.push_str(fields.location.unwrap_or_default());
                expanded = true;
            }
            Some(code) if code.is_ascii_alphabetic() => return None,
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    Some((out, expanded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(exec: &str) -> Vec<String> {
        argv(exec, &Fields::default())
    }

    #[test]
    fn spaces_split_and_double_quotes_keep_an_argument_whole() {
        assert_eq!(plain("app  -a\tb"), ["app", "-a", "b"]);
        assert_eq!(plain(r#"app "a b" "" c"#), ["app", "a b", "", "c"]);
        assert_eq!(plain(r#"app --title="a b""#), ["app", "--title=a b"]);
        assert_eq!(
            plain("app 'a b'"),
            ["app", "'a", "b'"],
            "single quotes are not quoting"
        );
    }

    #[test]
    fn quoted_arguments_undo_their_four_escapes() {
        // In the file: "\\"q\\" \\` \\$HOME \\\\ \\x" (string escapes first, then quoting).
        assert_eq!(
            plain(r#"sh "\\"q\\" \\` \\$HOME \\\\ \\x""#),
            ["sh", r#""q" ` $HOME \ \x"#]
        );
        // A single backslash before a quote survives the string escapes, then quotes it.
        assert_eq!(plain(r#"say "\"hi\"""#), ["say", "\"hi\""]);
        // Outside quotes a backslash makes the next character literal.
        assert_eq!(plain(r"a\\ b"), ["a b"]);
    }

    #[test]
    fn string_escapes_come_off_before_the_quoting() {
        assert_eq!(plain(r#""a\sb" c\sd"#), ["a b", "c", "d"]);
        assert_eq!(plain(r#"printf "x\ty""#), ["printf", "x\ty"]);
        assert_eq!(unescape(r"a\sb\\c\nd\q"), "a b\\c\nd\\q");
    }

    #[test]
    fn file_and_url_codes_and_deprecated_codes_expand_to_nothing() {
        assert_eq!(plain("app %f %F %u %U"), ["app"]);
        assert_eq!(plain("app %d %D %n %N %v %m"), ["app"]);
        assert_eq!(plain("app --file=%f x%Uy"), ["app", "--file=", "xy"]);
        assert_eq!(plain("%F"), Vec::<String>::new());
    }

    #[test]
    fn a_double_percent_is_one_literal_percent() {
        assert_eq!(plain("app 100%% %%f"), ["app", "100%", "%f"]);
        assert_eq!(plain(r#"app "50%%""#), ["app", "50%"]);
    }

    #[test]
    fn icon_name_and_location_expand_from_the_entry() {
        let fields = Fields {
            icon: Some("org.app.Icon"),
            name: Some("My App"),
            location: Some("/apps/org.app.desktop"),
        };
        assert_eq!(
            argv("app %i --name=%c %k", &fields),
            [
                "app",
                "--icon",
                "org.app.Icon",
                "--name=My App",
                "/apps/org.app.desktop"
            ]
        );
        assert_eq!(argv("app x%i %c", &fields), ["app", "x", "My App"]);
        assert_eq!(plain("app %i %c %k --name=%c"), ["app", "--name="]);
        let no_icon = Fields {
            icon: Some(""),
            ..fields
        };
        assert_eq!(argv("app %i", &no_icon), ["app"]);
    }

    #[test]
    fn an_open_quote_or_an_unknown_field_code_runs_nothing() {
        assert!(plain(r#"app "a b"#).is_empty());
        assert!(plain("app %x").is_empty());
        assert_eq!(
            plain("app %1 50%"),
            ["app", "%1", "50%"],
            "not a field code"
        );
    }
}
