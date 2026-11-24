//! The canvas side of a left-Option mouse drag, apart from Accessibility so it is testable
//! anywhere: what a drop (left button) or an edge resize (right button) at a pointer position
//! does to the canvas. A window that leaves the canvas mid-drag (it closes, or floats) makes
//! every answer `None` or `false`.

use newm_canvas::{Canvas, Edge, Id};

#[derive(Debug, Clone, Copy)]
pub struct Grab {
    pub id: Id,
    /// The right button resizes; the left one moves.
    pub right: bool,
    /// Where the button went down.
    pub start: (f64, f64),
    /// The edges a resize moves: the ones nearest the press.
    pub edges: (Edge, Edge),
}

impl Grab {
    /// The target of a drop at `p`: the cell a move goes to, or the tile steps a resize takes.
    fn target(&self, canvas: &Canvas, p: (f64, f64)) -> Option<(i32, i32)> {
        let r = canvas.rect(self.id)?;
        let (tw, th) = canvas.tile_size();
        let (gx, gy) = (((p.0 - self.start.0) / tw).round() as i32, ((p.1 - self.start.1) / th).round() as i32);
        Some(if self.right {
            (
                if self.edges.0 == Edge::Left { -gx } else { gx },
                if self.edges.1 == Edge::Top { -gy } else { gy },
            )
        } else {
            (r.i + gx, r.j + gy)
        })
    }

    /// Moves or resizes the window on `canvas` to `t`; whether the canvas changed.
    fn make(&self, canvas: &mut Canvas, t: (i32, i32)) -> bool {
        let Some(r) = canvas.rect(self.id) else {
            return false;
        };
        if self.right {
            let mut ok = false;
            if t.0 != 0 {
                ok |= canvas.resize(self.id, self.edges.0, t.0);
            }
            if t.1 != 0 {
                ok |= canvas.resize(self.id, self.edges.1, t.1);
            }
            ok
        } else {
            t != (r.i, r.j) && canvas.move_to(self.id, t.0, t.1)
        }
    }

    /// The target of a drop at `p` and the layout it makes, without making it.
    pub fn layout(&self, canvas: &Canvas, p: (f64, f64)) -> Option<((i32, i32), Canvas)> {
        let t = self.target(canvas, p)?;
        let mut c = canvas.clone();
        self.make(&mut c, t);
        Some((t, c))
    }

    /// Drops at `p`; whether the canvas changed.
    pub fn drop_on(&self, canvas: &mut Canvas, p: (f64, f64)) -> bool {
        self.target(canvas, p).is_some_and(|t| self.make(canvas, t))
    }
}

#[cfg(test)]
mod tests {
    use newm_canvas::Config;

    use super::*;

    fn canvas() -> Canvas {
        let config = Config {
            tile_w: 100.0,
            tile_h: 100.0,
            ..Config::default()
        };
        let mut c = Canvas::new(config, 1000.0, 800.0);
        c.add(1, 1.0, 1.0, None);
        c.add(2, 1.0, 1.0, None);
        c
    }

    fn grab(right: bool) -> Grab {
        Grab {
            id: 1,
            right,
            start: (50.0, 50.0),
            edges: (Edge::Right, Edge::Bottom),
        }
    }

    #[test]
    fn a_drop_moves_and_a_right_drag_resizes() {
        let mut c = canvas();
        let r = c.rect(1).unwrap();
        let far = (50.0 + 400.0, 50.0 + 300.0);
        let (t, preview) = grab(false).layout(&c, far).unwrap();
        assert_eq!(t, (r.i + 4, r.j + 3));
        assert_eq!(c.rect(1), Some(r), "the preview leaves the canvas alone");
        assert!(grab(false).drop_on(&mut c, far));
        assert_eq!(c.rect(1), preview.rect(1));

        let (t, _) = grab(true).layout(&c, (150.0, 50.0)).unwrap();
        assert_eq!(t, (1, 0));
    }

    #[test]
    fn a_window_gone_mid_drag_ends_it_quietly() {
        let mut c = canvas();
        c.remove(1);
        let before = c.clone();
        for right in [false, true] {
            let g = grab(right);
            assert!(g.layout(&c, (450.0, 350.0)).is_none());
            assert!(!g.drop_on(&mut c, (450.0, 350.0)));
        }
        assert_eq!(c.rect(2), before.rect(2), "the canvas stays as the close left it");
    }
}
