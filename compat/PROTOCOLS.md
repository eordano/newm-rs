# Wayland protocols: newm-rs and KWin

Measured by the compat suite (`inventory` and `kwin-reference` cases) in the same VM image
(nixpkgs d5cc6833, 2026-11-19): `wayland-info` as an ordinary client of each
compositor. KWin is `kwin_wayland 6.8.3 --virtual` with `KWIN_WAYLAND_NO_PERMISSION_CHECKS=1`,
so it lists its restricted interfaces too; its virtual backend has no DRM, so its
`wp_drm_lease_device_v1` (DRM only) is missing from the list. newm-rs shows one
`wp_drm_lease_device_v1` per DRM device (two on a desktop with two GPUs, one in the VM);
otherwise the VM's newm-rs list is exactly what a live desktop session advertises. Regenerate the table from a report's
`globals-*.txt` files.

## What the gaps break

Ranked by what a desktop's users (Chromium, KDE apps, Alacritty, Ghostty and other terminals,
remote desktops, hybrid Intel + NVIDIA graphics) would notice.

Closed since this table was written (2026-11-19 wave 2; the counts below are the first
inventory): 1 (newm-rs renders on a software EGL device when the primary GPU has nothing else),
6 (`org_kde_kwin_fake_input` v6, for xdg-desktop-portal-kde only), 7 (explicit sync),
8 (wlr-output-power), 10 (xdg-system-bell) and 11 (xdg-wm-dialog). Still open: 2, 4, 5, 9.

| # | Missing | Who uses it | What breaks without it |
|---|---|---|---|
| 1 | a software renderer | any machine whose GPU has no hardware EGL: a VM without virgl, simpledrm/efifb when the GPU driver fails, a headless box | niri (and newm-rs) skips software EGL devices (`src/backend/tty.rs`, "software EGL renderers are skipped"): the session starts but shows no output, nothing can draw, grim hangs. The suite tests newm-rs with that check dropped (`newm-rs.nix { allowSoftwareEgl = true; }`) |
| 2 | `xdg_toplevel_drag_manager_v1` | Chromium (tab strip drags), Firefox 125+ (`widget.wayland` tab detach), KDE apps with detachable tabs | dragging a tab out cannot carry a window with the pointer: the new window opens where the compositor places it and dropping it back onto another tab strip does not attach |
| 4 | `kde_output_device_registry_v2`, `kde_output_management_v2`, `kde_output_order_v1`, `kde_external_brightness_v1` | System Settings' Display & Monitor KCM (kscreen), `kscreen-doctor`, Plasma's display OSD, PowerDevil's external-monitor brightness | the Display page shows nothing or errors; outputs can only be changed with wlr-output-management tools (`wlr-randr`, kanshi) or the KDL; PowerDevil cannot dim external monitors |
| 5 | `org_kde_plasma_shell`, `org_kde_plasma_window_management`, `org_kde_plasma_virtual_desktop_management`, `org_kde_plasma_activation_feedback`, `kde_lockscreen_overlay_v1`, `kde_screen_edge_manager_v1`, `org_kde_kwin_shadow_manager` | KRunner and Plasma popups (positioning, panel role), Plasma's task manager, launch feedback, kscreenlocker overlays, Klipper's popup | KRunner opens as an ordinary window (a config can float it by rule); Plasma shell components that need these do not run; launch feedback falls back to newm's placeholders |
| 6 | `org_kde_kwin_fake_input` | KDE Connect remote input (phone as mouse/keyboard), Krfb, KDE's `kdotool`-style scripts | KDE Connect's remote input does nothing unless it goes through the RemoteDesktop portal; virtual keyboard/pointer tools (`wtype`, `wlrctl`) work instead |
| 7 | `wp_linux_drm_syncobj_manager_v1` (explicit sync) | NVIDIA's proprietary driver (GL/Vulkan clients and Xwayland on NVIDIA), Mesa Vulkan WSI | NVIDIA-rendered clients rely on implicit sync and can flicker or show stale frames; a hybrid desktop that renders newm-rs on the Intel iGPU exposes only the apps offloaded to the NVIDIA GPU |
| 8 | `zwlr_output_power_manager_v1` | `wlopm`, swayidle scripts that call it, wayvnc's output power | `wlopm --off '*'` fails on newm-rs; a config turns monitors off with newm-rs's own `power-off-monitors` action |
| 9 | `xdg_toplevel_icon_manager_v1` | foot (logs "compositor does not implement the xdg-toplevel-icon protocol" at every start), Qt 6.9+, SDL3, Ghostty | per-window icons are lost; taskbars fall back to the app ID's desktop entry |
| 10 | `xdg_system_bell_v1` | GTK 4.14+ (`gtk_widget_error_bell`), Ghostty (`bell-features = system`), libadwaita apps | the bell is silent |
| 11 | `xdg_wm_dialog_v1` | GTK 4.12+ and libadwaita (`AdwDialog`/`GtkWindow:modal`), Qt 6.8+ modal dialogs | modality stays client-side; newm's own dialog rule (float centred on the parent) still places them |
| 12 | `wp_color_manager_v1`, `wp_color_representation_manager_v1` | mpv, Chromium and Firefox HDR video, GTK 4.18 colour states, Mesa Vulkan HDR WSI, KDE apps | SDR only: HDR content is tone-mapped by the client, wide-gamut content is clamped to sRGB |
| 13 | `wp_fifo_manager_v1`, `wp_commit_timing_manager_v1` | Mesa 25+ Vulkan WSI (FIFO present mode: games, Proton, Zed) | Mesa falls back to frame-callback pacing, which can stall a Vulkan client while its window is hidden |
| 14 | `wp_pointer_warp_v1` | SDL3 games, Blender's continuous grab, Qt 6.10 `QCursor::setPos` | pointer warps fall back to pointer constraints' position hint, or do nothing |
| 15 | `wp_tearing_control_manager_v1`, `wp_content_type_manager_v1`, `wp_alpha_modifier_v1`, `wp_single_pixel_buffer_manager_v1` | games asking for tearing, video players' content type, mpv/gamescope overlays, GTK 4.14+ solid backgrounds and video black bars | no tearing (latency), no content-type hints, overlay and solid-colour planes come as ordinary buffers (a little more GPU and memory work); niri builds single-pixel-buffer only under `cfg(test)` |
| 17 | `zwp_text_input_manager_v2` (KDE's) | older Qt (5.x) input methods | Qt 6 and GTK use `zwp_text_input_manager_v3`, which both serve; fcitx5 talks `zwp_input_method_manager_v2` |
| 19 | `org_kde_kwin_dpms_manager`, `org_kde_kwin_keystate`, `org_kde_kwin_idle`, `org_kde_kwin_appmenu_manager`, `org_kde_kwin_server_decoration_palette_manager`, `xdg_session_manager_v1` | PowerDevil's screen off, Plasma's lock-key indicator, legacy KDE idle, the Plasma global menu, Breeze decoration colours, KWin 6.8's session restore | swayidle with newm's actions replaces PowerDevil's screen off; newm has its own session restore |

What KWin lacks and newm-rs serves: `ext_session_lock_manager_v1` (KWin locks with
kscreenlocker), wlr screencopy and `ext_image_copy_capture_*` (grim, wayvnc,
xdg-desktop-portal-wlr), wlr and ext foreign-toplevel (Quickshell taskbars), wlr output
management and gamma control (wlr-randr, kanshi, wlsunset), virtual keyboard and pointer (wtype,
wayvnc), `zwp_input_method_manager_v2` (fcitx5) and wlr data control
(cliphist, wl-clip-persist).

## Behaviour the suite found

- newm-rs sends a new window's fractional scale (1.25) before its first commit: Chromium's first
  title already reads 1.25.
- GTK 3 has no fractional scaling: at 1.25 it draws at buffer scale 2 and the compositor
  downscales (blurrier text), on newm-rs as on KWin.

newm-rs advertises the wlr protocols to every unsandboxed client: only clients inside a
`wp_security_context_v1` (Flatpak) are filtered.

## Globals

| Global | newm-rs | KWin 6.8.3 |
|---|---|---|
| `ext_background_effect_manager_v1` | 1 | 1 |
| `ext_data_control_manager_v1` | 1 | 1 |
| `ext_foreign_toplevel_image_capture_source_manager_v1` | 1 | - |
| `ext_foreign_toplevel_list_v1` | 1 | - |
| `ext_idle_notifier_v1` | 2 | 2 |
| `ext_image_copy_capture_manager_v1` | 1 | - |
| `ext_output_image_capture_source_manager_v1` | 1 | - |
| `ext_session_lock_manager_v1` | 1 | - |
| `ext_workspace_manager_v1` | 1 | - |
| `kde_external_brightness_v1` | - | 3 |
| `kde_lockscreen_overlay_v1` | - | 1 |
| `kde_output_device_registry_v2` | - | 26 |
| `kde_output_management_v2` | - | 22 |
| `kde_output_order_v1` | - | 1 |
| `kde_screen_edge_manager_v1` | - | 1 |
| `mutter_x11_interop` | 1 | - |
| `org_kde_kwin_appmenu_manager` | - | 2 |
| `org_kde_kwin_dpms_manager` | - | 1 |
| `org_kde_kwin_fake_input` | - | 6 |
| `org_kde_kwin_idle` | - | 1 |
| `org_kde_kwin_keystate` | - | 5 |
| `org_kde_kwin_server_decoration_manager` | 1 | 1 |
| `org_kde_kwin_server_decoration_palette_manager` | - | 1 |
| `org_kde_kwin_shadow_manager` | - | 2 |
| `org_kde_plasma_activation_feedback` | - | 1 |
| `org_kde_plasma_shell` | - | 8 |
| `org_kde_plasma_virtual_desktop_management` | - | 4 |
| `org_kde_plasma_window_management` | - | 21 |
| `wl_compositor` | 6 | 6 |
| `wl_data_device_manager` | 3 | 3 |
| `wl_fixes` | - | 1 |
| `wl_output` | 4 | 4 |
| `wl_seat` | 9 | 11 |
| `wl_shm` | 2 | 2 |
| `wl_subcompositor` | 1 | 1 |
| `wp_alpha_modifier_v1` | - | 1 |
| `wp_color_manager_v1` | - | 3 |
| `wp_color_representation_manager_v1` | - | 1 |
| `wp_content_type_manager_v1` | - | 1 |
| `wp_cursor_shape_manager_v1` | 2 | 2 |
| `wp_drm_lease_device_v1` | 1 | - |
| `wp_fifo_manager_v1` | - | 1 |
| `wp_fractional_scale_manager_v1` | 1 | 1 |
| `wp_linux_drm_syncobj_manager_v1` | - | 1 |
| `wp_pointer_warp_v1` | - | 1 |
| `wp_presentation` | 2 | 2 |
| `wp_security_context_manager_v1` | 1 | 1 |
| `wp_single_pixel_buffer_manager_v1` | - | 1 |
| `wp_tearing_control_manager_v1` | - | 1 |
| `wp_viewporter` | 1 | 1 |
| `xdg_activation_v1` | 1 | 1 |
| `xdg_session_manager_v1` | - | 1 |
| `xdg_system_bell_v1` | - | 1 |
| `xdg_toplevel_drag_manager_v1` | - | 1 |
| `xdg_toplevel_icon_manager_v1` | - | 1 |
| `xdg_toplevel_tag_manager_v1` | 1 | 1 |
| `xdg_wm_base` | 7 | 6 |
| `xdg_wm_dialog_v1` | - | 1 |
| `zkde_screencast_unstable_v1` | 5 | 6 |
| `zwlr_data_control_manager_v1` | 2 | - |
| `zwlr_foreign_toplevel_manager_v1` | 3 | - |
| `zwlr_gamma_control_manager_v1` | 1 | - |
| `zwlr_layer_shell_v1` | 5 | 5 |
| `zwlr_output_manager_v1` | 4 | - |
| `zwlr_screencopy_manager_v1` | 3 | - |
| `zwlr_virtual_pointer_manager_v1` | 2 | - |
| `zwp_idle_inhibit_manager_v1` | 1 | 1 |
| `zwp_input_method_manager_v2` | 1 | - |
| `zwp_keyboard_shortcuts_inhibit_manager_v1` | 1 | 1 |
| `zwp_linux_dmabuf_v1` | 6 | 6 |
| `zwp_pointer_constraints_v1` | 1 | 1 |
| `zwp_pointer_gestures_v1` | 3 | 3 |
| `zwp_primary_selection_device_manager_v1` | 1 | 1 |
| `zwp_relative_pointer_manager_v1` | 1 | 1 |
| `zwp_tablet_manager_v2` | 1 | 2 |
| `zwp_text_input_manager_v2` | - | 1 |
| `zwp_text_input_manager_v3` | 1 | 2 |
| `zwp_virtual_keyboard_manager_v1` | 1 | - |
| `zxdg_decoration_manager_v1` | 1 | 2 |
| `zxdg_exporter_v2` | 1 | 1 |
| `zxdg_importer_v2` | 1 | 1 |
| `zxdg_output_manager_v1` | 3 | 3 |
