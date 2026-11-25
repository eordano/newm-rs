# This flake file is community maintained
{
  description = "newm-rs: newm's strict tile canvas on niri.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  # Builds the dependencies as their own derivation, so CI caches them between runs.
  inputs.crane.url = "github:ipetkov/crane";

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      revision = self.shortRev or self.dirtyShortRev or "unknown";
      newm-rs-package =
        {
          lib,
          craneLib,
          cairo,
          dbus,
          libGL,
          libdisplay-info_0_3,
          libinput,
          seatd,
          libxkbcommon,
          libgbm,
          linux-pam,
          pango,
          pipewire,
          pixman,
          pkg-config,
          rustPlatform,
          systemd,
          systemdLibs,
          wayland,
          withDbus ? true,
          withSystemd ? true,
          withScreencastSupport ? true,
          withDinit ? false,
          # newm's tile canvas; without it the build keeps niri's scrolling layout.
          withCanvas ? true,
          # Text rendered with pango (hotkey overlay, notifications, titles). newm-rs builds
          # without it and draws stand-ins.
          withText ? false,
          # The canvas usage log, for tuning placement; off unless asked for.
          withTelemetry ? false,
          # The animated synth background (`synth-background { }`), with react-to-sound through
          # PipeWire; or the background alone, without PipeWire.
          withSynth ? false,
          withSynthBackground ? false,
          # The screen locker, newm-rs-lock, beside the compositor.
          withLocker ? true,
          # Run the test suite as part of the build.
          withTests ? true,
          # Cargo's profile: "release", or "dev" for a quicker, unstripped debug build.
          profile ? "release",
        }:
        let
          features =
            lib.optional withCanvas "canvas"
            ++ lib.optional withDbus "dbus"
            ++ lib.optional withDinit "dinit"
            ++ lib.optional withScreencastSupport "xdp-gnome-screencast"
            ++ lib.optional withSystemd "systemd"
            ++ lib.optional withText "text"
            ++ lib.optional withTelemetry "telemetry"
            ++ lib.optional withSynth "synth"
            ++ lib.optional (withSynthBackground && !withSynth) "synth-visual";
          packages = [ "newm-rs" ] ++ lib.optional withLocker "newm-rs-lock";

          # What both the dependencies' build and newm-rs's own share, so that the second reuses
          # the first's target directory: the same source layout, inputs, features and flags.
          # The dependencies build from a copy of the source with only the manifests, so their
          # derivation (and its cache entry) changes only when Cargo.lock or a manifest does.
          commonArgs = {
            pname = "newm-rs";
            version = revision;

            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./niri-config
                ./niri-ipc
                ./niri-visual-tests
                ./newm-canvas
                ./newm-kde
                ./newm-rs-lock
                ./newm-synth
                ./resources
                ./src
                ./Cargo.toml
                ./Cargo.lock
                ./build.rs
              ];
            };

            strictDeps = true;
            CARGO_PROFILE = profile;

            cargoExtraArgs = lib.concatStringsSep " " (
              [
                "--locked"
                "--no-default-features"
              ]
              ++ lib.optional (features != [ ]) "--features ${lib.concatStringsSep "," features}"
              ++ map (p: "--package ${p}") packages
            );
            # The tests also cover the canvas crate.
            cargoTestExtraArgs = "--package newm-canvas -- --skip=::egl";
            doCheck = withTests;

            nativeBuildInputs = [
              rustPlatform.bindgenHook
              pkg-config
            ];

            buildInputs = [
              dbus
              libGL
              libdisplay-info_0_3
              libinput
              seatd
              libxkbcommon
              libgbm
              linux-pam # newm-rs-lock
              pixman
              wayland
            ]
            ++ lib.optionals withText [
              cairo
              pango
            ]
            ++ lib.optional (withDbus || withScreencastSupport || withSystemd) dbus
            ++ lib.optional (withScreencastSupport || withSynth) pipewire
            # Also includes libudev
            ++ lib.optional withSystemd systemd
            # libudev alone, which the session backend links without the rest of systemd.
            ++ lib.optional (!withSystemd) systemdLibs;

            # Force linking with libEGL and libwayland-client so they end up in RPATH and
            # can be discovered by `dlopen()`
            RUSTFLAGS = toString (
              map (arg: "-C link-arg=" + arg) [
                "-Wl,--push-state,--no-as-needed"
                "-lEGL"
                "-lwayland-client"
                "-Wl,--pop-state"
              ]
            );
          };

          # A fixed version: the commit would change the dependencies' derivation on every commit.
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { version = "0"; });
        in
        craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;

            postPatch = ''
              patchShebangs resources/newm-rs-session
            '';

            # ever since this commit:
            # https://github.com/niri-wm/niri/commit/771ea1e81557ffe7af9cbdbec161601575b64d81
            # niri now runs an actual instance of the real compositor (with a mock backend) during
            # tests and thus creates a real socket file in the runtime dir.
            # this is fine for our build, we just need to make sure it has a directory to write to.
            preCheck = ''
              export XDG_RUNTIME_DIR="$(mktemp -d)"
            '';

            postInstall = ''
              install -Dm755 resources/newm-rs-session -t $out/bin

              install -Dm644 resources/niri-portals.conf -t $out/share/xdg-desktop-portal
              install -Dm644 /dev/stdin $out/share/wayland-sessions/newm-rs.desktop <<EOF
              [Desktop Entry]
              Name=newm-rs
              Comment=newm's tile canvas on niri
              Exec=$out/bin/newm-rs-session
              Type=Application
              DesktopNames=niri;KDE
              EOF
            ''
            + lib.optionalString withSystemd ''
              sed -e "s|^ExecStart=newm-rs --session|ExecStart=$out/bin/newm-rs --session|" \
                  resources/newm-rs.service > newm-rs.service
              install -Dm644 newm-rs.service -t $out/lib/systemd/user
              install -Dm644 resources/newm-rs-shutdown.target -t $out/lib/systemd/user
            '';

            dontStrip = profile != "release";

            NIRI_BUILD_COMMIT = revision;

            passthru = {
              providedSessions = [ "newm-rs" ];
              inherit cargoArtifacts;
            };

            meta = {
              description = "newm's strict tile canvas on niri";
              homepage = "https://github.com/eordano/newm-rs";
              license = lib.licenses.gpl3Plus;
              mainProgram = "newm-rs";
              platforms = lib.platforms.linux;
            };
          }
        );

      inherit (nixpkgs) lib;
      # Support all Linux systems that the nixpkgs flake exposes
      systems = lib.intersectLists lib.systems.flakeExposed lib.platforms.linux;

      forAllSystems = lib.genAttrs systems;
      nixpkgsFor = forAllSystems (system: nixpkgs.legacyPackages.${system});

      compat =
        system: strict:
        let
          pkgs = nixpkgsFor.${system};
          compositors = import ./compat/compositors.nix { inherit pkgs; };
        in
        import ./compat {
          inherit pkgs strict;
          compositor = compositors.newm-rs (pkgs.callPackage ./compat/newm-rs.nix { });
        };
    in
    {
      checks = forAllSystems (
        system:
        {
          # We use the debug build here to save a bit of time
          inherit (self.packages.${system}) newm-rs-debug;
        }
        // lib.optionalAttrs (system == "x86_64-linux") (compat system true)
      );

      legacyPackages.x86_64-linux.compat-report = compat "x86_64-linux" false;

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgsFor.${system};
          rustfmt' = pkgs.rustfmt.override { asNightly = true; };
          # cargo builds take the default features, text included.
          newm-rs = self.packages.${system}.newm-rs.override { withText = true; };
        in
        {
          default = pkgs.mkShell {
            packages = builtins.attrValues {
              inherit (pkgs)
                rustc
                cargo
                clippy
                cargo-insta
                ;
              inherit rustfmt';
            };

            nativeBuildInputs = [
              pkgs.rustPlatform.bindgenHook
              pkgs.pkg-config
              pkgs.wrapGAppsHook4 # For `niri-visual-tests`
            ];

            buildInputs = newm-rs.buildInputs ++ [
              pkgs.libadwaita # For `niri-visual-tests`
            ];

            env = {
              # WARN: Do not overwrite this variable in your shell!
              # It is required for `dlopen()` to work on some libraries; see the comment
              # in the package expression
              #
              # This should only be set with `RUSTFLAGS="$RUSTFLAGS -C your-flags"`
              RUSTFLAGS = newm-rs.RUSTFLAGS;
            };
          };
        }
      );

      formatter = forAllSystems (system: nixpkgsFor.${system}.nixfmt-rfc-style);

      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgsFor.${system};
          newm-rs = pkgs.callPackage newm-rs-package { craneLib = crane.mkLib pkgs; };
        in
        {
          inherit newm-rs;

          # NOTE: This is for development purposes only
          #
          # It is primarily to help with quickly iterating on
          # changes made to the above expression - though it is
          # also not stripped in order to better debug newm-rs itself
          newm-rs-debug = newm-rs.override { profile = "dev"; };

          default = newm-rs;
        }
      );

      overlays.default = final: _: {
        newm-rs = final.callPackage newm-rs-package { craneLib = crane.mkLib final; };
      };
    };
}
