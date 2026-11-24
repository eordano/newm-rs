# Sourced by run.sh: python with pillow and numpy, the JetBrainsMono Nerd Font, ffmpeg.
PY=$(nix build --no-link --print-out-paths --impure --expr 'with (builtins.getFlake "nixpkgs").legacyPackages.${builtins.currentSystem}; python3.withPackages (ps: [ ps.pillow ps.numpy ])' | tail -1)/bin/python3
FD=$(nix build --no-link --print-out-paths 'nixpkgs#nerd-fonts.jetbrains-mono' | tail -1)/share/fonts/truetype/NerdFonts/JetBrainsMono
FF=$(nix build --no-link --print-out-paths 'nixpkgs#ffmpeg-full^bin' | tail -1)/bin
test -x "$PY" && test -d "$FD" && test -x "$FF/ffmpeg"
