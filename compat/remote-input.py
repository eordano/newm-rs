# KDE Connect's mousepad asks the RemoteDesktop portal for a session; xdg-desktop-portal-kde
# serves it with org_kde_kwin_fake_input once ConnectToEIS fails (only KWin has EIS). The probe
# makes KDE Connect's calls; wev, focused, must read the motion, the click and the keys.


def remote_input(info):
    # Pre-authorize host apps (empty app id) as KDE's portal reads it, so no dialog waits.
    alice("busctl --user call org.freedesktop.impl.portal.PermissionStore "
          "/org/freedesktop/impl/portal/PermissionStore org.freedesktop.impl.portal.PermissionStore "
          "SetPermission sbssas kde-authorized true remote-desktop '' 1 yes")
    # stdbuf: wev's stdout to a file is block-buffered, and the log is read while wev runs.
    w = new_window("stdbuf -oL wev > /tmp/probe/wev.log 2>&1", "wev", match="wev")
    try:
        focus(w)
        # Put the pointer in the middle of wev's tile with the uinput tablet (absolute over the
        # output's logical size), so the remote relative motion and click land on wev.
        out = next(iter(js("outputs").values()))["logical"]
        lay = wait(lambda: (get(w["id"]) or {}).get("layout", {}).get("tile_pos_in_workspace_view")
                   and get(w["id"])["layout"], "wev's tile on screen")
        x = int(lay["tile_pos_in_workspace_view"][0] + lay["tile_size"][0] / 2)
        y = int(lay["tile_pos_in_workspace_view"][1] + lay["tile_size"][1] / 2)
        info["pointer_at"] = [x, y]
        sh(f"uin 'abs {x} {y} {out['width']} {out['height']}'", timeout=30)
        wait(lambda: "wl_pointer] enter" in alice("cat /tmp/probe/wev.log"),
             "the pointer over wev (wl_pointer enter)", 15)
        rc, out = alice_try("probe-remote-input 'motion 40 30' click 'keysym 0x68' "
                            "'keysym 0x49' > /tmp/probe/remote-input.log 2>&1; "
                            "cat /tmp/probe/remote-input.log", timeout=90)
        info["probe"] = out.strip().splitlines()[-6:]
        globs = alice("wayland-info")
        # Only xdg-desktop-portal-kde sees org_kde_kwin_fake_input; the compositor logs its
        # authenticate.
        info["portal_authenticated"] = "fake input" in journal() and "authenticated" in journal()
        # Explicit sync is offered only when the primary DRM device signals syncobj points
        # through an eventfd; whether the VM's virtio-gpu does is recorded here.
        info["syncobj_global"] = "wp_linux_drm_syncobj_manager_v1" in globs
        time.sleep(1)
        expect(rc == 0 and "done" in out, f"the portal probe failed: {out[-400:]}")
        log = alice("cat /tmp/probe/wev.log")
        info["wev_lines"] = len(log.splitlines())
        motion = "[wl_pointer] motion" in log or "motion:" in log
        button = re.search(r"button: \d+ \(left\), state: \d+ \(pressed\)", log) is not None
        key_h = re.search(r"sym: +h ", log) is not None
        key_I = re.search(r"sym: +I ", log) is not None
        info.update(motion=motion, button=button, key_h=key_h, key_I=key_I)
        expect(motion and button, "wev saw no remote pointer motion or click")
        expect(key_h and key_I, "wev saw no remote 'h' and 'I' (keysym, 'I' through Shift)")
    finally:
        close(w, "wev")


def explicit_sync(info):
    # linux-drm-syncobj-v1, offered when the primary DRM device signals syncobj points through
    # an eventfd: a commit waits for its acquire point and the replaced buffer's release point
    # gets signalled (probes/syncobj.c on the VM's virtio-gpu).
    rc, out = alice_try("syncobj-probe /dev/dri/card0 2>&1", timeout=60)
    info["probe"] = out.strip().splitlines()[-6:]
    expect(rc == 0 and "done" in out, f"the syncobj probe failed: {out[-400:]}")


wait_session()
case("remote-input", remote_input)
case("explicit-sync", explicit_sync)
with open(os.path.join(OUT, f"journal-{COMPOSITOR}-portals.txt"), "w") as f:
    f.write(alice("journalctl --user -b -q -o short-monotonic --no-pager "
                  "-u plasma-xdg-desktop-portal-kde -u xdg-desktop-portal "
                  "-u xdg-permission-store; systemctl --user show-environment"))
finish()
