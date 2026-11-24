# Extra guest for the remote-input test: the RemoteDesktop portal as KDE Connect reaches it
# (xdg-desktop-portal -> xdg-desktop-portal-kde -> org_kde_kwin_fake_input), the KDE portal
# pre-authorized for host apps so no consent dialog waits, and wev to read what arrives.
# xdg-desktop-portal-kde builds its RemoteDesktop, ScreenCast, Screenshot and GlobalShortcuts
# portals and its Wayland integration only when XDG_CURRENT_DESKTOP is exactly "KDE"
# (desktopportal.cpp), so its unit gets that.
{ pkgs }:
{ ... }:
let
  # probes/syncobj.c: a dumb-buffer dmabuf committed with linux-drm-syncobj-v1 points.
  syncobjProbe = pkgs.stdenv.mkDerivation {
    name = "syncobj-probe";
    dontUnpack = true;
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wayland-scanner
    ];
    buildInputs = [
      pkgs.wayland
      pkgs.libdrm
    ];
    buildPhase = ''
      p=${pkgs.wayland-protocols}/share/wayland-protocols
      for x in stable/xdg-shell/xdg-shell.xml stable/linux-dmabuf/linux-dmabuf-v1.xml \
          staging/linux-drm-syncobj/linux-drm-syncobj-v1.xml; do
        n=$(basename $x .xml)
        wayland-scanner client-header $p/$x $n-client-protocol.h
        wayland-scanner private-code $p/$x $n-protocol.c
      done
      $CC -O1 -Wall -Werror -I. ${./probes/syncobj.c} *-protocol.c -o syncobj-probe \
        $(pkg-config --cflags --libs wayland-client libdrm)
    '';
    installPhase = "install -Dm755 syncobj-probe $out/bin/syncobj-probe";
  };
in
{
  xdg.portal = {
    enable = true;
    extraPortals = [ pkgs.kdePackages.xdg-desktop-portal-kde ];
    config.common = {
      default = [ "kde" ];
      "org.freedesktop.impl.portal.RemoteDesktop" = [ "kde" ];
    };
  };
  systemd.user.services.plasma-xdg-desktop-portal-kde = {
    overrideStrategy = "asDropin";
    environment = {
      XDG_CURRENT_DESKTOP = "KDE";
      QT_LOGGING_RULES = "xdp-kde*.debug=true";
    };
  };
  environment.systemPackages = [
    syncobjProbe
    pkgs.wev
    (pkgs.writeShellScriptBin "probe-remote-input" ''
      exec ${pkgs.python3.withPackages (p: [ p.dbus-next ])}/bin/python3 ${./probes/remote_input.py} "$@"
    '')
  ];
}
