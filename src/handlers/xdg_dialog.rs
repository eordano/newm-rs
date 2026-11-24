//! xdg-dialog-v1: a modal dialog keeps the keyboard focus while it is up. Dialogs (toplevels with
//! a parent) already open floating, centred on their parent; a modal one also takes the focus
//! whenever the layout would give it to the parent (a click, Alt+Tab, IPC, keyboard navigation),
//! and closing it gives the focus back to the parent.

use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::dialog::{ToplevelDialogHint, XdgDialogHandler};
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData};

use crate::niri::State;

/// Dialogs nest (a dialog's own dialog): how deep the focus follows them.
const MAX_DEPTH: usize = 8;

impl XdgDialogHandler for State {
    fn dialog_hint_changed(&mut self, _toplevel: ToplevelSurface, _hint: ToplevelDialogHint) {
        // The next keyboard focus update moves the focus (see `redirect_focus_to_modal_dialog`).
        self.niri.queue_redraw_all();
    }
}

/// Whether the toplevel is a modal dialog (xdg_dialog_v1.set_modal).
pub fn is_modal(toplevel: &ToplevelSurface) -> bool {
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .is_some_and(|data| data.lock().unwrap().dialog_hint == ToplevelDialogHint::Modal)
    })
}

impl State {
    /// Moves the layout's focus from a window with a modal dialog up to that dialog (the newest
    /// such dialog, and that one's own, down the chain).
    pub fn redirect_focus_to_modal_dialog(&mut self) {
        for _ in 0..MAX_DEPTH {
            let Some(focused) = self.niri.layout.focus() else {
                return;
            };
            let parent = focused.toplevel().wl_surface().clone();
            let dialog = self
                .niri
                .layout
                .windows()
                .filter(|(_, m)| {
                    m.toplevel().parent().as_ref() == Some(&parent) && is_modal(m.toplevel())
                })
                .map(|(_, m)| (m.id(), m.window.clone()))
                .max_by_key(|(id, _)| id.get())
                .map(|(_, window)| window);
            let Some(dialog) = dialog else {
                return;
            };
            self.niri.layout.activate_window(&dialog);
        }
    }

    /// Gives the focus back to the parent of a modal dialog that closed while it had it.
    pub fn focus_dialog_parent(&mut self, parent: &WlSurface) {
        let window = self
            .niri
            .layout
            .find_window_and_output(parent)
            .map(|(mapped, _)| mapped.window.clone());
        if let Some(window) = window {
            self.niri.layout.activate_window(&window);
        }
    }
}
