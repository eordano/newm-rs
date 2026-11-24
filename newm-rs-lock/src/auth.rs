//! Password checks run off the Wayland thread, behind a trait so tests can fake them.

use std::sync::Arc;
use std::{io, thread};

use crate::secret::Secret;

pub trait Authenticator: Send + Sync {
    fn authenticate(&self, password: &[u8]) -> bool;
}

pub fn spawn_check<F>(auth: Arc<dyn Authenticator>, password: Secret, done: F) -> io::Result<()>
where
    F: FnOnce(bool) + Send + 'static,
{
    thread::Builder::new().name("auth".into()).spawn(move || {
        let ok = auth.authenticate(password.as_bytes());
        drop(password);
        done(ok);
    })?;
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::field::{Field, Key, Outcome, Phase};

    pub struct FakeAuth {
        pub password: &'static [u8],
        pub seen: Mutex<Vec<Vec<u8>>>,
    }

    impl Authenticator for FakeAuth {
        fn authenticate(&self, password: &[u8]) -> bool {
            self.seen.lock().unwrap().push(password.to_vec());
            password == self.password
        }
    }

    fn fake(password: &'static [u8]) -> Arc<FakeAuth> {
        Arc::new(FakeAuth {
            password,
            seen: Mutex::new(Vec::new()),
        })
    }

    fn attempt(field: &mut Field, auth: &Arc<FakeAuth>, typed: &str) -> Outcome {
        for c in typed.chars() {
            let mut buf = [0; 4];
            field.key(Key::Text(c.encode_utf8(&mut buf)));
        }
        let Outcome::Submit(secret) = field.key(Key::Enter) else {
            panic!("no submit");
        };
        let (tx, rx) = mpsc::channel();
        spawn_check(auth.clone(), secret, move |ok| tx.send(ok).unwrap()).unwrap();
        assert_eq!(field.phase(), Phase::Checking);
        let ok = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        field.auth_result(ok)
    }

    #[test]
    fn wrong_then_right_password() {
        let auth = fake(b"correct horse");
        let mut field = Field::new();

        assert!(matches!(
            attempt(&mut field, &auth, "battery"),
            Outcome::Changed
        ));
        assert_eq!(field.phase(), Phase::Failed);
        assert_eq!(field.dots(), 0);

        assert!(matches!(
            attempt(&mut field, &auth, "correct horse"),
            Outcome::Unlock
        ));

        let seen = auth.seen.lock().unwrap();
        assert_eq!(*seen, vec![b"battery".to_vec(), b"correct horse".to_vec()]);
    }

    #[test]
    fn non_ascii_password_reaches_the_authenticator_as_utf8() {
        let auth = fake("pässwörd".as_bytes());
        let mut field = Field::new();
        assert!(matches!(
            attempt(&mut field, &auth, "pässwörd"),
            Outcome::Unlock
        ));
    }
}
