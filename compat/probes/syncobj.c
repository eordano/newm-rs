/* Explicit sync probe (linux-drm-syncobj-v1): a dmabuf from a DRM dumb buffer, committed with an
 * acquire point that is not signalled yet. The compositor must hold the commit (no frame callback)
 * until the probe signals the point, then apply it; after a second commit replaces the buffer it
 * must signal the first buffer's release point. Prints one line per step and "done" on success.
 * Usage: syncobj-probe /dev/dri/cardN */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#include <drm_fourcc.h>
#include <wayland-client.h>
#include <xf86drm.h>

#include "linux-dmabuf-v1-client-protocol.h"
#include "linux-drm-syncobj-v1-client-protocol.h"
#include "xdg-shell-client-protocol.h"

static struct wl_compositor *compositor;
static struct xdg_wm_base *wm_base;
static struct zwp_linux_dmabuf_v1 *dmabuf;
static struct wp_linux_drm_syncobj_manager_v1 *syncobj_manager;
static int configured, frame_done, buffer_failed;

#define DIE(...) do { printf(__VA_ARGS__); printf("\n"); exit(1); } while (0)

static void global(void *data, struct wl_registry *reg, uint32_t name, const char *iface,
                   uint32_t version) {
    if (!strcmp(iface, wl_compositor_interface.name))
        compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
    else if (!strcmp(iface, xdg_wm_base_interface.name))
        wm_base = wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
    else if (!strcmp(iface, zwp_linux_dmabuf_v1_interface.name))
        dmabuf = wl_registry_bind(reg, name, &zwp_linux_dmabuf_v1_interface, 3);
    else if (!strcmp(iface, wp_linux_drm_syncobj_manager_v1_interface.name))
        syncobj_manager = wl_registry_bind(reg, name, &wp_linux_drm_syncobj_manager_v1_interface, 1);
}
static void global_remove(void *d, struct wl_registry *r, uint32_t n) {}
static const struct wl_registry_listener registry_listener = {global, global_remove};

static void ping(void *d, struct xdg_wm_base *b, uint32_t serial) { xdg_wm_base_pong(b, serial); }
static const struct xdg_wm_base_listener wm_base_listener = {ping};

static void surface_configure(void *d, struct xdg_surface *s, uint32_t serial) {
    xdg_surface_ack_configure(s, serial);
    configured = 1;
}
static const struct xdg_surface_listener xdg_surface_listener = {surface_configure};
static void top_configure(void *d, struct xdg_toplevel *t, int32_t w, int32_t h, struct wl_array *s) {}
static void top_close(void *d, struct xdg_toplevel *t) {}
static const struct xdg_toplevel_listener toplevel_listener = {top_configure, top_close, NULL, NULL};

static void frame(void *d, struct wl_callback *cb, uint32_t t) { frame_done = 1; wl_callback_destroy(cb); }
static const struct wl_callback_listener frame_listener = {frame};

static void params_created(void *d, struct zwp_linux_buffer_params_v1 *p, struct wl_buffer *b) {}
static void params_failed(void *d, struct zwp_linux_buffer_params_v1 *p) { buffer_failed = 1; }
static const struct zwp_linux_buffer_params_v1_listener params_listener = {params_created, params_failed};

/* Dispatches events for up to ms milliseconds, or until *flag. */
static void pump(struct wl_display *display, int ms, int *flag) {
    struct timespec start, now;
    clock_gettime(CLOCK_MONOTONIC, &start);
    for (;;) {
        if (flag && *flag) return;
        clock_gettime(CLOCK_MONOTONIC, &now);
        long left = ms - ((now.tv_sec - start.tv_sec) * 1000 + (now.tv_nsec - start.tv_nsec) / 1000000);
        if (left <= 0) return;
        wl_display_flush(display);
        struct pollfd pfd = {wl_display_get_fd(display), POLLIN, 0};
        if (wl_display_prepare_read(display) != 0) { wl_display_dispatch_pending(display); continue; }
        if (poll(&pfd, 1, left) > 0) wl_display_read_events(display); else wl_display_cancel_read(display);
        if (wl_display_dispatch_pending(display) < 0) DIE("protocol error: %s", strerror(errno));
    }
}

struct dumb { uint32_t handle, stride; int fd; };

static struct dumb make_buffer(int drm, uint32_t w, uint32_t h, uint32_t colour) {
    struct drm_mode_create_dumb create = {.width = w, .height = h, .bpp = 32};
    if (drmIoctl(drm, DRM_IOCTL_MODE_CREATE_DUMB, &create)) DIE("dumb buffer: %s", strerror(errno));
    struct drm_mode_map_dumb map = {.handle = create.handle};
    if (drmIoctl(drm, DRM_IOCTL_MODE_MAP_DUMB, &map)) DIE("map dumb: %s", strerror(errno));
    uint32_t *px = mmap(NULL, create.size, PROT_READ | PROT_WRITE, MAP_SHARED, drm, map.offset);
    if (px == MAP_FAILED) DIE("mmap dumb: %s", strerror(errno));
    for (uint64_t i = 0; i < create.size / 4; i++) px[i] = colour;
    munmap(px, create.size);
    struct dumb d = {create.handle, create.pitch, -1};
    if (drmPrimeHandleToFD(drm, create.handle, DRM_CLOEXEC | DRM_RDWR, &d.fd)) DIE("prime export: %s", strerror(errno));
    return d;
}

static struct wl_buffer *wrap(struct wl_display *display, struct dumb d, uint32_t w, uint32_t h) {
    struct zwp_linux_buffer_params_v1 *params = zwp_linux_dmabuf_v1_create_params(dmabuf);
    zwp_linux_buffer_params_v1_add_listener(params, &params_listener, NULL);
    zwp_linux_buffer_params_v1_add(params, d.fd, 0, 0, d.stride, DRM_FORMAT_MOD_LINEAR >> 32,
                                   DRM_FORMAT_MOD_LINEAR & 0xffffffff);
    struct wl_buffer *b = zwp_linux_buffer_params_v1_create_immed(params, w, h, DRM_FORMAT_XRGB8888, 0);
    wl_display_roundtrip(display);
    if (buffer_failed) DIE("dmabuf import failed (the compositor's renderer refused LINEAR XRGB8888)");
    return b;
}

static int signalled(int drm, uint32_t handle, uint64_t point, int timeout_ms) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    int64_t deadline = now.tv_sec * 1000000000LL + now.tv_nsec + (int64_t)timeout_ms * 1000000;
    return drmSyncobjTimelineWait(drm, &handle, &point, 1, deadline, 0, NULL) == 0;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    const char *node = argc > 1 ? argv[1] : "/dev/dri/card0";
    int drm = open(node, O_RDWR | O_CLOEXEC);
    if (drm < 0) DIE("open %s: %s", node, strerror(errno));
    uint64_t cap = 0;
    if (drmGetCap(drm, DRM_CAP_SYNCOBJ_TIMELINE, &cap) || !cap) DIE("%s has no timeline syncobj", node);

    struct wl_display *display = wl_display_connect(NULL);
    if (!display) DIE("no Wayland display");
    struct wl_registry *reg = wl_display_get_registry(display);
    wl_registry_add_listener(reg, &registry_listener, NULL);
    wl_display_roundtrip(display);
    if (!compositor || !wm_base || !dmabuf) DIE("missing wl_compositor/xdg_wm_base/zwp_linux_dmabuf_v1");
    if (!syncobj_manager) DIE("no wp_linux_drm_syncobj_manager_v1");
    printf("globals: syncobj manager offered\n");
    xdg_wm_base_add_listener(wm_base, &wm_base_listener, NULL);

    struct wl_surface *surface = wl_compositor_create_surface(compositor);
    struct xdg_surface *xs = xdg_wm_base_get_xdg_surface(wm_base, surface);
    xdg_surface_add_listener(xs, &xdg_surface_listener, NULL);
    struct xdg_toplevel *top = xdg_surface_get_toplevel(xs);
    xdg_toplevel_add_listener(top, &toplevel_listener, NULL);
    xdg_toplevel_set_title(top, "syncobj-probe");
    xdg_toplevel_set_app_id(top, "syncobj-probe");
    wl_surface_commit(surface);
    pump(display, 5000, &configured);
    if (!configured) DIE("no configure");

    /* Separate acquire and release timelines: a timeline point counts as signalled once a later
     * one is, so signalling an acquire point must not touch the release points checked. */
    uint32_t acq, rel;
    int acq_fd, rel_fd;
    if (drmSyncobjCreate(drm, 0, &acq) || drmSyncobjCreate(drm, 0, &rel)) DIE("syncobj create: %s", strerror(errno));
    if (drmSyncobjHandleToFD(drm, acq, &acq_fd) || drmSyncobjHandleToFD(drm, rel, &rel_fd))
        DIE("syncobj export: %s", strerror(errno));
    struct wp_linux_drm_syncobj_timeline_v1 *acq_tl =
        wp_linux_drm_syncobj_manager_v1_import_timeline(syncobj_manager, acq_fd);
    struct wp_linux_drm_syncobj_timeline_v1 *rel_tl =
        wp_linux_drm_syncobj_manager_v1_import_timeline(syncobj_manager, rel_fd);
    struct wp_linux_drm_syncobj_surface_v1 *sync =
        wp_linux_drm_syncobj_manager_v1_get_surface(syncobj_manager, surface);

    struct dumb d1 = make_buffer(drm, 256, 256, 0xff3060c0), d2 = make_buffer(drm, 256, 256, 0xffc06030);
    struct wl_buffer *b1 = wrap(display, d1, 256, 256), *b2 = wrap(display, d2, 256, 256);

    /* Commit 1: acquire point 1 not signalled yet, release point 1. */
    wl_surface_attach(surface, b1, 0, 0);
    wl_surface_damage_buffer(surface, 0, 0, 256, 256);
    wp_linux_drm_syncobj_surface_v1_set_acquire_point(sync, acq_tl, 0, 1);
    wp_linux_drm_syncobj_surface_v1_set_release_point(sync, rel_tl, 0, 1);
    wl_callback_add_listener(wl_surface_frame(surface), &frame_listener, NULL);
    wl_surface_commit(surface);
    pump(display, 1500, &frame_done);
    if (frame_done) DIE("held: no (the commit was applied before its acquire point was signalled)");
    printf("held: yes (no frame for 1.5 s while acquire point 1 is unsignalled)\n");

    if (drmSyncobjTimelineSignal(drm, &acq, (uint64_t[]){1}, 1)) DIE("signal: %s", strerror(errno));
    pump(display, 5000, &frame_done);
    if (!frame_done) DIE("applied: no (no frame within 5 s after the acquire point was signalled)");
    printf("applied: yes (frame after signalling point 1)\n");

    if (signalled(drm, rel, 1, 0)) DIE("released early: buffer 1's release point is signalled while it is shown");
    /* Commit 2 replaces buffer 1 (its acquire point 2 signalled up front): the compositor must
     * signal buffer 1's release point 1 once it is done with it. */
    if (drmSyncobjTimelineSignal(drm, &acq, (uint64_t[]){2}, 1)) DIE("signal 2: %s", strerror(errno));
    frame_done = 0;
    wl_surface_attach(surface, b2, 0, 0);
    wl_surface_damage_buffer(surface, 0, 0, 256, 256);
    wp_linux_drm_syncobj_surface_v1_set_acquire_point(sync, acq_tl, 0, 2);
    wp_linux_drm_syncobj_surface_v1_set_release_point(sync, rel_tl, 0, 2);
    wl_callback_add_listener(wl_surface_frame(surface), &frame_listener, NULL);
    wl_surface_commit(surface);
    pump(display, 5000, &frame_done);
    for (int i = 0; i < 50 && !signalled(drm, rel, 1, 0); i++) pump(display, 100, NULL);
    if (!signalled(drm, rel, 1, 0)) DIE("released: no (release point 1 not signalled 5 s after buffer 1 was replaced)");
    printf("released: yes (buffer 1's release point signalled after it was replaced)\n");
    printf("done\n");
    return 0;
}
