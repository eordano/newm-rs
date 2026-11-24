//! The password field: typed input, submission and the result of a check.

use crate::secret::Secret;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key<'a> {
    Text(&'a str),
    Backspace,
    Enter,
    Escape,
}

#[derive(Debug)]
pub enum Outcome {
    Ignored,
    Changed,
    Submit(Secret),
    Unlock,
}

#[derive(Debug, Default)]
pub struct Field {
    secret: Secret,
    phase: Phase,
    submitted: usize,
}

impl Field {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn dots(&self) -> usize {
        match self.phase {
            Phase::Checking => self.submitted,
            _ => self.secret.char_count(),
        }
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        if self.phase == Phase::Checking {
            return Outcome::Ignored;
        }
        match key {
            Key::Text(text) => {
                if text.is_empty() || text.chars().any(char::is_control) {
                    return Outcome::Ignored;
                }
                if !self.secret.push_str(text) {
                    return Outcome::Ignored;
                }
                self.phase = Phase::Idle;
                Outcome::Changed
            }
            Key::Backspace => {
                let popped = self.secret.pop_char();
                self.settle(popped)
            }
            Key::Escape => {
                let had = !self.secret.is_empty();
                self.secret.clear();
                self.settle(had)
            }
            Key::Enter => {
                if self.secret.is_empty() {
                    return Outcome::Ignored;
                }
                self.submitted = self.secret.char_count();
                self.phase = Phase::Checking;
                Outcome::Submit(self.secret.take())
            }
        }
    }

    pub fn auth_result(&mut self, ok: bool) -> Outcome {
        if self.phase != Phase::Checking {
            return Outcome::Ignored;
        }
        if ok {
            return Outcome::Unlock;
        }
        self.submitted = 0;
        self.secret.clear();
        self.phase = Phase::Failed;
        Outcome::Changed
    }

    fn settle(&mut self, changed: bool) -> Outcome {
        if changed || self.phase == Phase::Failed {
            self.phase = Phase::Idle;
            Outcome::Changed
        } else {
            Outcome::Ignored
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(field: &mut Field, s: &str) {
        for c in s.chars() {
            let mut buf = [0; 4];
            assert!(matches!(
                field.key(Key::Text(c.encode_utf8(&mut buf))),
                Outcome::Changed
            ));
        }
    }

    fn submit(field: &mut Field) -> Secret {
        match field.key(Key::Enter) {
            Outcome::Submit(secret) => secret,
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn typing_adds_one_dot_per_char() {
        let mut f = Field::new();
        type_str(&mut f, "pä$s");
        assert_eq!(f.dots(), 4);
        assert_eq!(f.phase(), Phase::Idle);
    }

    #[test]
    fn control_and_empty_text_is_ignored() {
        let mut f = Field::new();
        assert!(matches!(f.key(Key::Text("")), Outcome::Ignored));
        assert!(matches!(f.key(Key::Text("\t")), Outcome::Ignored));
        assert!(matches!(f.key(Key::Text("\u{15}")), Outcome::Ignored));
        assert_eq!(f.dots(), 0);
    }

    #[test]
    fn backspace_removes_one_char() {
        let mut f = Field::new();
        type_str(&mut f, "ab€");
        assert!(matches!(f.key(Key::Backspace), Outcome::Changed));
        assert_eq!(f.dots(), 2);
        f.key(Key::Backspace);
        f.key(Key::Backspace);
        assert!(matches!(f.key(Key::Backspace), Outcome::Ignored));
        assert_eq!(f.dots(), 0);
    }

    #[test]
    fn escape_clears_everything() {
        let mut f = Field::new();
        type_str(&mut f, "secret");
        assert!(matches!(f.key(Key::Escape), Outcome::Changed));
        assert_eq!(f.dots(), 0);
        assert!(matches!(f.key(Key::Escape), Outcome::Ignored));
    }

    #[test]
    fn enter_on_empty_does_nothing() {
        let mut f = Field::new();
        assert!(matches!(f.key(Key::Enter), Outcome::Ignored));
        assert_eq!(f.phase(), Phase::Idle);
    }

    #[test]
    fn submit_hands_over_the_password_and_keeps_the_dots() {
        let mut f = Field::new();
        type_str(&mut f, "hunter2");
        let secret = submit(&mut f);
        assert_eq!(secret.as_bytes(), b"hunter2");
        assert_eq!(f.phase(), Phase::Checking);
        assert_eq!(f.dots(), 7);
    }

    #[test]
    fn input_is_ignored_while_checking() {
        let mut f = Field::new();
        type_str(&mut f, "pw");
        let _ = submit(&mut f);
        assert!(matches!(f.key(Key::Text("x")), Outcome::Ignored));
        assert!(matches!(f.key(Key::Backspace), Outcome::Ignored));
        assert!(matches!(f.key(Key::Escape), Outcome::Ignored));
        assert!(matches!(f.key(Key::Enter), Outcome::Ignored));
        assert_eq!(f.dots(), 2);
    }

    #[test]
    fn failure_clears_the_dots_and_marks_failed() {
        let mut f = Field::new();
        type_str(&mut f, "wrong");
        let _ = submit(&mut f);
        assert!(matches!(f.auth_result(false), Outcome::Changed));
        assert_eq!(f.phase(), Phase::Failed);
        assert_eq!(f.dots(), 0);
    }

    #[test]
    fn typing_after_failure_returns_to_idle() {
        let mut f = Field::new();
        type_str(&mut f, "wrong");
        let _ = submit(&mut f);
        f.auth_result(false);
        type_str(&mut f, "r");
        assert_eq!(f.phase(), Phase::Idle);
        assert_eq!(f.dots(), 1);
    }

    #[test]
    fn escape_or_backspace_after_failure_returns_to_idle() {
        let mut f = Field::new();
        type_str(&mut f, "wrong");
        let _ = submit(&mut f);
        f.auth_result(false);
        assert!(matches!(f.key(Key::Escape), Outcome::Changed));
        assert_eq!(f.phase(), Phase::Idle);

        type_str(&mut f, "w");
        let _ = submit(&mut f);
        f.auth_result(false);
        assert!(matches!(f.key(Key::Backspace), Outcome::Changed));
        assert_eq!(f.phase(), Phase::Idle);
    }

    #[test]
    fn success_unlocks() {
        let mut f = Field::new();
        type_str(&mut f, "right");
        let _ = submit(&mut f);
        assert!(matches!(f.auth_result(true), Outcome::Unlock));
    }

    #[test]
    fn stray_results_are_ignored() {
        let mut f = Field::new();
        assert!(matches!(f.auth_result(true), Outcome::Ignored));
        assert!(matches!(f.auth_result(false), Outcome::Ignored));
        assert_eq!(f.phase(), Phase::Idle);
    }
}
