{
  description = "newm-rs in the browser: a static site that boots a Linux guest running newm-rs";

  inputs = {
    # The page, the engine (unkernel, from the npm registry tarball) and the Wayland compositor
    # the guest's windows are drawn by (wland) come from trynix's tree, built with its own
    # nix files. Its nixpkgs is followed so the guest's newm-rs links the same glibc as the GL
    # driver the page serves to every guest.
    trynix.url = "github:fzakaria/trynix";
    nixpkgs.follows = "trynix/nixpkgs";
    newm-rs = {
      url = "path:..";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      trynix,
      newm-rs,
    }:
    let
      inherit (nixpkgs) lib;
      # The guest runs aarch64 user space whatever the host is. Its libraries are nixpkgs' native
      # aarch64 builds, which cache.nixos.org serves to the browser.
      guestPkgs = import nixpkgs {
        system = "aarch64-linux";
        overlays = [ newm-rs.overlays.default ];
      };
      # The demo as built on `system`: natively on aarch64, with a cross rustc on x86_64; the
      # small session, or the full one.
      demoFor =
        system: full:
        if system == "aarch64-linux" then
          guestPkgs.callPackage ./demo.nix { inherit full; }
        else
          let
            cross = nixpkgs.legacyPackages.${system}.pkgsCross.aarch64-multiplatform.appendOverlays [
              newm-rs.overlays.default
            ];
          in
          guestPkgs.callPackage ./demo.nix {
            inherit full;
            crossStdenv = cross.stdenv;
            newm-rs = cross.newm-rs.override {
              inherit (guestPkgs)
                cairo
                dbus
                libGL
                libdisplay-info_0_3
                libinput
                seatd
                libxkbcommon
                libgbm
                linux-pam
                pango
                pipewire
                pixman
                systemd
                wayland
                ;
            };
          };
      forSystems = lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    {
      packages = forSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          demo = demoFor system false;
          demo-full = demoFor system true;
          site = pkgs.callPackage ./site.nix {
            inherit trynix demo demo-full;
          };
        in
        {
          inherit site demo demo-full;
          inherit (demo) newm-rs;
          default = site;

          # Serve the built site on http://localhost:8000/ (or the port given); the demo is at
          # /try/.
          serve = pkgs.writeShellScriptBin "newm-rs-web-serve" ''
            exec ${pkgs.python3}/bin/python3 -m http.server --bind 127.0.0.1 \
              --directory ${site} "''${1:-8000}"
          '';
        }
      );

      apps = forSystems (system: {
        serve = {
          type = "app";
          program = "${self.packages.${system}.serve}/bin/newm-rs-web-serve";
        };
      });
    };
}
