//! The Wayland side: ext-session-lock-v1, one lock surface per output, keyboard input.

use std::io;
use std::sync::Arc;

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::channel::{self, Sender};
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
    SessionLockSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_output, delegate_registry, delegate_seat, delegate_session_lock,
    delegate_shm, registry_handlers,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
    self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use xkbcommon::xkb;

use crate::args::Options;
use crate::auth::{spawn_check, Authenticator};
use crate::field::{Field, Outcome};
use crate::keyboard::{locale_compose_table, Keyboard};
use crate::render::{draw_field, Canvas, Colors, Layout};
use crate::wallpaper::{cover, Image};

struct View {
    output: wl_output::WlOutput,
    lock_surface: SessionLockSurface,
    viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    size: Option<(u32, u32)>,
    scale120: Option<u32>,
    int_scale: i32,
    background: Option<(u32, u32, Vec<u8>)>,
    buffer: Option<Buffer>,
}

impl View {
    fn surface(&self) -> &wl_surface::WlSurface {
        self.lock_surface.wl_surface()
    }

    fn scale(&self) -> f64 {
        match (self.viewport.is_some(), self.scale120) {
            (true, Some(s)) => s as f64 / 120.0,
            _ => self.int_scale as f64,
        }
    }

    fn buffer_size(&self) -> Option<(u32, u32)> {
        let (w, h) = self.size?;
        let s = self.scale();
        Some(((w as f64 * s).round() as u32, (h as f64 * s).round() as u32))
    }
}

impl Drop for View {
    fn drop(&mut self) {
        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }
        if let Some(fractional) = self.fractional.take() {
            fractional.destroy();
        }
    }
}

struct App {
    conn: Connection,
    registry: RegistryState,
    compositor: CompositorState,
    outputs: OutputState,
    seats: SeatState,
    shm: Shm,
    viewporter: Option<WpViewporter>,
    fractional: Option<WpFractionalScaleManagerV1>,
    pool: SlotPool,
    lock: Option<SessionLock>,
    views: Vec<View>,
    keyboards: Vec<(wl_seat::WlSeat, wl_keyboard::WlKeyboard)>,
    xkb_context: xkb::Context,
    keyboard: Keyboard,
    field: Field,
    colors: Colors,
    wallpaper: Option<Image>,
    auth: Arc<dyn Authenticator>,
    results: Sender<bool>,
    daemonize: bool,
    hinted: bool,
    exit: Option<u8>,
}

pub fn run(
    opts: Options,
    wallpaper: Option<Image>,
    auth: Arc<dyn Authenticator>,
) -> Result<u8, String> {
    let conn =
        Connection::connect_to_env().map_err(|e| format!("cannot connect to Wayland: {e}"))?;
    let (globals, queue) = registry_queue_init::<App>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let mut event_loop = EventLoop::<App>::try_new().map_err(|e| e.to_string())?;

    let (results, results_rx) = channel::channel();
    event_loop
        .handle()
        .insert_source(results_rx, |event, _, app| {
            if let channel::Event::Msg(ok) = event {
                app.auth_done(ok);
            }
        })
        .map_err(|e| e.error.to_string())?;

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let pool = SlotPool::new(1 << 16, &shm).map_err(|e| e.to_string())?;
    let lock_state = SessionLockState::new(&globals, &qh);
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional = viewporter.as_ref().and_then(|_| {
        globals
            .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
            .ok()
    });

    let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keyboard = Keyboard::new(locale_compose_table(&xkb_context).as_ref());
    let mut app = App {
        conn: conn.clone(),
        registry: RegistryState::new(&globals),
        compositor,
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        shm,
        viewporter,
        fractional,
        pool,
        lock: None,
        views: Vec::new(),
        keyboards: Vec::new(),
        xkb_context,
        keyboard,
        field: Field::new(),
        colors: opts.colors,
        wallpaper,
        auth,
        results,
        daemonize: opts.daemonize,
        hinted: false,
        exit: None,
    };

    let lock = lock_state
        .lock(&qh)
        .map_err(|_| "the compositor does not offer ext-session-lock-v1".to_owned())?;
    app.lock = Some(lock);
    for output in app.outputs.outputs().collect::<Vec<_>>() {
        app.add_view(&qh, output);
    }

    WaylandSource::new(conn, queue)
        .insert(event_loop.handle())
        .map_err(|e| e.error.to_string())?;

    loop {
        if let Some(code) = app.exit {
            return Ok(code);
        }
        event_loop
            .dispatch(None, &mut app)
            .map_err(|e| e.to_string())?;
    }
}

impl App {
    fn add_view(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        let Some(lock) = &self.lock else { return };
        if self.views.iter().any(|v| v.output == output) {
            return;
        }
        let int_scale = self
            .outputs
            .info(&output)
            .map_or(1, |i| i.scale_factor)
            .max(1);
        let surface = self.compositor.create_surface(qh);
        let viewport = self
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(&surface, qh, ()));
        let fractional = self
            .fractional
            .as_ref()
            .map(|m| m.get_fractional_scale(&surface, qh, surface.clone()));
        let lock_surface = lock.create_lock_surface(surface, &output, qh);
        self.views.push(View {
            output,
            lock_surface,
            viewport,
            fractional,
            size: None,
            scale120: None,
            int_scale,
            background: None,
            buffer: None,
        });
    }

    fn set_int_scale(&mut self, i: usize, scale: i32) {
        let before = self.views[i].scale();
        self.views[i].int_scale = scale.max(1);
        if self.views[i].scale() != before {
            self.draw(i);
        }
    }

    fn view_index(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.views.iter().position(|v| v.surface() == surface)
    }

    fn redraw_all(&mut self) {
        for i in 0..self.views.len() {
            self.draw(i);
        }
    }

    fn draw(&mut self, i: usize) {
        let App {
            views,
            pool,
            wallpaper,
            colors,
            field,
            ..
        } = self;
        let view = &mut views[i];
        let (Some((lw, lh)), Some((w, h))) = (view.size, view.buffer_size()) else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        if view
            .background
            .as_ref()
            .is_none_or(|b| (b.0, b.1) != (w, h))
        {
            let pixels = match wallpaper {
                Some(image) => cover(image, w, h),
                None => colors.bg.xrgb().repeat(w as usize * h as usize),
            };
            view.background = Some((w, h, pixels));
        }
        let stride = w as i32 * 4;
        let reusable = view.buffer.as_ref().is_some_and(|b| {
            b.height() == h as i32 && b.stride() == stride && b.canvas(pool).is_some()
        });
        if !reusable {
            match pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Xrgb8888) {
                Ok((buffer, _)) => view.buffer = Some(buffer),
                Err(e) => {
                    eprintln!("newm-rs-lock: cannot allocate a {w}x{h} buffer: {e}");
                    return;
                }
            }
        }
        let Some(buffer) = view.buffer.as_ref() else {
            return;
        };
        let Some(data) = buffer.canvas(pool) else {
            return;
        };
        let Some((_, _, background)) = view.background.as_ref() else {
            return;
        };
        data.copy_from_slice(background);
        let layout = Layout::new(w, h, view.scale());
        let mut canvas = Canvas {
            data,
            width: w,
            height: h,
        };
        draw_field(&mut canvas, &layout, colors, field.phase(), field.dots());

        let surface = view.lock_surface.wl_surface();
        match &view.viewport {
            Some(viewport) => viewport.set_destination(lw as i32, lh as i32),
            None => surface.set_buffer_scale(view.int_scale),
        }
        surface.damage_buffer(0, 0, w as i32, h as i32);
        if buffer.attach_to(surface).is_ok() {
            surface.commit();
        }
    }

    fn on_key(&mut self, keycode: u32) {
        if self.daemonize {
            return;
        }
        let code = xkb::Keycode::new(keycode + 8);
        match self.keyboard.press(&mut self.field, code) {
            Outcome::Ignored => {}
            Outcome::Changed => self.redraw_all(),
            Outcome::Submit(password) => {
                let results = self.results.clone();
                let spawned = spawn_check(self.auth.clone(), password, move |ok| {
                    let _ = results.send(ok);
                });
                self.redraw_all();
                if let Err(e) = spawned {
                    eprintln!("newm-rs-lock: cannot start the password check: {e}");
                    self.auth_done(false);
                }
            }
            Outcome::Unlock => self.unlock(),
        }
    }

    fn auth_done(&mut self, ok: bool) {
        self.keyboard.reset_compose();
        match self.field.auth_result(ok) {
            Outcome::Unlock => self.unlock(),
            Outcome::Changed => self.redraw_all(),
            _ => {}
        }
    }

    fn unlock(&mut self) {
        if let Some(lock) = self.lock.take() {
            lock.unlock();
        }
        self.views.clear();
        let _ = self.conn.roundtrip();
        self.clear_hint();
        self.exit = Some(0);
    }

    fn clear_hint(&mut self) {
        if std::mem::take(&mut self.hinted) {
            crate::hint::set(false);
        }
    }
}

fn daemonize() {
    match unsafe { libc::fork() } {
        -1 => eprintln!("newm-rs-lock: fork failed: {}", io::Error::last_os_error()),
        0 => unsafe {
            libc::setsid();
            libc::chdir(c"/".as_ptr());
            let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
            if null >= 0 {
                libc::dup2(null, libc::STDIN_FILENO);
                libc::dup2(null, libc::STDOUT_FILENO);
                if null > libc::STDERR_FILENO {
                    libc::close(null);
                }
            }
        },
        _ => unsafe { libc::_exit(0) },
    }
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        if std::mem::take(&mut self.daemonize) {
            daemonize();
        }
        crate::hint::set(true);
        self.hinted = true;
    }

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        eprintln!("newm-rs-lock: the compositor refused or ended the lock");
        if let Some(lock) = self.lock.take() {
            lock.unlock();
        }
        self.views.clear();
        let _ = self.conn.roundtrip();
        self.clear_hint();
        self.exit = Some(1);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        if let Some(i) = self.view_index(surface.wl_surface()) {
            self.views[i].size = Some(configure.new_size);
            self.draw(i);
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(i) = self.view_index(surface) {
            self.set_int_scale(i, factor);
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.add_view(qh, output.clone());
        self.update_output(conn, qh, output);
    }

    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let Some(scale) = self.outputs.info(&output).map(|i| i.scale_factor) else {
            return;
        };
        if let Some(i) = self.views.iter().position(|v| v.output == output) {
            self.set_int_scale(i, scale);
        }
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.views.retain(|v| v.output != output);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if matches!(capability, Capability::Keyboard)
            && !self.keyboards.iter().any(|(s, _)| *s == seat)
        {
            let keyboard = seat.get_keyboard(qh, ());
            self.keyboards.push((seat, keyboard));
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if matches!(capability, Capability::Keyboard) {
            self.release_keyboard(&seat);
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.release_keyboard(&seat);
    }
}

impl App {
    fn release_keyboard(&mut self, seat: &wl_seat::WlSeat) {
        self.keyboards.retain(|(s, keyboard)| {
            if s != seat {
                return true;
            }
            if keyboard.version() >= 3 {
                keyboard.release();
            }
            false
        });
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                if !matches!(format, WEnum::Value(wl_keyboard::KeymapFormat::XkbV1)) {
                    return;
                }
                let keymap = unsafe {
                    xkb::Keymap::new_from_fd(
                        &app.xkb_context,
                        fd,
                        size as usize,
                        xkb::KEYMAP_FORMAT_TEXT_V1,
                        xkb::KEYMAP_COMPILE_NO_FLAGS,
                    )
                };
                match keymap {
                    Ok(Some(keymap)) => app.keyboard.set_keymap(&keymap),
                    _ => eprintln!("newm-rs-lock: cannot compile the keymap"),
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                app.on_key(key);
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                app.keyboard
                    .update_mask(mods_depressed, mods_latched, mods_locked, group);
            }
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, wl_surface::WlSurface> for App {
    fn event(
        app: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(i) = app.view_index(surface) {
                let before = app.views[i].scale();
                app.views[i].scale120 = Some(scale);
                if app.views[i].scale() != before {
                    app.draw(i);
                }
            }
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_seat!(App);
delegate_session_lock!(App);
delegate_shm!(App);
delegate_registry!(App);
delegate_noop!(App: WpViewporter);
delegate_noop!(App: WpViewport);
delegate_noop!(App: WpFractionalScaleManagerV1);
