# What the protocol cases (input.py, desktop.py, robust.py) add to the compat guest: the clients
# they drive, probe wrappers, fcitx5 with pinyin, PipeWire and xdg-desktop-portal (kde for
# dialogs, gnome for capture), a pen tablet, a touchscreen and a
# pinch clickpad (uin.py run as uinx), and PAM for swaylock.
{
  pkgs,
  c,
  typelibs,
}:
let
  inherit (pkgs) lib;

  giTypelibs = typelibs [ pkgs.gtk4-layer-shell ];

  py = pkgs.python3.withPackages (p: [
    p.pygobject3
    p.pyqt6
    p.pywayland
    p.pysdl2
  ]);

  probe = pkgs.writeShellScriptBin "probe" ''
    # probe NAME ARGS...: runs compat/probes/NAME.py with GTK, Qt, pywayland and SDL at hand.
    name=$1; shift
    export GI_TYPELIB_PATH=${giTypelibs}
    export PYSDL2_DLL_PATH=${pkgs.SDL2}/lib
    export GST_PLUGIN_PATH=${gstPath}
    if [ "$name" = gtkx ] && [ "$1" = layer ]; then
      export LD_PRELOAD=${pkgs.gtk4-layer-shell}/lib/libgtk4-layer-shell.so
    fi
    exec ${py}/bin/python3 ${./probes}/$name.py "$@"
  '';

  uinx = pkgs.writeScriptBin "uinx" (
    "#!${pkgs.python3.withPackages (p: [ p.evdev ])}/bin/python3\n" + builtins.readFile ./uin.py
  );

  fcitx = pkgs.qt6Packages.fcitx5-with-addons.override {
    addons = [ pkgs.qt6Packages.fcitx5-chinese-addons ];
  };

  gstPath = lib.makeSearchPath "lib/gstreamer-1.0" [
    pkgs.gst_all_1.gstreamer.out
    pkgs.gst_all_1.gst-plugins-base
    pkgs.pipewire
  ];

  portalDesktop = "niri";
  capture = [ "gnome" ];
in
{
  services.pipewire = {
    enable = true;
    pulse.enable = false;
    wireplumber.enable = true;
  };
  xdg.portal = {
    enable = true;
    extraPortals = [
      pkgs.kdePackages.xdg-desktop-portal-kde
    ]
    ++ [ pkgs.xdg-desktop-portal-gnome ];
    config.${portalDesktop} = {
      default = [ "kde" ];
      "org.freedesktop.impl.portal.ScreenCast" = capture;
      "org.freedesktop.impl.portal.Screenshot" = capture;
    };
    wlr.settings.screencast = {
      chooser_type = "none";
      output_name = "Virtual-1";
    };
  };

  security.pam.services.swaylock = { };

  # fcitx5 ships an XDG autostart entry, and both sessions start xdg-desktop-autostart.target (niri's
  # unit wants it), so it ran from login with pinyin active and composed every key a case typed
  # (the robust after-probe, wtype, the GTK to Qt copy read nothing). It also ships a D-Bus
  # activation file, so a client asking for org.fcitx.Fcitx5 brought it back after the input-method
  # cases stopped it. Only those cases run it now (input.py fcitx_up spawns it, fcitx_down stops
  # it): a Hidden entry in the user's own autostart directory outranks every XDG_CONFIG_DIRS one,
  # and the user's own D-Bus service file, which the session bus reads first, never activates it.
  systemd.tmpfiles.rules =
    let
      hidden = pkgs.writeText "org.fcitx.Fcitx5.desktop" ''
        [Desktop Entry]
        Type=Application
        Name=Fcitx 5
        Hidden=true
      '';
      noActivation = pkgs.writeText "org.fcitx.Fcitx5.service" ''
        [D-BUS Service]
        Name=org.fcitx.Fcitx5
        Exec=${pkgs.coreutils}/bin/false
      '';
    in
    [
      "d /home/alice/.config 0755 alice users -"
      "d /home/alice/.config/autostart 0755 alice users -"
      "L+ /home/alice/.config/autostart/org.fcitx.Fcitx5.desktop - - - - ${hidden}"
      "d /home/alice/.local 0755 alice users -"
      "d /home/alice/.local/share 0755 alice users -"
      "d /home/alice/.local/share/dbus-1 0755 alice users -"
      "d /home/alice/.local/share/dbus-1/services 0755 alice users -"
      "L+ /home/alice/.local/share/dbus-1/services/org.fcitx.Fcitx5.service - - - - ${noActivation}"
    ];

  # As NixOS's Plasma module does: share/kglobalaccel holds the desktop files whose
  # X-KDE-Shortcuts both compositors bind (Spectacle's Print, Shift+Print, ...).
  environment.pathsToLink = [ "/share/kglobalaccel" ];

  systemd.services.uin-extra = {
    description = "more uinput devices for the protocol cases";
    wantedBy = [ "multi-user.target" ];
    serviceConfig.ExecStart = "${uinx}/bin/uinx serve";
  };

  environment.etc = {
    "xdg/fcitx5/profile".text = ''
      [Groups/0]
      Name=Default
      Default Layout=us
      DefaultIM=pinyin

      [Groups/0/Items/0]
      Name=keyboard-us
      Layout=

      [Groups/0/Items/1]
      Name=pinyin
      Layout=

      [GroupOrder]
      0=Default
    '';
    "xdg/fcitx5/config".text = ''
      [Behavior]
      ActiveByDefault=True
      ShareInputState=No
      ShowInputMethodInformation=False
    '';
  };

  environment.variables.GST_PLUGIN_PATH = gstPath;

  environment.systemPackages = [
    probe
    uinx
    fcitx
    pkgs.wtype
    pkgs.wlrctl
    pkgs.lswt
    pkgs.wl-clipboard
    pkgs.xclip
    pkgs.gammastep
    pkgs.swayidle
    pkgs.swaylock
    pkgs.weston
    pkgs.mpv
    pkgs.ffmpeg-headless
    pkgs.gst_all_1.gstreamer
    pkgs.kdePackages.spectacle
    pkgs.kdePackages.dolphin
    pkgs.libnotify
    pkgs.procps
    pkgs.libinput
  ];
}
