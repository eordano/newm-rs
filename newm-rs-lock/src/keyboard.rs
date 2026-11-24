//! Key presses to password input: the keymap from the compositor, then the locale's compose
//! table, so dead keys and compose sequences type the characters they stand for.

use std::ffi::OsString;

use xkbcommon::xkb::{self, compose};

use crate::field::{Field, Key, Outcome};
use crate::secret::wipe;

/// What one key press types. The text is wiped on drop.
#[derive(Debug, PartialEq, Eq)]
enum Typed {
    Text(String),
    Backspace,
    Enter,
    Escape,
}

impl Typed {
    fn key(&self) -> Key<'_> {
        match self {
            Typed::Text(text) => Key::Text(text),
            Typed::Backspace => Key::Backspace,
            Typed::Enter => Key::Enter,
            Typed::Escape => Key::Escape,
        }
    }
}

impl Drop for Typed {
    fn drop(&mut self) {
        if let Typed::Text(text) = self {
            // Zero bytes keep the string valid UTF-8.
            wipe(unsafe { text.as_bytes_mut() });
        }
    }
}

/// The keymap state and, when the locale has a compose table, the compose state.
pub struct Keyboard {
    state: Option<xkb::State>,
    compose: Option<compose::State>,
}

impl Keyboard {
    pub fn new(table: Option<&compose::Table>) -> Self {
        Self {
            state: None,
            compose: table.map(|t| compose::State::new(t, compose::STATE_NO_FLAGS)),
        }
    }

    pub fn set_keymap(&mut self, keymap: &xkb::Keymap) {
        self.state = Some(xkb::State::new(keymap));
        self.reset_compose();
    }

    pub fn update_mask(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(state) = &mut self.state {
            state.update_mask(depressed, latched, locked, 0, 0, group);
        }
    }

    /// Drops a half-typed compose sequence.
    pub fn reset_compose(&mut self) {
        if let Some(compose) = &mut self.compose {
            compose.reset();
        }
    }

    /// Types the key with the given xkb keycode into the field.
    pub fn press(&mut self, field: &mut Field, code: xkb::Keycode) -> Outcome {
        match self.typed(code) {
            Some(typed) => field.key(typed.key()),
            None => Outcome::Ignored,
        }
    }

    fn typed(&mut self, code: xkb::Keycode) -> Option<Typed> {
        let state = self.state.as_ref()?;
        let sym = state.key_get_one_sym(code);
        if let Some(compose) = &mut self.compose {
            if compose.feed(sym) == compose::FeedResult::Accepted {
                match compose.status() {
                    compose::Status::Composing => return None,
                    compose::Status::Composed => {
                        let text = compose
                            .utf8()
                            .or_else(|| compose.keysym().map(xkb::keysym_to_utf8))
                            .unwrap_or_default();
                        compose.reset();
                        return Some(Typed::Text(text));
                    }
                    compose::Status::Cancelled => {
                        compose.reset();
                        return None;
                    }
                    compose::Status::Nothing => {}
                }
            }
        }
        Some(if sym == xkb::Keysym::BackSpace {
            Typed::Backspace
        } else if sym == xkb::Keysym::Return || sym == xkb::Keysym::KP_Enter {
            Typed::Enter
        } else if sym == xkb::Keysym::Escape {
            Typed::Escape
        } else {
            Typed::Text(state.key_get_utf8(code))
        })
    }
}

/// The compose table of the locale in `LC_ALL`, `LC_CTYPE` or `LANG`, else of "C".
pub fn locale_compose_table(context: &xkb::Context) -> Option<compose::Table> {
    let locale = compose_locale(std::env::var_os);
    compose::Table::new_from_locale(context, &locale, compose::COMPILE_NO_FLAGS).ok()
}

fn compose_locale(var: impl Fn(&'static str) -> Option<OsString>) -> OsString {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .filter_map(var)
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "C".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESC: u32 = 9;
    const BKSP: u32 = 22;
    const E: u32 = 26;
    const X: u32 = 53;
    const RTRN: u32 = 36;
    const ACUTE: u32 = 48;
    const SPCE: u32 = 65;

    const KEYMAP: &str = r#"xkb_keymap {
        xkb_keycodes "test" {
            minimum = 8;
            maximum = 255;
            <ESC> = 9;
            <BKSP> = 22;
            <AD03> = 26;
            <RTRN> = 36;
            <AC11> = 48;
            <AB02> = 53;
            <SPCE> = 65;
        };
        xkb_types "test" {
            type "ONE_LEVEL" {
                modifiers = none;
                level_name[Level1] = "Any";
            };
        };
        xkb_compat "test" {};
        xkb_symbols "test" {
            key <ESC> { [ Escape ] };
            key <BKSP> { [ BackSpace ] };
            key <AD03> { [ e ] };
            key <RTRN> { [ Return ] };
            key <AC11> { [ dead_acute ] };
            key <AB02> { [ x ] };
            key <SPCE> { [ space ] };
        };
    };"#;

    const COMPOSE: &str = "<dead_acute> <e> : \"é\" eacute\n\
                           <dead_acute> <space> : \"´\" acute\n";

    fn context() -> xkb::Context {
        xkb::Context::new(xkb::CONTEXT_NO_DEFAULT_INCLUDES | xkb::CONTEXT_NO_ENVIRONMENT_NAMES)
    }

    fn keyboard(compose: bool) -> Keyboard {
        let ctx = context();
        let table = compose.then(|| {
            compose::Table::new_from_buffer(
                &ctx,
                COMPOSE,
                "C",
                compose::FORMAT_TEXT_V1,
                compose::COMPILE_NO_FLAGS,
            )
            .expect("the test compose table compiles")
        });
        let keymap = xkb::Keymap::new_from_string(
            &ctx,
            KEYMAP.to_owned(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("the test keymap compiles");
        let mut kb = Keyboard::new(table.as_ref());
        kb.set_keymap(&keymap);
        kb
    }

    fn typed(kb: &mut Keyboard, code: u32) -> Option<Typed> {
        kb.typed(xkb::Keycode::new(code))
    }

    fn text(s: &str) -> Option<Typed> {
        Some(Typed::Text(s.to_owned()))
    }

    /// Presses the keys into a fresh field, then Enter, and returns the submitted password.
    fn password(kb: &mut Keyboard, codes: &[u32]) -> Vec<u8> {
        let mut field = Field::new();
        for &code in codes {
            kb.press(&mut field, xkb::Keycode::new(code));
        }
        match kb.press(&mut field, xkb::Keycode::new(RTRN)) {
            Outcome::Submit(secret) => secret.as_bytes().to_vec(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn dead_acute_then_e_types_e_acute() {
        let mut kb = keyboard(true);
        assert_eq!(typed(&mut kb, ACUTE), None);
        assert_eq!(typed(&mut kb, E), text("é"));
        assert_eq!(typed(&mut kb, E), text("e"));
        assert_eq!(password(&mut kb, &[E, ACUTE, E, X]), "eéx".as_bytes());
    }

    #[test]
    fn dead_acute_then_space_types_the_acute_accent() {
        let mut kb = keyboard(true);
        assert_eq!(typed(&mut kb, ACUTE), None);
        assert_eq!(typed(&mut kb, SPCE), text("´"));
    }

    #[test]
    fn a_key_outside_the_table_cancels_the_sequence_and_types_nothing() {
        let mut kb = keyboard(true);
        assert_eq!(typed(&mut kb, ACUTE), None);
        assert_eq!(typed(&mut kb, X), None);
        assert_eq!(typed(&mut kb, E), text("e"));
        assert_eq!(typed(&mut kb, ACUTE), None);
        assert_eq!(typed(&mut kb, BKSP), None);
        assert_eq!(typed(&mut kb, BKSP), Some(Typed::Backspace));
        assert_eq!(password(&mut kb, &[E, ACUTE, X, X]), b"ex");
    }

    #[test]
    fn reset_drops_a_half_typed_sequence() {
        let mut kb = keyboard(true);
        assert_eq!(typed(&mut kb, ACUTE), None);
        kb.reset_compose();
        assert_eq!(typed(&mut kb, E), text("e"));
    }

    #[test]
    fn without_a_compose_table_keys_type_their_own_text() {
        let mut kb = keyboard(false);
        assert_eq!(typed(&mut kb, E), text("e"));
        assert_eq!(typed(&mut kb, BKSP), Some(Typed::Backspace));
        assert_eq!(typed(&mut kb, RTRN), Some(Typed::Enter));
        assert_eq!(typed(&mut kb, ESC), Some(Typed::Escape));
        assert_eq!(password(&mut kb, &[ACUTE, E, X]), b"ex");
    }

    #[test]
    fn no_keymap_types_nothing() {
        let mut kb = Keyboard::new(None);
        assert_eq!(typed(&mut kb, E), None);
    }

    #[test]
    fn the_compose_locale_follows_lc_all_then_lc_ctype_then_lang() {
        fn env<'a>(
            pairs: &'a [(&'a str, &'a str)],
        ) -> impl Fn(&'static str) -> Option<OsString> + 'a {
            move |name| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| OsString::from(v))
            }
        }
        let all = [
            ("LC_ALL", "de_DE.UTF-8"),
            ("LC_CTYPE", "fr_FR.UTF-8"),
            ("LANG", "C"),
        ];
        assert_eq!(compose_locale(env(&all)), "de_DE.UTF-8");
        let ctype = [
            ("LC_ALL", ""),
            ("LC_CTYPE", "fr_FR.UTF-8"),
            ("LANG", "en_US.UTF-8"),
        ];
        assert_eq!(compose_locale(env(&ctype)), "fr_FR.UTF-8");
        assert_eq!(
            compose_locale(env(&[("LANG", "pt_BR.UTF-8")])),
            "pt_BR.UTF-8"
        );
        assert_eq!(compose_locale(env(&[])), "C");
    }
}
