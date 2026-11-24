//! Restored windows find their saved spots by title. A window takes the placeholder whose saved
//! title is closest to its own when it maps, but many apps title their windows only afterwards:
//! a terminal once tmux or ssh attaches, a browser once its restored tab loads. Until
//! [`REMATCH_FOR`] after a restore starts, a window whose title changes trades spots with the
//! placeholder or restored window of its app that fits the titles better.

use std::collections::BTreeSet;
use std::time::Duration;

use newm_canvas::Id;
use newm_kde::apps;
use serde_json::json;

use super::{now_ms, CanvasSpace, LayoutElement};

/// How long after a restore starts its windows may still trade spots as their titles arrive: an
/// ssh window can wait for a security key touch.
pub const REMATCH_FOR: Duration = Duration::from_secs(90);

/// The score from which a title picks a placeholder at map time rather than the app alone.
pub const TITLE_MATCH: f64 = 0.5;

/// How much better a trade must fit the titles than the spots held now.
const MARGIN: f64 = 0.25;

/// The saved spot a restored window took: its app and the title saved with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub app_id: String,
    pub title: Option<String>,
}

/// How well `now`, a window's title, fits `saved`, the title saved with a spot: 1 when they are
/// equal, else at most 0.99 times the share of their lowercase alphanumeric words they have in
/// common, so a spinner glyph, an unread count or a "1 new item" still finds the spot.
pub fn title_score(saved: Option<&str>, now: Option<&str>) -> f64 {
    let (Some(saved), Some(now)) = (saved, now) else {
        return 0.;
    };
    if saved == now {
        return 1.;
    }
    let words = |s: &str| -> BTreeSet<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let (a, b) = (words(saved), words(now));
    let union = a.union(&b).count();
    if union == 0 {
        return 0.;
    }
    0.99 * a.intersection(&b).count() as f64 / union as f64
}

/// What a retitled window trades its spot with.
#[derive(Debug, Clone, Copy)]
enum Trade {
    Placeholder(usize),
    Window(usize),
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// The window `window` got a new title: during a restore, it trades spots with the
    /// placeholder or restored window of its app whose saved title fits it better (see the
    /// module docs).
    pub fn retitled(&mut self, window: &W::Id) {
        let _span = tracy_client::span!("CanvasSpace::retitled");
        let now = self.clock.now_unadjusted();
        if self.rematch_until.is_none_or(|until| now > until) {
            self.rematch_until = None;
            return;
        }
        let Some(x) = self.idx_of(window) else {
            return;
        };
        let Some(app) = self.entries[x].tile.window().app_id() else {
            return;
        };
        let title = self.entries[x].tile.window().title();
        let title = title.as_deref();
        let slot_title = |slot: &Option<Slot>| slot.as_ref().and_then(|s| s.title.clone());
        let sx = slot_title(&self.entries[x].slot);
        let cur = title_score(sx.as_deref(), title);
        if cur >= 1. {
            return;
        }

        let mut best: Option<(Trade, f64)> = None;
        let mut consider = |trade: Trade, gain: f64| {
            if gain > MARGIN && best.is_none_or(|(_, g)| gain > g) {
                best = Some((trade, gain));
            }
        };
        for (k, p) in self.placeholders.iter().enumerate() {
            if apps::same(&p.app_id, &app) {
                consider(
                    Trade::Placeholder(k),
                    title_score(p.title.as_deref(), title) - cur,
                );
            }
        }
        for (y, e) in self.entries.iter().enumerate() {
            let Some(slot) = &e.slot else {
                continue;
            };
            if y == x || !apps::same(&slot.app_id, &app) {
                continue;
            }
            let other = e.tile.window().title();
            let other = other.as_deref();
            let sy = slot.title.as_deref();
            let gain = title_score(sy, title) + title_score(sx.as_deref(), other)
                - cur
                - title_score(sy, other);
            consider(Trade::Window(y), gain);
        }
        let Some((trade, gain)) = best else {
            return;
        };

        let ix = self.entries[x].id;
        match trade {
            Trade::Window(y) => {
                let iy = self.entries[y].id;
                self.swap_spots(ix, iy);
                let (a, b) = (self.entries[x].slot.take(), self.entries[y].slot.take());
                self.entries[x].slot = b;
                self.entries[y].slot = a;
                self.retarget_focus(ix, iy);
            }
            Trade::Placeholder(k) => {
                let pid = self.placeholders[k].id;
                self.swap_spots(ix, pid);
                let p = &mut self.placeholders[k];
                let took = Slot {
                    app_id: p.app_id.clone(),
                    title: p.title.take(),
                };
                match self.entries[x].slot.replace(took) {
                    // The placeholder now waits on the saved spot the window left.
                    Some(left) => {
                        p.app_id = left.app_id;
                        p.title = left.title;
                        self.retarget_focus(ix, pid);
                    }
                    // The window left a spot no session saved: nothing waits there.
                    None => {
                        self.placeholders.remove(k);
                        if let Some(restore) = &mut self.restore {
                            restore.waiting.retain(|&w| w != pid);
                            if restore.focus == Some(pid) {
                                restore.focus = Some(ix);
                            }
                        }
                        self.apply_for("rematch", |c| c.remove(pid));
                    }
                }
            }
        }
        self.emit(json!({
            "t": now_ms(),
            "op": "restore-rematch",
            "app": apps::canonical(&app),
            "with": match trade { Trade::Window(_) => "window", Trade::Placeholder(_) => "placeholder" },
            "gain": (gain * 100.).round() / 100.,
        }));
        self.update_tile_sizes(true, None);
        self.finish_restore();
    }

    /// Exchanges the tiles of canvas ids `a` and `b`, each keeping its own minimum and the
    /// focus staying on its window.
    fn swap_spots(&mut self, a: Id, b: Id) {
        let tmp = self.next_id;
        self.next_id += 1;
        let min = |s: &Self, id: Id| {
            s.entries
                .iter()
                .find(|e| e.id == id)
                .map_or((0., 0.), |e| s.min_of(e))
        };
        let (min_a, min_b) = (min(self, a), min(self, b));
        let focus = self.canvas.focus();
        self.apply_for("rematch", |c| {
            c.rename(a, tmp);
            c.rename(b, a);
            c.rename(tmp, b);
            c.set_min(a, min_a.0, min_a.1);
            c.set_min(b, min_b.0, min_b.1);
            if let Some(f) = focus.filter(|&f| f == a || f == b) {
                c.focus_id(f);
            }
        });
    }

    /// The saved focus follows the saved spot: when `a` and `b` traded spots, a restore that
    /// focuses one focuses the other.
    fn retarget_focus(&mut self, a: Id, b: Id) {
        if let Some(restore) = &mut self.restore {
            if restore.focus == Some(a) {
                restore.focus = Some(b);
            } else if restore.focus == Some(b) {
                restore.focus = Some(a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_titles_score_one_and_only_they_do() {
        assert_eq!(title_score(Some("desk: ~"), Some("desk: ~")), 1.);
        assert!(title_score(Some("a b"), Some("b a")) < 1.);
        assert_eq!(title_score(None, Some("a")), 0.);
        assert_eq!(title_score(Some("a"), None), 0.);
        assert_eq!(title_score(Some("--"), Some("::")), 0.);
    }

    #[test]
    fn a_changed_marker_still_fits_better_than_another_window() {
        let saved = "* Alex Doe (DM) - Acme - Chat - Chromium";
        let same = "Alex Doe (DM) - Acme - 1 new item - Chat - Chromium";
        let other = "Sam Roe (DM) - Acme - Chat - Chromium";
        let page = "refactor(parser): read config files lazily - Chromium";
        assert!(title_score(Some(saved), Some(same)) >= TITLE_MATCH);
        assert!(title_score(Some(saved), Some(same)) > title_score(Some(saved), Some(other)));
        assert!(title_score(Some(saved), Some(page)) < TITLE_MATCH);
        let spinner = title_score(Some("desk: ✳ todo-app"), Some("desk: ✶ todo-app"));
        assert!(spinner > 0.9, "{spinner}");
    }
}
