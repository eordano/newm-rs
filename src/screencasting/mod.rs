use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::time::Duration;

use anyhow::{ensure, Context as _};
use calloop::LoopHandle;
use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::renderer::element::utils::{
    CropRenderElement, Relocate, RelocateRenderElement, RescaleRenderElement,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::Window;
use smithay::output::Output;
use smithay::reexports::gbm::Modifier;
use smithay::reexports::wayland_server::{Client, DisplayHandle};
use smithay::utils::{DeviceFd, Logical, Physical, Point, Rectangle, Scale, Size};
use zbus::object_server::SignalEmitter;

use crate::dbus::kde_kwin::parse_window_id;
use crate::dbus::mutter_screen_cast::{self, CursorMode, ScreenCastToNiri, StreamTargetId};
use crate::niri::{
    scale_relocate_crop, CastTarget, Niri, OutputRenderElements, PointerRenderElements, State,
};
use crate::niri_render_elements;
use crate::protocols::kde_screencast::{
    KdeCastSource, KdePointer, KdeScreencastHandler, KdeScreencastManagerState,
};
use crate::render_helpers::{RenderCtx, RenderTarget};
use crate::utils::{get_monotonic_time, CastSessionId, CastStreamId};
use crate::window::mapped::{MappedId, WindowCastRenderElements};

mod pw_utils;
use pw_utils::{Cast, CastSizeChange, CursorData, NodeAnnouncer, PipeWire, PwToNiri};

pub struct Screencasting {
    pub casts: Vec<Cast>,

    /// Dynamic-target casts waiting for their first target to start.
    pub pending_dynamic_casts: Vec<PendingCast>,

    pub pw_to_niri: calloop::channel::Sender<PwToNiri>,

    /// Screencast output for each mapped window.
    pub mapped_cast_output: HashMap<Window, Output>,

    /// Window ID for the "dynamic cast" special window for the xdp-gnome picker.
    pub dynamic_cast_id_for_portal: MappedId,

    /// Streams asked for over zkde_screencast_unstable_v1, the way Spectacle records.
    pub kde: KdeScreencastManagerState,

    // Drop PipeWire last, and specifically after casts, to prevent a double-free (yay).
    pub pipewire: Option<PipeWire>,
}

/// A screencast request that hasn't been started yet.
pub struct PendingCast {
    pub session_id: CastSessionId,
    pub stream_id: CastStreamId,
    pub cursor_mode: CursorMode,
    pub signal_ctx: SignalEmitter<'static>,
}

impl Screencasting {
    pub fn new<F>(
        event_loop: &LoopHandle<'static, State>,
        display: &DisplayHandle,
        filter: F,
    ) -> Self
    where
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let pw_to_niri = {
            let (pw_to_niri, from_pipewire) = calloop::channel::channel();
            event_loop
                .insert_source(from_pipewire, move |event, _, state| match event {
                    calloop::channel::Event::Msg(msg) => state.on_pw_msg(msg),
                    calloop::channel::Event::Closed => (),
                })
                .unwrap();
            pw_to_niri
        };

        Self {
            casts: vec![],
            pending_dynamic_casts: vec![],
            pw_to_niri,
            mapped_cast_output: HashMap::new(),
            dynamic_cast_id_for_portal: MappedId::next(),
            kde: KdeScreencastManagerState::new::<State, _>(display, filter),
            pipewire: None,
        }
    }
}

impl State {
    fn prepare_pw_cast(&mut self) -> anyhow::Result<Option<(GbmDevice<DeviceFd>, FormatSet)>> {
        // Ensure PipeWire is initialized.
        if self.niri.casting.pipewire.is_none() {
            let pw = PipeWire::new(
                self.niri.event_loop.clone(),
                self.niri.casting.pw_to_niri.clone(),
            )
            .context("error initializing PipeWire")?;
            self.niri.casting.pipewire = Some(pw);
        }

        if self.niri.config.borrow().debug.disable_pipewire_dmabuf {
            return Ok(None);
        }

        let Some(gbm) = self.backend.gbm_device() else {
            // We will offer shm only.
            return Ok(None);
        };

        let mut render_formats = self
            .backend
            .with_primary_renderer(|renderer| {
                renderer.egl_context().dmabuf_render_formats().clone()
            })
            .unwrap_or_default();

        {
            let config = self.niri.config.borrow();
            if config.debug.force_pipewire_invalid_modifier {
                render_formats = render_formats
                    .into_iter()
                    .filter(|f| f.modifier == Modifier::Invalid)
                    .collect();
            }
        }

        Ok(Some((gbm, render_formats)))
    }

    pub fn on_pw_msg(&mut self, msg: PwToNiri) {
        match msg {
            PwToNiri::StopCast { session_id } => self.niri.stop_cast(session_id),
            PwToNiri::Redraw { stream_id } => self.redraw_cast(stream_id),
            PwToNiri::FatalError => {
                warn!("stopping PipeWire due to fatal error");
                let casting = &mut self.niri.casting;
                if let Some(pw) = casting.pipewire.take() {
                    let mut ids = HashSet::new();
                    for cast in &casting.pending_dynamic_casts {
                        ids.insert(cast.session_id);
                    }
                    for cast in &casting.casts {
                        ids.insert(cast.session_id);
                    }
                    for id in ids {
                        self.niri.stop_cast(id);
                    }
                    self.niri.event_loop.remove(pw.token);
                }
            }
        }
    }

    fn redraw_cast(&mut self, stream_id: CastStreamId) {
        let _span = tracy_client::span!("State::redraw_cast");

        let casts = &mut self.niri.casting.casts;
        let Some(idx) = casts.iter().position(|cast| cast.stream_id == stream_id) else {
            warn!("cast to redraw is missing");
            return;
        };
        let cast = &mut casts[idx];

        let id = match &cast.target {
            CastTarget::Nothing => {
                self.backend.with_primary_renderer(|renderer| {
                    if cast.dequeue_buffer_and_clear(renderer) {
                        cast.record_frame_time(get_monotonic_time());
                    }
                });
                return;
            }
            CastTarget::Output { output, .. } => {
                if let Some(output) = output.upgrade() {
                    self.niri.queue_redraw(&output);
                }
                return;
            }
            CastTarget::Region { rect, .. } => {
                let rect = *rect;
                for output in self.niri.region_cast_outputs(rect) {
                    self.niri.queue_redraw(&output);
                }
                return;
            }
            CastTarget::Window { id } => MappedId::specific(*id),
        };

        // Lack of partial borrowing strikes again...
        let mut casts = mem::take(&mut self.niri.casting.casts);
        let cast = &mut casts[idx];
        let mut stop = false;
        // Use a loop {} so we can break instead of early-return.
        #[allow(clippy::never_loop)]
        #[allow(clippy::while_let_loop)]
        loop {
            let Some((mapped, _)) = self.niri.find_mapped_output_by_id(id) else {
                break;
            };

            // Use the cached output since it will be present even if the output was
            // currently disconnected.
            let Some(output) = self.niri.casting.mapped_cast_output.get(&mapped.window) else {
                break;
            };

            let scale = Scale::from(output.current_scale().fractional_scale());
            let bbox = mapped.capture_bbox(scale);

            match cast.ensure_size(bbox.size) {
                Ok(CastSizeChange::Ready) => (),
                Ok(CastSizeChange::Pending) => break,
                Err(err) => {
                    warn!("error updating stream size, stopping screencast: {err:?}");
                    stop = true;
                    break;
                }
            }

            self.backend.with_primary_renderer(|renderer| {
                let mut elements = Vec::new();
                let mut pointer_location = Point::default();

                if self.niri.pointer_visibility.is_visible() {
                    if let Some((pointer_pos, win_pos)) =
                        self.niri.pointer_pos_for_window_cast(mapped)
                    {
                        // Pointer location must be relative to the screencast buffer.
                        // - win_pos is the position of the main window surface in output-local
                        //   coordinates
                        // - bbox.loc moves us relative to the screencast buffer
                        let buf_pos = win_pos + bbox.loc.to_f64().to_logical(scale);
                        let output_pos =
                            self.niri.global_space.output_geometry(output).unwrap().loc;
                        pointer_location = pointer_pos - output_pos.to_f64() - buf_pos;

                        let pos = buf_pos.to_physical_precise_round(scale).upscale(-1);
                        self.niri.render_pointer(renderer, output, &mut |elem| {
                            let elem =
                                RelocateRenderElement::from_element(elem, pos, Relocate::Relative);
                            elements.push(CastRenderElement::from(elem));
                        });
                    }
                }

                let ctx = RenderCtx {
                    renderer,
                    target: RenderTarget::Screencast,
                    xray: None,
                };
                let main_start = elements.len();
                mapped.render_for_screen_cast(ctx, scale, &mut |elem| {
                    elements.push(CastRenderElement::from(elem))
                });

                let cursor_data =
                    CursorData::compute(&elements, main_start, pointer_location, scale);

                if cast.dequeue_buffer_and_render(
                    renderer,
                    &elements,
                    &cursor_data,
                    bbox.size,
                    scale,
                ) {
                    cast.record_frame_time(get_monotonic_time());
                }
            });

            break;
        }
        let session_id = cast.session_id;
        self.niri.casting.casts = casts;

        if stop {
            self.niri.stop_cast(session_id);
        }
    }

    pub fn set_dynamic_cast_target(&mut self, target: CastTarget) {
        let _span = tracy_client::span!("State::set_dynamic_cast_target");

        let mut refresh = None;
        match &target {
            // Leave refresh as is when clearing. Chances are, the next refresh will match it,
            // then we'll avoid reconfiguring.
            CastTarget::Nothing => (),
            CastTarget::Output { output, .. } => {
                if let Some(output) = output.upgrade() {
                    refresh = Some(output.current_mode().unwrap().refresh as u32);
                }
            }
            CastTarget::Window { id } => {
                let mut windows = self.niri.layout.windows();
                if let Some((_, mapped)) = windows.find(|(_, mapped)| mapped.id().get() == *id) {
                    if let Some(output) = self.niri.casting.mapped_cast_output.get(&mapped.window) {
                        refresh = Some(output.current_mode().unwrap().refresh as u32);
                    }
                }
            }
            CastTarget::Region { .. } => (),
        }

        let mut to_redraw = Vec::new();
        let mut to_stop = Vec::new();
        for cast in &mut self.niri.casting.casts {
            if !cast.dynamic_target {
                continue;
            }

            if let Some(refresh) = refresh {
                if let Err(err) = cast.set_refresh(refresh) {
                    warn!("error changing cast FPS: {err:?}");
                    to_stop.push(cast.session_id);
                    continue;
                }
            }

            cast.target = target.clone();
            to_redraw.push(cast.stream_id);
        }

        for id in to_redraw {
            self.redraw_cast(id);
        }

        // Start any pending dynamic casts if we have a real target.
        if !matches!(target, CastTarget::Nothing) {
            self.start_pending_dynamic_casts(&target);
        }
    }

    fn start_pending_dynamic_casts(&mut self, target: &CastTarget) {
        let pending = &self.niri.casting.pending_dynamic_casts;
        if pending.is_empty() {
            return;
        }
        debug!("starting {} pending dynamic cast(s)", pending.len());

        let _span = tracy_client::span!("State::start_pending_dynamic_casts");

        // We don't stop dynamic casts on missing output/window.
        let (size, refresh) = match target {
            CastTarget::Nothing => panic!("dynamic cast starting target must not be Nothing"),
            CastTarget::Output { output, .. } => {
                let Some(output) = output.upgrade() else {
                    return;
                };
                cast_params_for_output(&output)
            }
            CastTarget::Window { id } => {
                let Some((size, refresh)) = self.niri.cast_params_for_window(*id) else {
                    return;
                };
                (size, refresh)
            }
            CastTarget::Region { .. } => return,
        };

        let gbm = match self.prepare_pw_cast() {
            Ok(x) => x,
            Err(err) => {
                warn!("error starting pending screencasts: {err:?}");
                let mut ids = HashSet::new();
                for pending in self.niri.casting.pending_dynamic_casts.drain(..) {
                    ids.insert(pending.session_id);
                }
                for id in ids {
                    self.niri.stop_cast(id);
                }
                return;
            }
        };
        let pw = self.niri.casting.pipewire.as_ref().unwrap();

        // Alpha is always true since the dynamic target can change between window & output.
        let alpha = true;

        // Start each pending cast.
        let mut to_stop = HashSet::new();
        for pending in self.niri.casting.pending_dynamic_casts.drain(..) {
            let res = pw.start_cast(
                gbm.clone(),
                pending.session_id,
                pending.stream_id,
                target.clone(),
                size,
                refresh,
                alpha,
                pending.cursor_mode,
                announce_to_mutter(pending.signal_ctx),
            );
            match res {
                Ok(mut cast) => {
                    cast.dynamic_target = true;
                    self.niri.casting.casts.push(cast);
                }
                Err(err) => {
                    warn!("error starting pending screencast: {err:?}");
                    to_stop.insert(pending.session_id);
                }
            }
        }

        for session_id in to_stop {
            self.niri.stop_cast(session_id);
        }
    }

    pub fn on_screen_cast_msg(&mut self, msg: ScreenCastToNiri) {
        match msg {
            ScreenCastToNiri::StartCast {
                session_id,
                stream_id,
                target,
                cursor_mode,
                signal_ctx,
            } => {
                let _span = tracy_client::span!("StartCast");
                let _span = debug_span!("StartCast", %session_id, %stream_id).entered();

                let (target, size, refresh, alpha) = match target {
                    StreamTargetId::Output { name } => {
                        let global_space = &self.niri.global_space;
                        let output = global_space.outputs().find(|out| out.name() == name);
                        let Some(output) = output else {
                            warn!("error starting screencast: requested output is missing");
                            self.niri.stop_cast(session_id);
                            return;
                        };

                        let (size, refresh) = cast_params_for_output(output);
                        (CastTarget::output(output), size, refresh, false)
                    }
                    StreamTargetId::Window { id }
                        if id == self.niri.casting.dynamic_cast_id_for_portal.get() =>
                    {
                        debug!("delaying dynamic cast until target is set");
                        self.niri.casting.pending_dynamic_casts.push(PendingCast {
                            session_id,
                            stream_id,
                            cursor_mode,
                            signal_ctx,
                        });
                        return;
                    }
                    StreamTargetId::Window { id } => {
                        let Some((size, refresh)) = self.niri.cast_params_for_window(id) else {
                            warn!("error starting screencast: requested window is missing");
                            self.niri.stop_cast(session_id);
                            return;
                        };
                        (CastTarget::Window { id }, size, refresh, true)
                    }
                };

                let gbm = match self.prepare_pw_cast() {
                    Ok(x) => x,
                    Err(err) => {
                        warn!("error starting screencast: {err:?}");
                        self.niri.stop_cast(session_id);
                        return;
                    }
                };
                let pw = self.niri.casting.pipewire.as_ref().unwrap();

                let res = pw.start_cast(
                    gbm,
                    session_id,
                    stream_id,
                    target,
                    size,
                    refresh,
                    alpha,
                    cursor_mode,
                    announce_to_mutter(signal_ctx),
                );
                match res {
                    Ok(cast) => {
                        self.niri.casting.casts.push(cast);
                    }
                    Err(err) => {
                        warn!("error starting screencast: {err:?}");
                        self.niri.stop_cast(session_id);
                    }
                }
            }
            ScreenCastToNiri::StopCast { session_id } => self.niri.stop_cast(session_id),
        }
    }
}

impl Niri {
    pub fn refresh_mapped_cast_outputs(&mut self) {
        let mut seen = HashSet::new();
        let mut output_changed = vec![];

        self.layout.with_windows(|mapped, output, _, _| {
            seen.insert(mapped.window.clone());

            let Some(output) = output else {
                return;
            };

            match self.casting.mapped_cast_output.entry(mapped.window.clone()) {
                Entry::Occupied(mut entry) => {
                    if entry.get() != output {
                        entry.insert(output.clone());
                        output_changed.push((mapped.id(), output.clone()));
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(output.clone());
                }
            }
        });

        self.casting
            .mapped_cast_output
            .retain(|win, _| seen.contains(win));

        let mut to_stop = vec![];
        for (id, out) in output_changed {
            let refresh = out.current_mode().unwrap().refresh as u32;
            let target = CastTarget::Window { id: id.get() };
            for cast in self
                .casting
                .casts
                .iter_mut()
                .filter(|cast| cast.target == target)
            {
                if let Err(err) = cast.set_refresh(refresh) {
                    warn!("error changing cast FPS: {err:?}");
                    to_stop.push(cast.session_id);
                };
            }
        }

        for session_id in to_stop {
            self.stop_cast(session_id);
        }
    }

    pub fn render_for_screen_cast(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        target_presentation_time: Duration,
    ) {
        let _span = tracy_client::span!("Niri::render_for_screen_cast");

        let weak = output.downgrade();
        let size = output.current_mode().unwrap().size;
        let transform = output.current_transform();
        let size = transform.transform_size(size);

        let scale = Scale::from(output.current_scale().fractional_scale());

        let mut elements = Vec::new();
        let mut cursor_data = None;

        let mut casts_to_stop = vec![];

        let mut casts = mem::take(&mut self.casting.casts);
        for cast in &mut casts {
            if !cast.is_active() {
                continue;
            }

            if !cast.target.matches_output(&weak) {
                continue;
            }

            match cast.ensure_size(size) {
                Ok(CastSizeChange::Ready) => (),
                Ok(CastSizeChange::Pending) => continue,
                Err(err) => {
                    warn!("error updating stream size, stopping screencast: {err:?}");
                    casts_to_stop.push(cast.session_id);
                }
            }

            if cast.check_time_and_schedule(output, target_presentation_time) {
                continue;
            }

            if cursor_data.is_none() {
                let mut pointer_pos = Point::default();
                if self.pointer_visibility.is_visible() {
                    let output_geo = self.global_space.output_geometry(output).unwrap().to_f64();
                    let pointer_loc = self
                        .tablet_cursor_location
                        .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());
                    // Only render when the pointer is within the output. Otherwise, it will
                    // happily appear anywhere outside the output video source in OBS.
                    if output_geo.contains(pointer_loc) {
                        pointer_pos = pointer_loc - output_geo.loc;
                        self.render_pointer(renderer, output, &mut |elem| {
                            elements.push(elem.into())
                        });
                    }
                }

                let main_start = elements.len();
                let ctx = RenderCtx {
                    renderer,
                    target: RenderTarget::Screencast,
                    xray: None,
                };
                self.render(ctx, output, false, &mut |elem| elements.push(elem.into()));

                cursor_data = Some(CursorData::compute(
                    &elements,
                    main_start,
                    pointer_pos,
                    scale,
                ));
            }
            let cursor_data = cursor_data.as_ref().unwrap();

            if cast.dequeue_buffer_and_render(renderer, &elements, cursor_data, size, scale) {
                cast.record_frame_time(target_presentation_time);
            }
        }
        self.casting.casts = casts;

        for id in casts_to_stop {
            self.stop_cast(id);
        }
    }

    pub fn render_windows_for_screen_cast(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        target_presentation_time: Duration,
    ) {
        let _span = tracy_client::span!("Niri::render_windows_for_screen_cast");

        let scale = Scale::from(output.current_scale().fractional_scale());

        let mut casts_to_stop = vec![];

        let mut casts = mem::take(&mut self.casting.casts);
        for cast in &mut casts {
            if !cast.is_active() {
                continue;
            }

            let CastTarget::Window { id } = cast.target else {
                continue;
            };

            let mut windows = self.layout.windows_for_output(output);
            let Some(mapped) = windows.find(|win| win.id().get() == id) else {
                continue;
            };

            let bbox = mapped.capture_bbox(scale);

            match cast.ensure_size(bbox.size) {
                Ok(CastSizeChange::Ready) => (),
                Ok(CastSizeChange::Pending) => continue,
                Err(err) => {
                    warn!("error updating stream size, stopping screencast: {err:?}");
                    casts_to_stop.push(cast.session_id);
                }
            }

            if cast.check_time_and_schedule(output, target_presentation_time) {
                continue;
            }

            let mut elements = Vec::new();
            let mut pointer_location = Point::default();

            if self.pointer_visibility.is_visible() {
                if let Some((pointer_pos, win_pos)) = self.pointer_pos_for_window_cast(mapped) {
                    // Pointer location must be relative to the screencast buffer.
                    // - win_pos is the position of the main window surface in output-local
                    //   coordinates
                    // - bbox.loc moves us relative to the screencast buffer
                    let buf_pos = win_pos + bbox.loc.to_f64().to_logical(scale);
                    let output_pos = self.global_space.output_geometry(output).unwrap().loc;
                    pointer_location = pointer_pos - output_pos.to_f64() - buf_pos;

                    let pos = buf_pos.to_physical_precise_round(scale).upscale(-1);
                    self.render_pointer(renderer, output, &mut |elem| {
                        let elem =
                            RelocateRenderElement::from_element(elem, pos, Relocate::Relative);
                        elements.push(CastRenderElement::from(elem));
                    });
                }
            }

            let ctx = RenderCtx {
                renderer,
                target: RenderTarget::Screencast,
                xray: None,
            };
            let main_start = elements.len();
            mapped.render_for_screen_cast(ctx, scale, &mut |elem| {
                elements.push(CastRenderElement::from(elem))
            });

            let cursor_data = CursorData::compute(&elements, main_start, pointer_location, scale);

            if cast.dequeue_buffer_and_render(renderer, &elements, &cursor_data, bbox.size, scale) {
                cast.record_frame_time(target_presentation_time);
            }
        }
        self.casting.casts = casts;

        for id in casts_to_stop {
            self.stop_cast(id);
        }
    }

    pub fn stop_cast(&mut self, session_id: CastSessionId) {
        let _span = tracy_client::span!("Niri::stop_cast");
        let _span = debug_span!("stop_cast", %session_id).entered();

        self.casting
            .pending_dynamic_casts
            .retain(|p| p.session_id != session_id);

        let had_node = self.disconnect_casts(session_id);
        let failure = (!had_node).then_some("the screencast stopped before it started");
        if self.casting.kde.end(session_id, failure) {
            return;
        }

        let dbus = &self.dbus.as_ref().unwrap();
        let server = dbus.conn_screen_cast.as_ref().unwrap().object_server();
        let path = format!("/org/gnome/Mutter/ScreenCast/Session/u{}", session_id.get());
        if let Ok(iface) = server.interface::<_, mutter_screen_cast::Session>(path) {
            let _span = tracy_client::span!("invoking Session::stop");

            async_io::block_on(async move {
                iface
                    .get()
                    .stop(server.inner(), iface.signal_emitter().clone())
                    .await
            });
        }
    }

    /// Disconnects the session's streams and says whether any of them had announced its node.
    fn disconnect_casts(&mut self, session_id: CastSessionId) -> bool {
        let mut had_node = false;
        for i in (0..self.casting.casts.len()).rev() {
            let cast = &self.casting.casts[i];
            if cast.session_id != session_id {
                continue;
            }

            let cast = self.casting.casts.swap_remove(i);
            had_node |= cast.node_id().is_some();
            if let Err(err) = cast.stream.disconnect() {
                warn!("error disconnecting stream: {err:?}");
            }
        }
        had_node
    }

    pub fn stop_casts_for_target(&mut self, target: CastTarget) {
        let _span = tracy_client::span!("Niri::stop_casts_for_target");

        // This is O(N^2) but it shouldn't be a problem I think.
        let mut saw_dynamic = false;
        let mut ids = Vec::new();
        for cast in &self.casting.casts {
            if cast.target != target {
                continue;
            }

            if cast.dynamic_target {
                saw_dynamic = true;
                continue;
            }

            ids.push(cast.session_id);
        }

        for id in ids {
            self.stop_cast(id);
        }

        // We don't stop dynamic casts, instead we switch them to Nothing.
        if saw_dynamic {
            self.event_loop
                .insert_idle(|state| state.set_dynamic_cast_target(CastTarget::Nothing));
        }
    }

    fn cast_params_for_window(&self, window_id: u64) -> Option<(Size<i32, Physical>, u32)> {
        let (_, mapped) = self
            .layout
            .windows()
            .find(|(_, m)| m.id().get() == window_id)?;
        let output = self.casting.mapped_cast_output.get(&mapped.window)?;
        let scale = Scale::from(output.current_scale().fractional_scale());
        let bbox = mapped.capture_bbox(scale);
        let refresh = output.current_mode().unwrap().refresh as u32;
        Some((bbox.size, refresh))
    }
}

impl Niri {
    pub fn region_cast_outputs(&self, rect: Rectangle<i32, Logical>) -> Vec<Output> {
        self.global_space
            .outputs()
            .filter(|output| {
                self.global_space
                    .output_geometry(output)
                    .is_some_and(|geo| geo.overlaps(rect))
            })
            .cloned()
            .collect()
    }

    /// Buffer size, scale and refresh of a region cast, or `None` while no output shows the
    /// region.
    fn cast_params_for_region(
        &self,
        rect: Rectangle<i32, Logical>,
        scale: Option<f64>,
    ) -> Option<(Size<i32, Physical>, f64, u32)> {
        let outputs = self.region_cast_outputs(rect);
        let max_scale = outputs
            .iter()
            .map(|output| output.current_scale().fractional_scale())
            .max_by(f64::total_cmp)?;
        let refresh = outputs
            .iter()
            .map(|output| output.current_mode().unwrap().refresh as u32)
            .max()?;
        let scale = scale.unwrap_or(max_scale);
        let size = rect.size.to_f64().to_physical(scale).to_i32_round::<i32>();
        let size = Size::from((size.w.max(1), size.h.max(1)));
        Some((size, scale, refresh))
    }

    /// Renders the region casts that `output` shows part of. A region spanning several outputs
    /// gets every one of them, each scaled to the cast's scale and cut to its own rectangle.
    pub fn render_regions_for_screen_cast(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        target_presentation_time: Duration,
    ) {
        let _span = tracy_client::span!("Niri::render_regions_for_screen_cast");

        let Some(output_geo) = self.global_space.output_geometry(output) else {
            return;
        };

        let mut casts_to_stop = vec![];

        let mut casts = mem::take(&mut self.casting.casts);
        for cast in &mut casts {
            if !cast.is_active() {
                continue;
            }

            let CastTarget::Region { rect, scale } = cast.target else {
                continue;
            };
            if !output_geo.overlaps(rect) {
                continue;
            }
            let Some((size, scale, _)) = self.cast_params_for_region(rect, scale) else {
                continue;
            };

            match cast.ensure_size(size) {
                Ok(CastSizeChange::Ready) => (),
                Ok(CastSizeChange::Pending) => continue,
                Err(err) => {
                    warn!("error updating stream size, stopping screencast: {err:?}");
                    casts_to_stop.push(cast.session_id);
                }
            }

            if cast.check_time_and_schedule(output, target_presentation_time) {
                continue;
            }

            let scale = Scale::from(scale);
            let mut elements = Vec::new();
            let mut pointer_pos = Point::default();

            if self.pointer_visibility.is_visible() {
                let pointer_loc = self
                    .tablet_cursor_location
                    .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());
                let pointer_output = self.global_space.output_under(pointer_loc).next().cloned();
                let placed = pointer_output
                    .filter(|_| rect.to_f64().contains(pointer_loc))
                    .and_then(|output| {
                        Some((self.region_placement(&output, rect, scale)?, output))
                    });
                if let Some(((geo, zoom), pointer_output)) = placed {
                    pointer_pos = pointer_loc - rect.loc.to_f64();
                    self.render_pointer(renderer, &pointer_output, &mut |elem| {
                        let elem = OutputRenderElements::from(elem);
                        if let Some(elem) = scale_relocate_crop(elem, scale, zoom, geo) {
                            elements.push(CastRenderElement::from(elem));
                        }
                    });
                }
            }

            let main_start = elements.len();
            for region_output in self.region_cast_outputs(rect) {
                let Some((geo, zoom)) = self.region_placement(&region_output, rect, scale) else {
                    continue;
                };
                let ctx = RenderCtx {
                    renderer: &mut *renderer,
                    target: RenderTarget::Screencast,
                    xray: None,
                };
                self.render(ctx, &region_output, false, &mut |elem| {
                    if let Some(elem) = scale_relocate_crop(elem, scale, zoom, geo) {
                        elements.push(CastRenderElement::from(elem));
                    }
                });
            }

            let cursor_data = CursorData::compute(&elements, main_start, pointer_pos, scale);

            if cast.dequeue_buffer_and_render(renderer, &elements, &cursor_data, size, scale) {
                cast.record_frame_time(target_presentation_time);
            }
        }
        self.casting.casts = casts;

        for id in casts_to_stop {
            self.stop_cast(id);
        }
    }

    /// Where `output` sits in a region cast's buffer, in the region's logical coordinates, and
    /// the zoom that takes its rendering from its own scale to the cast's.
    fn region_placement(
        &self,
        output: &Output,
        rect: Rectangle<i32, Logical>,
        scale: Scale<f64>,
    ) -> Option<(Rectangle<f64, Logical>, f64)> {
        let geo = self.global_space.output_geometry(output)?;
        let geo = Rectangle::new(geo.loc - rect.loc, geo.size).to_f64();
        let zoom = scale.x / output.current_scale().fractional_scale();
        Some((geo, zoom))
    }
}

impl State {
    fn start_kde_cast_on(
        &mut self,
        session_id: CastSessionId,
        source: KdeCastSource,
        pointer: KdePointer,
    ) -> anyhow::Result<()> {
        let (target, size, refresh, alpha) = match source {
            KdeCastSource::Output(output) => {
                ensure!(
                    self.niri.output_state.contains_key(&output),
                    "the output is gone"
                );
                let (size, refresh) = cast_params_for_output(&output);
                (CastTarget::output(&output), size, refresh, false)
            }
            KdeCastSource::Window(uuid) => {
                let id = parse_window_id(&uuid);
                let params = id.and_then(|id| Some((id, self.niri.cast_params_for_window(id)?)));
                let (id, (size, refresh)) = params.context("no window has this id")?;
                (CastTarget::Window { id }, size, refresh, true)
            }
            KdeCastSource::Region { rect, scale } => {
                let (size, _, refresh) = self
                    .niri
                    .cast_params_for_region(rect, scale)
                    .context("no output shows this region")?;
                (CastTarget::Region { rect, scale }, size, refresh, false)
            }
        };

        let gbm = self.prepare_pw_cast()?;
        let stream = self
            .niri
            .casting
            .kde
            .stream(session_id)
            .context("the client closed the stream")?;
        let announce: NodeAnnouncer = Box::new(move |node| {
            stream.created(node);
            Ok(())
        });

        let pw = self.niri.casting.pipewire.as_ref().unwrap();
        let mut cast = pw.start_cast(
            gbm,
            session_id,
            CastStreamId::next(),
            target,
            size,
            refresh,
            alpha,
            pointer.into(),
            announce,
        )?;
        cast.stamp_pts = true;
        self.niri.casting.casts.push(cast);
        Ok(())
    }
}

impl KdeScreencastHandler for State {
    fn kde_screencast_state(&mut self) -> &mut KdeScreencastManagerState {
        &mut self.niri.casting.kde
    }

    fn start_kde_cast(
        &mut self,
        session_id: CastSessionId,
        source: KdeCastSource,
        pointer: KdePointer,
    ) {
        let _span = debug_span!("start_kde_cast", %session_id).entered();

        if let Err(err) = self.start_kde_cast_on(session_id, source, pointer) {
            warn!("error starting a KDE screencast: {err:?}");
            let error = format!("{err:#}");
            self.niri.casting.kde.end(session_id, Some(&error));
        }
    }

    fn stop_kde_cast(&mut self, session_id: CastSessionId) {
        self.niri.disconnect_casts(session_id);
    }
}

impl From<KdePointer> for CursorMode {
    fn from(pointer: KdePointer) -> Self {
        match pointer {
            KdePointer::Hidden => CursorMode::Hidden,
            KdePointer::Embedded => CursorMode::Embedded,
            KdePointer::Metadata => CursorMode::Metadata,
        }
    }
}

fn announce_to_mutter(signal_ctx: SignalEmitter<'static>) -> NodeAnnouncer {
    Box::new(move |node| {
        let _span = tracy_client::span!("sending PipeWireStreamAdded");
        async_io::block_on(mutter_screen_cast::Stream::pipe_wire_stream_added(
            &signal_ctx,
            node,
        ))
        .context("error sending PipeWireStreamAdded")
    })
}

fn cast_params_for_output(output: &Output) -> (Size<i32, Physical>, u32) {
    let mode = output.current_mode().unwrap();
    let transform = output.current_transform();
    let size = transform.transform_size(mode.size);
    let refresh = mode.refresh as u32;
    (size, refresh)
}

niri_render_elements! {
    CastRenderElement<R> => {
        Output = OutputRenderElements<R>,
        Window = WindowCastRenderElements<R>,
        Pointer = PointerRenderElements<R>,
        RelocatedPointer = RelocateRenderElement<PointerRenderElements<R>>,
        Region = CropRenderElement<RelocateRenderElement<RescaleRenderElement<
            OutputRenderElements<R>
        >>>,
    }
}
