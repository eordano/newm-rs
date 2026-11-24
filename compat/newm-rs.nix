{
  lib,
  rustPlatform,
  pkg-config,
  dbus,
  libdisplay-info_0_3,
  libglvnd,
  libinput,
  libxkbcommon,
  libgbm,
  linux-pam,
  pipewire,
  pixman,
  seatd,
  systemdLibs,
  wayland,
  # The usage log ($XDG_STATE_HOME/newm-rs/usage.jsonl), built in when Cargo.toml has the feature.
  withTelemetry ? true,
}:
let
  telemetry = withTelemetry && (lib.importTOML ../Cargo.toml).features ? telemetry;
in
rustPlatform.buildRustPackage {
  pname = "newm-rs";
  version = "26.4.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../build.rs
      ../src
      ../resources
      ../niri-config
      ../niri-ipc
      ../niri-visual-tests
      ../newm-rs-lock
      ../newm-canvas
      ../newm-kde
      ../newm-synth
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    allowBuiltinFetchGit = true;
  };

  postPatch = ''
    patchShebangs resources/newm-rs-session
  '';

  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    dbus
    libdisplay-info_0_3
    libglvnd
    libinput
    libxkbcommon
    libgbm
    linux-pam
    pipewire
    pixman
    seatd
    systemdLibs
    wayland
  ];

  buildNoDefaultFeatures = true;
  buildFeatures = [
    "canvas"
    "dbus"
    "systemd"
    "xdp-gnome-screencast"
  ]
  ++ lib.optional telemetry "telemetry";
  cargoBuildFlags = [
    "--package"
    "newm-rs"
    "--package"
    "newm-rs-lock"
  ];
  doCheck = false;

  env = {
    RUSTFLAGS = toString (
      map (arg: "-C link-arg=" + arg) [
        "-Wl,--push-state,--no-as-needed"
        "-lEGL"
        "-lwayland-client"
        "-Wl,--pop-state"
      ]
    );
    NIRI_BUILD_COMMIT = "newm-rs";
  };

  postInstall = ''
    install -Dm0755 resources/newm-rs-session -t $out/bin
    sed -e "s|^ExecStart=newm-rs --session|ExecStart=$out/bin/newm-rs --session|" \
        -e '/^\[Service\]/a X-RestartIfChanged=false' \
        resources/newm-rs.service > newm-rs.service
    install -Dm0644 newm-rs.service -t $out/lib/systemd/user
    install -Dm0644 resources/newm-rs-shutdown.target -t $out/lib/systemd/user
  '';

  meta = {
    description = "newm's strict tile canvas on niri";
    license = lib.licenses.gpl3Only;
    mainProgram = "newm-rs";
    platforms = lib.platforms.linux;
  };
}
