//! KRunner's window search: KWin serves `org.kde.krunner1` at `/WindowsRunner` on
//! `org.kde.KWin`, and KRunner (Plasma's search, Alt+Space) asks it on every keystroke. newm owns
//! `org.kde.KWin`, so it answers in KWin's place: matches by title or app id, run = focus.

/// A window as the search sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app_id: String,
}

/// One `(sssida{sv})` of `Match`: id, text, icon name, category relevance, relevance and the
/// subtext property.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub id: String,
    pub text: String,
    pub icon: String,
    pub category_relevance: i32,
    pub relevance: f64,
    pub subtext: String,
}

/// KRunner's category relevances (`KRunner::QueryMatch::CategoryRelevance`).
pub const HIGHEST: i32 = 100;
pub const HIGH: i32 = 70;
pub const MODERATE: i32 = 50;

const PREFIX: &str = "window-";

/// The words that list every window, as KWin's runner takes them.
const KEYWORDS: &[&str] = &["window", "windows"];

/// The match id of a window.
pub fn match_id(window: u64) -> String {
    format!("{PREFIX}{window}")
}

/// The window a match id names.
pub fn window_of(match_id: &str) -> Option<u64> {
    match_id.strip_prefix(PREFIX)?.parse().ok()
}

/// The windows matching a query, the best first. A query of one character matches nothing
/// (KRunner asks on every keystroke); "window" or "windows" alone lists every window, and
/// "window <term>" searches for the term.
pub fn matches(query: &str, windows: &[Window]) -> Vec<Match> {
    let query = query.trim().to_lowercase();
    let (term, keyword) = match query.split_once(' ') {
        Some((word, rest)) if KEYWORDS.contains(&word) => (rest.trim().to_owned(), true),
        _ if KEYWORDS.contains(&query.as_str()) => (String::new(), true),
        _ => (query, false),
    };
    if term.chars().count() < 2 && !(keyword && term.is_empty()) {
        return Vec::new();
    }
    let mut found: Vec<Match> = windows
        .iter()
        .filter_map(|w| {
            let (category_relevance, relevance) = if term.is_empty() {
                (MODERATE, 0.5)
            } else {
                score(&term, w)?
            };
            Some(Match {
                id: match_id(w.id),
                text: if w.title.is_empty() {
                    w.app_id.clone()
                } else {
                    w.title.clone()
                },
                icon: w.app_id.clone(),
                category_relevance,
                relevance,
                subtext: w.app_id.clone(),
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.relevance
            .total_cmp(&a.relevance)
            .then_with(|| a.id.cmp(&b.id))
    });
    found
}

fn score(term: &str, w: &Window) -> Option<(i32, f64)> {
    let title = w.title.to_lowercase();
    let app = w.app_id.to_lowercase();
    // "org.kde.dolphin" answers "dolphin" as well.
    let app_name = app.rsplit('.').next().unwrap_or(&app).to_owned();
    if title == term || app == term || app_name == term {
        Some((HIGHEST, 1.0))
    } else if title.starts_with(term) || app.starts_with(term) || app_name.starts_with(term) {
        Some((HIGH, 0.8))
    } else if title.contains(term) || app.contains(term) {
        Some((MODERATE, 0.6))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> Vec<Window> {
        vec![
            Window {
                id: 1,
                title: String::from("~/projects — Dolphin"),
                app_id: String::from("org.kde.dolphin"),
            },
            Window {
                id: 2,
                title: String::from("dolphin"),
                app_id: String::from("foot"),
            },
            Window {
                id: 3,
                title: String::from("Inbox - Mail"),
                app_id: String::from("mail"),
            },
            Window {
                id: 4,
                title: String::new(),
                app_id: String::from("org.gnome.Nautilus"),
            },
        ]
    }

    #[test]
    fn an_exact_title_or_app_name_ranks_first() {
        let found = matches("Dolphin", &windows());
        let ids: Vec<_> = found.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["window-1", "window-2"]);
        assert_eq!(found[0].category_relevance, HIGHEST);
        assert_eq!(found[0].subtext, "org.kde.dolphin");
        assert_eq!(found[0].icon, "org.kde.dolphin");
    }

    #[test]
    fn prefixes_and_substrings_rank_below() {
        let found = matches("mai", &windows());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].category_relevance, HIGH);
        let found = matches("inbox", &windows());
        assert_eq!(found[0].category_relevance, HIGH);
        let found = matches("ail", &windows());
        assert_eq!(found[0].category_relevance, MODERATE);
    }

    #[test]
    fn short_queries_match_nothing_and_the_keyword_lists_all() {
        assert!(matches("d", &windows()).is_empty());
        assert!(matches("  ", &windows()).is_empty());
        assert_eq!(matches("windows", &windows()).len(), 4);
        let found = matches("window nautilus", &windows());
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].text, "org.gnome.Nautilus",
            "an untitled window shows its app id"
        );
    }

    #[test]
    fn match_ids_name_their_window() {
        assert_eq!(window_of(&match_id(42)), Some(42));
        assert_eq!(window_of("window-x"), None);
        assert_eq!(window_of("42"), None);
    }
}
