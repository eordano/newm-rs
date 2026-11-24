//! What pointer motion with the left Option key held does, apart from the event tap so it is
//! testable anywhere: nothing within `PAN_THRESHOLD` of its anchor, past it a pan that starts
//! at the anchor. The anchor is where left Option went down, or where the last mouse button
//! came up, so motion after a left-Option drag measures from the drop and not from the press.

/// Pointer motion with left Option held pans past this many points.
const PAN_THRESHOLD: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanEv {
    Start((f64, f64)),
    Move((f64, f64)),
    End,
}

#[derive(Debug, Default)]
pub struct OptPointer {
    /// Left Option is down.
    pub held: bool,
    anchor: (f64, f64),
    panning: bool,
}

impl OptPointer {
    /// Left Option goes down or up with the pointer at `p`; releasing it ends a pan.
    pub fn opt(&mut self, down: bool, p: (f64, f64)) -> Option<PanEv> {
        if down && !self.held {
            self.anchor = p;
            self.panning = false;
        }
        self.held = down;
        if down { None } else { self.stop() }
    }

    /// A mouse button goes down: a pan in progress ends.
    pub fn stop(&mut self) -> Option<PanEv> {
        std::mem::take(&mut self.panning).then_some(PanEv::End)
    }

    /// A mouse button comes up at `p`: the next pan measures from there.
    pub fn release(&mut self, p: (f64, f64)) {
        self.anchor = p;
        self.panning = false;
    }

    /// The pointer moves to `p` with no button down: the pan events that makes.
    pub fn moved(&mut self, p: (f64, f64)) -> impl Iterator<Item = PanEv> + use<> {
        let mut start = None;
        if self.held && !self.panning {
            let (dx, dy) = (p.0 - self.anchor.0, p.1 - self.anchor.1);
            if dx.hypot(dy) >= PAN_THRESHOLD {
                self.panning = true;
                start = Some(PanEv::Start(self.anchor));
            }
        }
        let step = self.panning.then_some(PanEv::Move(p));
        start.into_iter().chain(step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(o: &mut OptPointer, p: (f64, f64)) -> Vec<PanEv> {
        o.moved(p).collect()
    }

    #[test]
    fn a_pan_starts_past_the_threshold_at_the_press() {
        let mut o = OptPointer::default();
        assert!(moved(&mut o, (50.0, 0.0)).is_empty(), "no pan without left Option");
        assert_eq!(o.opt(true, (0.0, 0.0)), None);
        assert!(moved(&mut o, (5.0, 0.0)).is_empty());
        assert_eq!(moved(&mut o, (10.0, 0.0)), [PanEv::Start((0.0, 0.0)), PanEv::Move((10.0, 0.0))]);
        assert_eq!(moved(&mut o, (11.0, 0.0)), [PanEv::Move((11.0, 0.0))]);
        assert_eq!(o.opt(false, (11.0, 0.0)), Some(PanEv::End));
        assert!(moved(&mut o, (40.0, 0.0)).is_empty());
    }

    #[test]
    fn motion_after_a_drag_measures_from_the_drop() {
        let mut o = OptPointer::default();
        o.opt(true, (0.0, 0.0));
        assert_eq!(o.stop(), None);
        o.release((300.0, 0.0));
        assert!(moved(&mut o, (303.0, 0.0)).is_empty(), "a small motion after the drop is no pan");
        assert_eq!(moved(&mut o, (310.0, 0.0)), [PanEv::Start((300.0, 0.0)), PanEv::Move((310.0, 0.0))]);
    }

    #[test]
    fn a_button_ends_a_pan() {
        let mut o = OptPointer::default();
        o.opt(true, (0.0, 0.0));
        moved(&mut o, (20.0, 0.0));
        assert_eq!(o.stop(), Some(PanEv::End));
        assert_eq!(o.stop(), None);
    }
}
