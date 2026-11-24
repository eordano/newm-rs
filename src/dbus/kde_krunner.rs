//! KRunner's window search. KWin serves `org.kde.krunner1` at `/WindowsRunner` on `org.kde.KWin`
//! (its `kwin-runner-windows` plugin file tells KRunner so), and newm-rs owns `org.kde.KWin`:
//! without this, every KRunner query logged an UnknownObject error.
//! Matching lives in [`newm_kde::krunner`]; running a match focuses its window the way Alt+Tab
//! does.

use std::collections::HashMap;

use newm_kde::krunner;
use zbus::interface;
use zbus::zvariant::{OwnedValue, Value};

use super::kde_kwin::Request;

pub struct WindowsRunner {
    to_niri: calloop::channel::Sender<Request>,
}

/// One `(sssida{sv})`.
type WireMatch = (
    String,
    String,
    String,
    i32,
    f64,
    HashMap<String, OwnedValue>,
);

fn to_wire(m: krunner::Match) -> WireMatch {
    let mut properties = HashMap::new();
    let mut insert = |key: &str, value: Value<'_>| {
        if let Ok(value) = value.try_to_owned() {
            properties.insert(key.to_owned(), value);
        }
    };
    insert("subtext", Value::from(m.subtext.as_str()));
    insert("category", Value::from("Windows"));
    (
        m.id,
        m.text,
        m.icon,
        m.category_relevance,
        m.relevance,
        properties,
    )
}

#[interface(name = "org.kde.krunner1")]
impl WindowsRunner {
    async fn actions(&self) -> Vec<(String, String, String)> {
        Vec::new()
    }

    async fn config(&self) -> HashMap<String, OwnedValue> {
        let mut config = HashMap::new();
        if let Ok(value) = Value::from(2i32).try_to_owned() {
            config.insert(String::from("MinLetterCount"), value);
        }
        config
    }

    #[zbus(name = "Match")]
    async fn match_query(&self, query: String) -> Vec<WireMatch> {
        let (reply, from_niri) = async_channel::bounded(1);
        if self.to_niri.send(Request::Windows(reply)).is_err() {
            return Vec::new();
        }
        let windows = from_niri.recv().await.unwrap_or_default();
        krunner::matches(&query, &windows)
            .into_iter()
            .map(to_wire)
            .collect()
    }

    async fn run(&self, match_id: String, _action_id: String) {
        if let Some(id) = krunner::window_of(&match_id) {
            if let Err(err) = self.to_niri.send(Request::Activate(id)) {
                warn!("error sending a KRunner window to niri: {err:?}");
            }
        }
    }

    async fn teardown(&self) {}
}

impl WindowsRunner {
    pub fn new(to_niri: calloop::channel::Sender<Request>) -> Self {
        Self { to_niri }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_match_carries_its_subtext_and_category() {
        let m = krunner::Match {
            id: krunner::match_id(3),
            text: String::from("Inbox"),
            icon: String::from("mail"),
            category_relevance: krunner::HIGH,
            relevance: 0.8,
            subtext: String::from("mail"),
        };
        let (id, text, icon, category, relevance, properties) = to_wire(m);
        assert_eq!(
            (id.as_str(), text.as_str(), icon.as_str()),
            ("window-3", "Inbox", "mail")
        );
        assert_eq!((category, relevance), (krunner::HIGH, 0.8));
        assert_eq!(
            String::try_from(properties["subtext"].clone()).unwrap(),
            "mail"
        );
        assert_eq!(
            String::try_from(properties["category"].clone()).unwrap(),
            "Windows"
        );
    }
}
