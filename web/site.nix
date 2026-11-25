# The static site: one page (page/) that boots a Linux guest in the tab on the unkernel engine (a
# Linux kernel compiled to wasm that runs aarch64 programs), fetches the demo's closure and shows
# newm-rs's window, drawn by wland, over the whole viewport. The page is at /try/: the small session
# there, the full one at /try/?full=1. The site's root is the landing page (landing/).
#
# The boot is trynix's: page/main.js imports its modules (closure walk, narinfo signatures, lazy NAR
# fetching, the engine and the compositor glue), and the site ships only the modules main.js
# reaches, not trynix's page, styles, benchmarks or examples. The paths only this site has
# (newm-rs, the demo session) are a binary cache under gl/cache, which the page reads from its own
# origin; the rest of the closure comes from cache.nixos.org, signature checked.
{
  pkgs,
  runCommand,
  python3,
  dejavu_fonts,
  trynix,
  demo,
  demo-full,
}:
let
  vendor = import "${trynix}/nix/vendor.nix" { inherit pkgs; };
  unzstd = import "${trynix}/nix/unzstd-wasm.nix" { inherit pkgs; };
  wlandr = import "${trynix}/nix/wlandr.nix" { inherit pkgs; };

  # What each session's first visit fetches for the guest, in MB, as measured with a fresh
  # browser profile (the engine, about 11 MB, comes on top); the help quotes them.
  firstVisitMB = {
    small = "37";
    full = "83";
  };

in
runCommand "newm-rs-web-site"
  {
    nativeBuildInputs = [ python3 ];
    __structuredAttrs = true;
    # The store paths this site serves itself: the demo's closure less what cache.nixos.org has,
    # which is the closure of the nixpkgs packages the demo names (demo.upstream) and what
    # newm-rs links, less the libraries built here for it (demo.ownLibs). Everything else was
    # built here: the demo's scripts and configs, newm-rs, the trimmed fish, foot and libinput,
    # and the editor's plugins, which nixpkgs' cache does not build for aarch64.
    exportReferencesGraph = {
      closure = [
        demo
        demo-full
      ];
      upstream = demo.upstream ++ demo-full.upstream;
      newm = [ demo.newm-rs ];
    };
    helpNote = demo-full.helpNote;
    ownLibs = map toString demo.ownLibs;
  }
  ''
    site=$out/try
    hashOf() { LC_ALL=C sort | xargs sha256sum | sha256sum | cut -c1-12; }

    mkdir -p $site/js $site/gl
    cp ${trynix}/LICENSE $site/LICENSE-trynix
    cp ${trynix}/site/gl/drivers.json $site/gl/drivers.json
    cp -r ${trynix}/site/gl/cache $site/gl/cache
    chmod -R u+w $site/gl

    # main.js and the trynix modules it reaches, statically or by dynamic import.
    cp ${./page/main.js} $site/js/main.js
    substituteInPlace $site/js/main.js \
      --replace-fail __DEMO_SMALL__ ${demo} \
      --replace-fail __DEMO_FULL__ ${demo-full}
    cp ${unzstd}/unzstd-wasm.js $site/js/unzstd-wasm.js
    python3 -I ${./page/modules.py} ${trynix}/site/js $site/js main.js
    # A path the guest opens brings its libraries with it (from their ELF headers), and trynix
    # also fetches, in the background, each of its references up to 2 MB, for the data it may
    # read next. Here that guess costs more than it saves: most of those references are never
    # opened, and the first visit is the download the reader waits for. What is opened is
    # fetched when it is.
    substituteInPlace $site/js/lazy.js --replace-fail \
      "PREFETCH_LIMIT_BYTES = 2 * 1024 * 1024" "PREFETCH_LIMIT_BYTES = 0"

    cp -r ${vendor} $site/vendor
    chmod -R u+w $site/vendor
    mv $site/vendor/coi-serviceworker.js $site/coi-serviceworker.js

    # wland's compositor, its guest relay and the guest's fonts, renamed after their hash as
    # trynix's build does.
    mv $site/vendor/wland $site/gui
    mv $site/gui/LICENSE $site/gui/LICENSE-wland
    cp ${wlandr}/bin/wlandr $site/gui/wlandr
    for font in DejaVuSansMono.ttf DejaVuSans.ttf; do
      cp ${dejavu_fonts}/share/fonts/truetype/$font $site/gui/
    done
    gui=gui.$(ls -d $site/gui/* | hashOf)
    mv $site/gui "$site/$gui"
    substituteInPlace $site/js/config.js --replace-fail '"gui"' "\"$gui\""

    # The demo's own paths, beside the GL driver trynix serves, and named in the manifest the page
    # reads to know which paths to ask its own origin for.
    mapfile -t own < <(python3 - "$NIX_ATTRS_JSON_FILE" ${demo.newm-rs} <<'EOF'
    import json, sys
    attrs = json.load(open(sys.argv[1]))
    paths = lambda key: {entry["path"] for entry in attrs[key]}
    upstream = paths("upstream") | (paths("newm") - {sys.argv[2]} - set(attrs["ownLibs"]))
    print("\n".join(sorted(paths("closure") - upstream)))
    EOF
    )
    echo "serving ''${#own[@]} store paths"
    python3 -I ${./make-cache.py} "$NIX_ATTRS_JSON_FILE" closure $site/gl/cache "''${own[@]}"
    python3 - $site/gl/drivers.json "''${own[@]}" <<'EOF'
    import json, sys
    path, *own = sys.argv[1:]
    manifest = json.load(open(path))
    manifest["paths"] = sorted(set(manifest["paths"]) | set(own))
    json.dump(manifest, open(path, "w"), indent=2)
    EOF

    # Cache-safe names: the engine's chunks by query string, as trynix's build does, its large
    # files through assets.json, and the page's own files by their hash.
    chunk=$(sha256sum $site/vendor/unkernel/_chunks/core.mjs | cut -c1-12)
    substituteInPlace $site/vendor/unkernel/web.mjs \
      --replace-fail '"./_chunks/core.mjs"' "\"./_chunks/core.mjs?v=$chunk\""
    for name in vk-host vk-shader; do
      substituteInPlace $site/vendor/unkernel/_chunks/$name.mjs \
        --replace-fail '"./core.mjs"' "\"./core.mjs?v=$chunk\""
      hash=$(sha256sum $site/vendor/unkernel/_chunks/$name.mjs | cut -c1-12)
      substituteInPlace $site/vendor/unkernel/web.mjs \
        --replace-fail "\"./_chunks/$name.mjs\"" "\"./_chunks/$name.mjs?v=$hash\""
    done
    wasm=$(sha256sum $site/vendor/unkernel/virgl.wasm | cut -c1-12)
    substituteInPlace $site/vendor/unkernel/virgl.mjs \
      --replace-fail '"virgl.wasm"' "\"virgl.wasm?v=$wasm\""
    python3 ${trynix}/tools/asset-versions.py $site vendor/unkernel

    js=js.$(ls -d $site/js/* | hashOf)
    mv $site/js "$site/$js"
    css=page.$(sha256sum ${./page/page.css} | cut -c1-12).css
    cp ${./page/page.css} $site/$css
    substitute ${./page/index.html} $site/index.html \
      --replace-fail @css@ "$css" \
      --replace-fail @main@ "$js/main.js" \
      --replace-fail @gui@ "$gui" \
      --replace-fail @help-note@ "$helpNote" \
      --replace-fail @small-mb@ ${firstVisitMB.small} \
      --replace-fail @full-mb@ ${firstVisitMB.full}

    # The landing page at the site's root, quoting the same sizes.
    cp -r ${./landing}/. $out/
    chmod -R u+w $out
    substituteInPlace $out/index.html \
      --replace-fail @small-mb@ ${firstVisitMB.small} \
      --replace-fail @full-mb@ ${firstVisitMB.full}
    touch $out/.nojekyll
  ''
