# What the browser guest runs: newm-rs built small (the canvas alone: no D-Bus, systemd,
# screencast or text rendering), foot with the author's terminal settings (foot.ini), tmux with
# their tmux config (tmux.conf) and fish, a newm-rs config, and `newm-rs-demo`, the command the
# page types at the guest's shell.
#
# Two sessions: the small one (`full = false`), the page's default, has only that, so a first
# visit fetches little; the full one adds waybar, Neovim as the editor and Alacritty on demand.
#
# `newm-rs` is the package to trim. On an x86_64 host it comes from the cross package set, so
# rustc runs natively rather than under binfmt, with the guest's own (native aarch64) libraries
# passed in; `crossStdenv` is then that set's stdenv, whose glibc and libgcc the binary is
# pointed away from, so the guest holds one glibc, the one cache.nixos.org has.
{
  lib,
  full ? false,
  newm-rs,
  crossStdenv ? null,
  stdenv,
  glibc,
  foot,
  fcft,
  libinput,
  tmux,
  tmuxPlugins,
  waybar,
  alacritty,
  dbus,
  fish,
  fishPlugins,
  python3,
  man-db,
  neovim,
  neovim-unwrapped,
  vimPlugins,
  writeTextDir,
  ncurses,
  runtimeShellPackage,
  coreutils,
  writeText,
  removeReferencesTo,
  runCommand,
  writeShellScript,
  runtimeShell,
}:
let
  # libinput without tablet support and its Lua plugins: libwacom brings GLib, 4 MB the browser
  # would fetch for tablets the guest does not have.
  libinput' = libinput.override {
    wacomSupport = false;
    luaSupport = false;
  };

  newm-rs' =
    (newm-rs.override {
      libinput = libinput';
      withDbus = false;
      withSystemd = false;
      withScreencastSupport = false;
      withText = false;
      withCanvas = true;
      # The synthwave background, without PipeWire (react-to-sound stays off).
      withSynthBackground = true;
      # The compositor alone: the locker and the tests stay out of the guest build.
      withLocker = false;
      withTests = false;
    }).overrideAttrs
      (old: {
        pname = "newm-rs-web";
        # Built for size as well as speed, since the browser fetches the program whole on every
        # first visit: whole-program LTO in one codegen unit, no unwinding tables, and no symbols
        # or line tables. The optimisation level stays the release one; the guest's CPUs are
        # emulated, and newm-rs draws every frame on them.
        CARGO_PROFILE_RELEASE_LTO = "fat";
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "1";
        CARGO_PROFILE_RELEASE_PANIC = "abort";
        CARGO_PROFILE_RELEASE_DEBUG = "false";
        CARGO_PROFILE_RELEASE_STRIP = "symbols";
        # The session script and its desktop files are for a login manager the guest does not
        # have; the script would also bring a second bash and glibc into the closure.
        postInstall = old.postInstall + ''
          rm -r $out/bin/newm-rs-session $out/share
        '';
        postFixup =
          (old.postFixup or "")
          + lib.optionalString (crossStdenv != null) ''
            f=$out/bin/newm-rs
            patchelf --set-interpreter ${glibc}/lib/ld-linux-aarch64.so.1 $f
            rpath=
            IFS=: read -ra dirs <<< "$(patchelf --print-rpath $f)"
            for dir in "''${dirs[@]}"; do
              case $dir in
                ${crossStdenv.cc.libc}* | ${crossStdenv.cc.cc.lib}*) ;;
                *) rpath+=$dir: ;;
              esac
            done
            patchelf --set-rpath "$rpath${glibc}/lib:${stdenv.cc.cc.lib}/lib" $f
            # patchelf leaves the old rpath's text behind in the string table.
            remove-references-to -t ${crossStdenv.cc.libc} -t ${crossStdenv.cc.cc.lib} $f
          '';
        nativeBuildInputs = old.nativeBuildInputs ++ lib.optional (crossStdenv != null) removeReferencesTo;
        disallowedReferences = lib.optionals (crossStdenv != null) [
          crossStdenv.cc.libc
          crossStdenv.cc.cc.lib
        ];
      });

  # The first window: what to press.
  keys = writeShellScript "newm-rs-demo-keys" ''
    cat <<'EOF'
    newm-rs: windows on an infinite canvas of tiles.

    Mod is Alt here (newm-rs runs nested in the page).

      Alt + wheel             zoom in and out
      Alt + = / Alt + -       zoom in / out
      Alt + 0                 back to 1:1
      Alt + O                 overview
      Alt + arrows            focus the next window; the camera follows
      Alt + Shift + Left/Right  send a window to an edge
      Alt + Return            a new terminal (tmux, fish)
    ${lib.optionalString full "  Alt + Shift + Return    Alacritty, drawn with the GPU\n"}  Alt + Q                 close the window
      Alt + Z / Shift+Z       undo / redo

    ${
      if full then
        ''
          Every terminal runs tmux and fish with the author's config, and nvim is
          the editor (their Neovim, without language servers). tmux's prefix is''
      else
        "Every terminal runs tmux and fish with the author's config. tmux's prefix is"
    }
    Ctrl+A (or Ctrl+B), Ctrl+1..9 picks a window, Ctrl+Tab the next.
    None of it uses Alt, so newm-rs keeps every Alt key.

      prefix |  /  prefix -   split side by side / stacked
      prefix h j k l          move between panes

    EOF
    exec "''${SHELL:-sh}"
  '';

  # foot without text shaping (ligatures, combining emoji): shaping is HarfBuzz, which brings
  # GLib, together 6 MB of the first visit. Built without PGO, whose profiling run would be emulated.
  foot' = foot.override {
    fcft = fcft.override { withShapingTypes = [ ]; };
    allowPgo = false;
  };

  footIni = writeText "newm-rs-demo-foot.ini" (builtins.readFile ./foot.ini);

  # The author's Neovim (nvim/init.lua, nvim/helpers.lua): its colours, status line, tabs, file
  # tree, finder, motions and completion, with tree-sitter for a few languages, and no language
  # servers, formatters or linters, which would add hundreds of MB.
  nvim = neovim.override {
    configure = {
      customLuaRC = builtins.readFile ./nvim/init.lua + builtins.readFile ./nvim/helpers.lua;
      packages.demo.start = with vimPlugins; [
        tokyonight-nvim
        lualine-nvim
        bufferline-nvim
        nvim-tree-lua
        telescope-nvim
        plenary-nvim
        which-key-nvim
        hop-nvim
        nvim-notify
        nvim-cmp
        cmp-buffer
        cmp-path
        zen-mode-nvim
        (nvim-treesitter.withPlugins (
          p: with p; [
            bash
            fish
            json
            lua
            markdown
            nix
            python
            rust
            toml
            yaml
          ]
        ))
      ];
    };
  };

  # fish with the author's prompt (bobthefish), greeting, window titles and command times
  # (fish/), the shell of every tmux pane.
  fishConfig = writeText "newm-rs-demo-config.fish" (builtins.readFile ./fish/config.fish);
  fishFunctions = writeTextDir "functions/fish_title.fish" (
    builtins.readFile ./fish/functions/fish_title.fish
  );
  # fish alone: its program, which carries its functions and completions inside it, without the
  # man pages and the copies under other names (fish_indent, fish_key_reader) that make nixpkgs'
  # fish a 12 MB download, and without the paths it names for its documentation, `man` and the
  # completions generator (Python). The browser fetches a store path whole, so this is 3 MB.
  fish' = runCommand "fish-${fish.version}-demo" { nativeBuildInputs = [ removeReferencesTo ]; } ''
    install -Dm755 ${fish}/bin/fish $out/bin/fish
    remove-references-to -t ${fish} -t ${fish.doc} -t ${python3} -t ${man-db} $out/bin/fish
  '';

  # fish's first interactive start in a home without a cache generates completions from the man
  # pages, in Python: in the guest, every start is a first one, and Python is 60 MB the browser
  # would fetch for completions nobody waits for. The empty function stops it.
  shell = writeShellScript "newm-rs-demo-fish" ''
    exec ${fish'}/bin/fish --init-command \
      "function fish_update_completions; end; set -p fish_function_path ${fishPlugins.bobthefish}/share/fish/vendor_functions.d ${fishFunctions}/functions; source ${fishConfig}" \
      "$@"
  '';

  # What every terminal's tmux finds on PATH.
  sessionPath = lib.makeBinPath (
    [
      tmux
      fish'
      runtimeShellPackage
      coreutils
    ]
    ++ lib.optional full nvim
  );

  # A terminal of its own: a new tmux session, whose panes run fish.
  tmuxNew = writeShellScript "newm-rs-demo-tmux-new" ''
    export PATH=${sessionPath}:$PATH
    export TERMINFO_DIRS=${foot'.terminfo}/share/terminfo:${ncurses}/share/terminfo''${TERMINFO_DIRS:+:$TERMINFO_DIRS}
    # The session's first terminals start together; the one that makes the tmux server goes
    # first, or the others' servers race it and their sessions die with it.
    for _ in $(seq 50); do
      tmux -f ${tmuxConf} has-session -t demo 2>/dev/null && break
      sleep 0.2
    done
    exec tmux -f ${tmuxConf} new-session
  '';

  # foot with the demo's settings, which every terminal of the session is started through; with no
  # command it opens a tmux session, as the author's terminals do.
  term = writeShellScript "newm-rs-demo-foot" ''
    export SHELL=${tmuxNew}
    exec ${lib.getExe foot'} -c ${footIni} "$@"
  '';

  # tmux.conf with the plugins nixpkgs packages for it, loaded as programs.tmux loads them.
  tmuxConf = writeText "newm-rs-demo-tmux.conf" (
    builtins.readFile ./tmux.conf
    + lib.concatMapStrings (plugin: "run-shell ${plugin.rtp}\n") [
      tmuxPlugins.nord
      tmuxPlugins.logging
    ]
    + ''
      set -g default-shell ${shell}
    ''
    + lib.optionalString full ''
      set-environment -g EDITOR nvim
      set-environment -g VISUAL nvim
    ''
  );

  # waybar with the modules the bar uses and niri's IPC; the rest (audio, media players, cava,
  # network, bluetooth, battery, tray, GPS) would bring hundreds of MB of libraries it loads at
  # start, which the browser would fetch on every first visit.
  waybar' = waybar.override {
    cavaSupport = false;
    enableManpages = false;
    evdevSupport = false;
    gpsSupport = false;
    inputSupport = false;
    jackSupport = false;
    mpdSupport = false;
    mprisSupport = false;
    nlSupport = false;
    pipewireSupport = false;
    pulseSupport = false;
    rfkillSupport = false;
    sndioSupport = false;
    systemdSupport = false;
    traySupport = false;
    upowerSupport = false;
    wireplumberSupport = false;
  };

  # waybar with the author's bar, cut down to what the guest has (waybar/config.jsonc); its
  # "+ term" button opens a terminal. newm-rs starts it, so it finds newm-rs's IPC socket
  # (NIRI_SOCKET) for the focused window's title.
  waybarConfig = runCommand "newm-rs-demo-waybar.jsonc" { } ''
    substitute ${./waybar/config.jsonc} $out --replace-fail @term@ ${term}
  '';
  waybarStyle = writeText "newm-rs-demo-waybar.css" (builtins.readFile ./waybar/style.css);
  # waybar is a GtkApplication, which registers on a D-Bus session bus and exits without one; the
  # guest has none, so the bar brings its own. Its log is /tmp/waybar.log.
  bar = writeShellScript "newm-rs-demo-waybar" ''
    exec ${dbus}/bin/dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- \
      ${lib.getExe waybar'} -c ${waybarConfig} -s ${waybarStyle} >/tmp/waybar.log 2>&1
  '';

  # Alacritty, the full session's GPU terminal, on Mod+Shift+Return: it draws with OpenGL ES,
  # which the guest has (Mesa's virgl over the page's WebGL2), and is a 4 MB download the first
  # time it starts. (Ghostty, the author's own terminal, needs desktop OpenGL 4.3, which the
  # browser cannot give it; ghostty.conf says more.)
  alacrittyToml = writeText "newm-rs-demo-alacritty.toml" (builtins.readFile ./alacritty.toml);
  alacrittyTerm = writeShellScript "newm-rs-demo-alacritty" ''
    export TERMINFO_DIRS=${alacritty.terminfo}/share/terminfo
    exec ${lib.getExe alacritty} --config-file ${alacrittyToml} -e ${tmuxNew}
  '';

  # The first terminal: a tmux session whose first window lists the keys, and a second window
  # split in two.
  tmuxSession = writeShellScript "newm-rs-demo-tmux" ''
    export PATH=${sessionPath}:$PATH
    export TERMINFO_DIRS=${foot'.terminfo}/share/terminfo:${ncurses}/share/terminfo
    t() { tmux -f ${tmuxConf} "$@"; }
    if ! t has-session -t demo 2>/dev/null; then
      t new-session -d -s demo -n keys ${keys}
      t new-window -t demo -n shell
      t split-window -h -t demo:shell
      t select-window -t demo:keys
    fi
    exec tmux -f ${tmuxConf} attach -t demo
  '';
in
runCommand "newm-rs-demo"
  {
    passthru = {
      newm-rs = newm-rs';
      # The libraries newm-rs links that are built here rather than taken from nixpkgs' cache.
      ownLibs = [ libinput'.out ];
      inherit
        keys
        term
        tmuxSession
        tmuxConf
        footIni
        bar
        alacrittyTerm
        ;
      # The nixpkgs packages the demo takes as they are, which cache.nixos.org serves to the
      # browser; the site serves the rest of the demo's closure itself (site.nix).
      upstream = [
        foot
        foot.terminfo
        tmux
        tmuxPlugins.nord
        tmuxPlugins.logging
        fishPlugins.bobthefish
        neovim-unwrapped
        alacritty
        alacritty.terminfo
        dbus
        waybar
        ncurses
        runtimeShellPackage
        coreutils
      ];
      # A sentence for the page's help about the terminal (index.html).
      helpNote =
        "<kbd>Alt</kbd> <kbd>Shift</kbd> <kbd>Return</kbd> opens Alacritty instead, a terminal that "
        + "draws with the GPU: its OpenGL ES goes to the page's WebGL2. (Ghostty, the author's own, "
        + "needs desktop OpenGL 4.3, which a browser cannot give it yet.)";
    };
    meta.mainProgram = "newm-rs-demo";
  }
  (
    ''
      mkdir -p $out/bin $out/share/newm-rs-demo
    ''
    + (
      if full then
        ''
          substitute ${./config.kdl} $out/share/newm-rs-demo/config.kdl \
            --replace-fail @bar@ ${bar} \
            --replace-fail @gpu-term@ ${alacrittyTerm}
        ''
      else
        # The small session has no bar and no second terminal: their lines go.
        ''
          grep -v -e @bar@ -e @gpu-term@ ${./config.kdl} > $out/share/newm-rs-demo/config.kdl
        ''
    )
    + ''
      substituteInPlace $out/share/newm-rs-demo/config.kdl \
        --replace-fail @term@ ${term} \
        --replace-fail @tmux@ ${tmuxSession} \
        --replace-fail @synth-mode@ ${if full then "animated" else "still"}

      cat > $out/bin/newm-rs-demo <<EOF
      #!${runtimeShell}
      # newm-rs as a window of the page's compositor, its log in /tmp/newm-rs.log. Each step, and
      # how newm-rs ended, goes to /tmp/newm-rs-demo.log, which the page's debug panel shows.
      # RUST_LOG comes from the page: info, or debug with ?debug=1.
      step() { echo "\$(date +%T) \$*" >> /tmp/newm-rs-demo.log; }
      step "newm-rs-demo: starting (WAYLAND_DISPLAY=\''${WAYLAND_DISPLAY:-unset}, RUST_LOG=\''${RUST_LOG:-unset})"
      export XDG_CONFIG_HOME="\''${XDG_CONFIG_HOME:-/tmp/newm-rs-demo}"
      export RUST_LOG="\''${RUST_LOG:-info}"
      mkdir -p "\$XDG_CONFIG_HOME"
      step "newm-rs-demo: exec newm-rs"
      ${lib.getExe newm-rs'} -c $out/share/newm-rs-demo/config.kdl "\$@" >/tmp/newm-rs.log 2>&1
      code=\$?
      step "newm-rs-demo: newm-rs exited with code \$code"
      exit \$code
      EOF
      chmod +x $out/bin/newm-rs-demo
      ln -s ${lib.getExe newm-rs'} $out/bin/newm-rs
    ''
  )
