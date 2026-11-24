{ pkgs, c }:
pkgs.writeText "${c.name}-compat.kdl" ''
  prefer-no-csd
  environment {
      XDG_CURRENT_DESKTOP "${c.desktop}"
  }
  hotkey-overlay {
      skip-at-startup
  }
  ${pkgs.lib.optionalString (c.xwaylandSatellite != null) ''
    xwayland-satellite {
        path "${c.xwaylandSatellite}/bin/xwayland-satellite"
    }
  ''}
  output "Virtual-1" {
      scale 1.25
  }
  input {
      keyboard {
          xkb {
              layout "us"
          }
          repeat-delay 600
          repeat-rate 25
      }
  }
  layout {
      gaps 10
      canvas {
          tile-width 119
          tile-height 113
          default-width { fixed 960; }
          default-height { proportion 0.666700; }
      }
  }
  ${c.vmKdl}
  spawn-at-startup "${pkgs.swaybg}/bin/swaybg" "-c" "#203040"
  binds {
      Mod+Shift+p { quit skip-confirmation=true; }
      Mod+t { spawn-sh "touch /tmp/probe/fired-t"; }
      Mod+y allow-inhibiting=false { spawn-sh "touch /tmp/probe/fired-y"; }
      Mod+Escape { toggle-keyboard-shortcuts-inhibit; }
      Mod+F12 { spawn-sh "touch /tmp/probe/bind-fired"; }
  }
''
