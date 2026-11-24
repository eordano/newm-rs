{ pkgs }:
{
  newm-rs = package: {
    name = "newm-rs";
    inherit package;
    session = "${package}/bin/newm-rs-session";
    unit = "newm-rs.service";
    ipc = "${package}/bin/newm-rs";
    process = "newm-rs";
    desktop = "niri:KDE";
    # xwayland-satellite with the five patches in patches/.
    xwaylandSatellite = pkgs.xwayland-satellite.overrideAttrs (old: {
      patches = (old.patches or [ ]) ++ [
        ./patches/xwayland-satellite-popup-parent.patch
        ./patches/xwayland-satellite-opaque-region.patch
        ./patches/xwayland-satellite-wayland-to-x11-dnd.patch
        ./patches/xwayland-satellite-x11-drags.patch
        ./patches/xwayland-satellite-syslog-level.patch
      ];
    });
    env = {
      RUST_LOG = "newm_rs=info,newm_rs::idle=debug";
    };
    # A nested session of this compositor inside the VM's session (its winit backend), and a
    # nested wlroots compositor (sway's Wayland backend).
    nested = "newm-rs";
    nestedOthers = [ "sway" ];
    vmKdl = "";
    xfail = {
      # linux-drm-syncobj-v1 applies to dmabuf buffers only; if the VM's EGL refuses the probe's
      # dumb-buffer dmabuf (create_immed failed), no syncobj commit is possible. Hardware EGL
      # (Intel's i915) imports it.
      explicit-sync = "the VM's software EGL renderer imports no dmabuf, so no syncobj commit";
      two-outputs = "a software-only secondary GPU's output cannot be driven: the primary renderer's formats meet none of its planes (NoSupportedPlaneFormat), and niri skips software EGL on secondary GPUs; dual-GPU hosts with hardware EGL on both are not affected";
      portal-screenshot = "the portal's Screenshot fails in the compat VM only (no answer from the backend there): open";
      screencast-pipewire = "the portal's ScreenCast fails in the compat VM only: open";
      idle-inhibit-gtk4 = "the GTK probe never acted in the VM (its app log stays empty; the portal frontend is suspected slow there); newm-rs honours Wayland and D-Bus holds (mpv, ScreenSaver cases)";
      idle-kde-inhibit-portal = "the portal probe's request came only after the case ended (portal frontend suspected slow in the VM)";
    };
  };

}
