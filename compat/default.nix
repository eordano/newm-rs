{
  pkgs,
  compositor,
  strict ? true,
}:
let
  inherit (pkgs) lib;
  c = compositor;

  uin = pkgs.writeScriptBin "uin" (
    "#!${pkgs.python3.withPackages (p: [ p.evdev ])}/bin/python3\n" + builtins.readFile ./uin.py
  );

  asAlice = pkgs.writeShellScriptBin "as-alice" ''
    exec ${pkgs.util-linux}/bin/runuser -u alice -- env XDG_RUNTIME_DIR=/run/user/1000 \
      DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c '
        eval "$(systemctl --user show-environment \
          | grep -E "^(WAYLAND_DISPLAY|DISPLAY|NIRI_SOCKET|XDG_SESSION_ID|XDG_CURRENT_DESKTOP|PATH)=" \
          | sed "s/^/export /")"
        exec sh -c "$1"' sh "$1"
  '';

  typelibs =
    extra:
    lib.makeSearchPath "lib/girepository-1.0" (
      with pkgs;
      [
        gtk3
        gtk4
        pango.out
        gdk-pixbuf
        harfbuzz
        atk
        graphene
        glib.out
        gobject-introspection
      ]
      ++ extra
    );
  giTypelibs = typelibs [ ];

  gtkProbes = map (
    name:
    pkgs.writeShellScriptBin "probe-${name}" ''
      export GI_TYPELIB_PATH=${giTypelibs}
      exec ${pkgs.python3.withPackages (p: [ p.pygobject3 ])}/bin/python3 ${./probes + "/${name}.py"} "$@"
    ''
  ) [
    "gtk"
    "inhibit"
    "bell"
    "dialog"
  ];

  qtPlugins = lib.makeSearchPath "lib/qt-6/plugins" [
    pkgs.qt6.qtbase
    pkgs.qt6.qtwayland
    pkgs.kdePackages.plasma-integration
    pkgs.kdePackages.breeze
  ];

  probeQt = pkgs.writeShellScriptBin "probe-qt" ''
    export QT_PLUGIN_PATH=${qtPlugins}''${QT_PLUGIN_PATH:+:$QT_PLUGIN_PATH}
    exec ${pkgs.python3.withPackages (p: [ p.pyqt6 ])}/bin/python3 ${./probes/qt.py} "$@"
  '';

  kwinRef = pkgs.writeShellScriptBin "kwin-ref" ''
    unset WAYLAND_DISPLAY DISPLAY
    export KWIN_WAYLAND_NO_PERMISSION_CHECKS=1
    exec ${pkgs.dbus}/bin/dbus-run-session -- ${pkgs.kdePackages.kwin}/bin/kwin_wayland \
      --virtual --socket kwin-ref --width 1280 --height 800 --no-lockscreen --no-global-shortcuts
  '';

  # Two virtio GPUs at fixed PCI slots, named /dev/dri/gpu{0,1}-{card,render} by udev:
  # WLR_DRM_DEVICES is colon-separated, so it cannot take the by-path names.
  twoGpus = {
    qemu = [
      "-vga none"
      "-device virtio-gpu-pci,xres=2560,yres=1080,addr=0x18"
      "-device virtio-gpu-pci,xres=1920,yres=1080,addr=0x19"
    ];
    udev = ''
      SUBSYSTEM=="drm", KERNEL=="card[0-9]*", KERNELS=="0000:00:18.0", SYMLINK+="dri/gpu0-card"
      SUBSYSTEM=="drm", KERNEL=="renderD[0-9]*", KERNELS=="0000:00:18.0", SYMLINK+="dri/gpu0-render"
      SUBSYSTEM=="drm", KERNEL=="card[0-9]*", KERNELS=="0000:00:19.0", SYMLINK+="dri/gpu1-card"
      SUBSYSTEM=="drm", KERNEL=="renderD[0-9]*", KERNELS=="0000:00:19.0", SYMLINK+="dri/gpu1-render"
    '';
  };

  # The second output's scale stays 1, so a case can tell where a window opened.
  twoOutputsKdl = ''
    output "Virtual-2" {
        scale 1
        position x=2048 y=0
    }
  '';

  # Each suite: its script (<name>.py), extra script libraries (libs), and the guest's QEMU devices,
  # udev rules, extra KDL, environment and extra NixOS modules (modules).
  tests = {
    smoke = { };
    gaps = { };
    remote-input.modules = [ (import ./remote-input.nix { inherit pkgs; }) ];
    input = {
      modules = [ protocols ];
      libs = [ "proto_lib" ];
    };
    desktop = {
      modules = [ protocols ];
      libs = [ "proto_lib" ];
    };
    robust = {
      modules = [ protocols ];
      libs = [ "proto_lib" ];
    };
    backends = {
      libs = [ "backends_lib" ];
      inherit (twoGpus) qemu udev;
      kdl = twoOutputsKdl + ''
        debug {
            render-drm-device "/dev/dri/gpu0-render"
        }
      '';
      env.WLR_DRM_DEVICES = "/dev/dri/gpu0-card:/dev/dri/gpu1-card";
    };
    two-gpu = {
      libs = [ "backends_lib" ];
      inherit (twoGpus) qemu udev;
      kdl = ''
        debug {
            render-drm-device "/dev/dri/gpu0-render"
            ignore-drm-device "/dev/dri/gpu1-render"
        }
      '';
      env.WLR_DRM_DEVICES = "/dev/dri/gpu0-card";
    };
  };

  nestedKdl = pkgs.writeText "nested.kdl" ''
    prefer-no-csd
    hotkey-overlay {
        skip-at-startup
    }
    spawn-at-startup "probe-gtk" "4.0" "probe-nested" "/tmp/probe/nested.json"
  '';
  nestedSway = pkgs.writeText "nested-sway" ''
    exec probe-gtk 4.0 probe-nested /tmp/probe/nested.json
  '';

  protocols = import ./protocols.nix { inherit pkgs c typelibs; };

  guest =
    {
      qemu ? [
        "-vga none"
        "-device virtio-gpu-pci,xres=2560,yres=1080"
      ],
      kdl ? "",
      env ? { },
      udev ? "",
      ...
    }:
    { ... }:
    let
      config = import ./config.nix {
        inherit pkgs;
        c = c // {
          vmKdl = c.vmKdl + kdl;
        };
      };
    in
    {
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = qemu;
      };
      services.udev.extraRules = udev;
      environment.etc."newm-compat/nested.kdl".source = nestedKdl;
      environment.etc."newm-compat/nested-sway".source = nestedSway;
      hardware.uinput.enable = true;
      hardware.graphics.enable = true;
      fonts.enableDefaultPackages = true;
      programs.gdk-pixbuf.modulePackages = [ pkgs.librsvg ];

      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        password = "correct-horse-7";
      };
      services.getty.autologinUser = "alice";
      environment.loginShellInit = ''
        if [ "$(tty)" = /dev/tty1 ] && [ -z "$NEWM_VM_SESSION" ]; then
          export NEWM_VM_SESSION=1
          export XDG_SESSION_TYPE=wayland
          export NIRI_CONFIG=${config}
          ${c.session}
          exit
        fi
      '';
      environment.sessionVariables = c.env // env // {
        XDG_CURRENT_DESKTOP = c.desktop;
        QT_QPA_PLATFORMTHEME = "kde";
        QT_PLUGIN_PATH = qtPlugins;
        NIXOS_OZONE_WL = "1";
        GST_PLUGIN_SYSTEM_PATH_1_0 = lib.makeSearchPath "lib/gstreamer-1.0" [
          pkgs.gst_all_1.gst-plugins-base
          pkgs.gst_all_1.gst-plugins-good
        ];
      };

      systemd.packages = [ c.package ];
      systemd.tmpfiles.rules = [
        "d /tmp/shots 1777 root root -"
        "d /tmp/probe 1777 root root -"
      ];
      security.pam.services.newm-lock = { };

      systemd.services.uin = {
        description = "uinput devices for the tests";
        wantedBy = [ "multi-user.target" ];
        serviceConfig.ExecStart = "${uin}/bin/uin serve";
      };

      environment.etc."newm-compat/page.html".source = ./probes/page.html;

      environment.systemPackages = gtkProbes ++ [
        c.package
        uin
        asAlice
        probeQt
        kwinRef
        pkgs.dbus
        pkgs.foot
        pkgs.xterm
        pkgs.grim
        pkgs.jq
        pkgs.swaybg
        pkgs.wlr-randr
        pkgs.wlopm
        pkgs.wayland-utils
        pkgs.gtk4.dev
        pkgs.gtk3.dev
        pkgs.zenity
        pkgs.kdePackages.kate
        pkgs.kdePackages.kwin
        pkgs.kdePackages.breeze
        pkgs.kdePackages.breeze-icons
        pkgs.chromium
        pkgs.kbd
        pkgs.sway
        pkgs.niri
      ];
    };

  test =
    name: t:
    pkgs.testers.runNixOSTest {
      name = "compat-${c.name}-${name}${lib.optionalString (!strict) "-report"}";
      nodes.machine = {
        imports = [ (guest t) ] ++ (t.modules or [ ]);
      };
      extraPythonPackages = p: [ p.pillow ];
      globalTimeout = if (t.modules or [ ]) != [ ] then 5400 else 3600;
      skipTypeCheck = true;
      skipLint = true;
      testScript = ''
        TEST = ${builtins.toJSON name}
        COMPOSITOR = ${builtins.toJSON c.name}
        IPC = ${builtins.toJSON c.ipc}
        UNIT = ${builtins.toJSON c.unit}
        PROCESS = ${builtins.toJSON c.process}
        XFAIL = ${builtins.toJSON c.xfail}
        NESTED = ${builtins.toJSON (c.nested or null)}
        NESTED_OTHERS = ${builtins.toJSON (c.nestedOthers or [ ])}
        STRICT = ${if strict then "True" else "False"}
      ''
      + builtins.readFile ./lib.py
      + lib.concatMapStrings (l: builtins.readFile (./. + "/${l}.py")) (t.libs or [ ])
      + builtins.readFile (./. + "/${name}.py");
    };
in
lib.mapAttrs' (
  name: t: lib.nameValuePair (if name == "smoke" then "compat-${c.name}" else "compat-${c.name}-${name}") (test name t)
) tests
