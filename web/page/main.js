// The demo page: boot the guest, start newm-rs in it, and show its window over the whole page.
//
// The boot is trynix's (https://github.com/fzakaria/trynix, MIT), driven without its page: the
// same modules walk the demo's closure through narinfos, check its signatures, serve its store
// paths to the guest lazily as it opens them, and start the unkernel engine, whose in-page
// Wayland compositor draws newm-rs's window. This file only strings them together, as trynix's
// app.js does for a link that boots one store path, and keeps the reader posted in one line.
//
// The build (site.nix) puts this file beside trynix's modules and replaces __DEMO_SMALL__ and
// __DEMO_FULL__ with the store paths of the demo's two sessions.

import { walkClosure } from "./closure.js";
import { fetchNar } from "./store.js";
import { lazyStore, PathOrigin, Priority, References } from "./lazy.js";
import {
  fetchListing,
  forgetListing,
  listingFetcher,
  programsOfListing,
} from "./listing.js";
import { startUnkernel } from "./unkernel.js";
import { warmHttpCache } from "./net.js";
import { readSubstituters, verify } from "./substituters.js";
import {
  DIGEST_LENGTH,
  FETCH_CONCURRENCY,
  NAR_CONCURRENCY,
  UNKERNEL_GL,
  UNKERNEL_KERNEL,
  UNKERNEL_MAIN,
  UNKERNEL_VK,
  X11_SERVER,
} from "./config.js";
import { assets } from "./assets.js";
import { chooseDriver, drawsWithGl, publishedDrivers } from "./gldriver.js";
import { fallbacksBelow, pickMemory, thisDevice } from "./memory.js";
import { pickCpus, thisDevice as cpuDevice } from "./cpus.js";
import { hasJspi } from "./engine.js";
import { Isolation, isolationOf } from "./isolation.js";
import { Vulkan } from "./vulkan.js";
import { Sound } from "./sound.js";
import { log, onLog, logLines } from "./log.js";

const params = new URLSearchParams(location.search);
// The session: the small one by default (newm-rs and its terminals, a short first download), or
// with ?full=1 the full desktop (waybar, Neovim, Alacritty on demand), which fetches several times
// as much.
const FULL = params.get("full") === "1";
const DEMO_PATH = FULL ? "__DEMO_FULL__" : "__DEMO_SMALL__";
document.body.dataset.variant = FULL ? "full" : "small";
// ?debug=1: the debug panel opens at once and newm-rs logs at debug level.
const DEBUG = params.get("debug") === "1";
// What the page types at the guest's shell once it is up.
const COMMAND = DEBUG ? "RUST_LOG=debug newm-rs-demo &" : "newm-rs-demo &";

// Every fetch the page makes, for the debug panel: the ones still waiting for their response,
// and a log line for each that fails or answers with an error status.
const shortUrl = (url) => (url.length > 110 ? `${url.slice(0, 60)}…${url.slice(-45)}` : url);
const fetchesWaiting = new Map();
let fetchSeq = 0;
{
  const realFetch = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : (input?.url ?? String(input));
    const id = ++fetchSeq;
    fetchesWaiting.set(id, { url, at: performance.now() });
    try {
      const response = await realFetch(input, init);
      if (!response.ok) {
        log(`fetch: HTTP ${response.status} for ${shortUrl(url)}`);
      }
      return response;
    } catch (err) {
      log(`fetch: failed for ${shortUrl(url)}: ${err?.message ?? err}`);
      throw err;
    } finally {
      fetchesWaiting.delete(id);
    }
  };
}

// The store paths whose NAR is being fetched, since when, and how many bytes have come.
const narsWaiting = new Map();
let lastBytesAt = performance.now();

const STORE_PREFIX = "/nix/store/";
const basenameOf = (info) => info.storePath.slice(STORE_PREFIX.length);
const digestOf = (storePath) =>
  storePath.slice(STORE_PREFIX.length, STORE_PREFIX.length + DIGEST_LENGTH);

const $ = (id) => document.getElementById(id);
const stage = $("boot-stage");
const bar = $("boot-bar");
const elapsed = $("boot-elapsed");
const started = performance.now();

function say(text, fraction = null) {
  stage.textContent = text;
  if (fraction !== null) {
    bar.style.width = `${Math.round(Math.min(1, Math.max(0, fraction)) * 100)}%`;
  }
}

const mb = (bytes) => `${(bytes / 1e6).toFixed(1)} MB`;

// The last lines of the boot log, shown when the boot fails.
let recent = [];
// Whether the engine said it has no WebGL2 for the guest. Its first probe can fail where its
// renderer later gets a context (seen on Chrome 158 with ANGLE on Vulkan), so that alone is no
// failure, however long the downloads take: the boot fails only when the guest says newm-rs
// exited (newm-rs-demo logs it), and a window that comes clears everything.
let noGl = false;
onLog((lines) => {
  recent = lines.slice(-12);
  window.newmDemoLog = lines;
  if (!noGl && lines.some((l) => l.includes("the guest gets no GL renderer"))) {
    noGl = true;
  }
});

// newm-rs exited before its window came: why, from what the guest logged.
function newmExited(code) {
  if (noGl) {
    fail(
      "This browser gave the Linux engine no WebGL2",
      "newm-rs draws with OpenGL, which the guest gets from WebGL2 on an OffscreenCanvas, and " +
        "this browser refused one (its page canvas has WebGL2), so newm-rs stopped (exit code " +
        `${code}). In Chrome, chrome://gpu says why. If chrome://flags sets "Choose ANGLE ` +
        'graphics backend" to Vulkan, or turns on Vulkan, set them back to Default and ' +
        'relaunch, or try another browser. "details" shows the diagnostics.',
    );
  } else {
    fail(
      "newm-rs stopped before its window opened",
      `newm-rs exited with code ${code}. "details" shows its log and the diagnostics; "copy ` +
        'diagnostics" copies them.',
    );
  }
}

function fail(title, detail) {
  document.body.dataset.state = "failed";
  $("fail-title").textContent = title;
  $("fail-detail").textContent = detail;
  $("fail-log").textContent = recent.join("\n");
}

// WebGL2 as the engine asks for it (unkernel's context attributes), on an OffscreenCanvas here
// and in a worker: "ok (renderer)" or why not.
const ENGINE_GL = { alpha: false, depth: false, stencil: false, antialias: false, powerPreference: "high-performance" };
const PROBE_GL = `(${(attrs) => {
  try {
    if (typeof OffscreenCanvas !== "function") return "no OffscreenCanvas";
    const gl = new OffscreenCanvas(1, 1).getContext("webgl2", attrs);
    if (!gl) return "REFUSED (getContext gave none)";
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    const renderer = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return `ok (${renderer})`;
  } catch (err) {
    return `REFUSED (${err?.message ?? err})`;
  }
}})`;

function offscreenGl() {
  return (0, eval)(PROBE_GL)(ENGINE_GL);
}

function workerGl() {
  return new Promise((resolve) => {
    let worker;
    try {
      const src = `onmessage = (e) => postMessage(${PROBE_GL}(e.data));`;
      worker = new Worker(URL.createObjectURL(new Blob([src], { type: "text/javascript" })));
    } catch (err) {
      resolve(`no worker (${err?.message ?? err})`);
      return;
    }
    const timer = setTimeout(() => {
      worker.terminate();
      resolve("no answer from the worker in 5 s");
    }, 5000);
    worker.onmessage = (e) => {
      clearTimeout(timer);
      worker.terminate();
      resolve(e.data);
    };
    worker.onerror = (e) => {
      clearTimeout(timer);
      resolve(`worker error (${e.message})`);
    };
    worker.postMessage(ENGINE_GL);
  });
}

// What the browser must have. Each missing one is a message, not a boot that dies half way.
function missing() {
  if (!hasJspi()) {
    return [
      "This browser cannot run the guest",
      "The Linux engine needs WebAssembly JavaScript Promise Integration (JSPI), which Chrome 137, Firefox 153 and Safari 27 have.",
    ];
  }
  const gl = document.createElement("canvas").getContext("webgl2");
  if (gl === null) {
    return [
      "WebGL2 is off",
      "newm-rs draws with OpenGL, which the guest gets through WebGL2. Turn on hardware acceleration in the browser's settings, or try another browser.",
    ];
  }
  if (!globalThis.crossOriginIsolated || typeof SharedArrayBuffer !== "function") {
    return [
      "The page is not cross-origin isolated",
      "The guest needs SharedArrayBuffer, which needs cross-origin isolation. A service worker sets it up on the first load and reloads the page once; if this stays, the browser is blocking service workers (a private window does in some browsers), or the page is not served over HTTPS or from localhost.",
    ];
  }
  return null;
}

// The DOM trynix's modules draw into: a terminal for the guest's shell, the desktop for its
// windows, and the controls they expect, which this page keeps hidden.
function engineElements() {
  return {
    terminalElement: $("terminal"),
    keyBarElement: $("keybar"),
    consoleElement: $("console"),
    desktopElement: $("desktop"),
    soundControl: $("sound-control"),
    windowsButton: $("console-windows"),
  };
}

async function boot() {
  const memory = pickMemory({ linked: null, device: thisDevice() });
  const cpus = pickCpus({
    linked: null,
    device: cpuDevice({ pointer: thisDevice().pointer }),
  });
  log(`session: ${FULL ? "full" : "small"}, ${DEMO_PATH}`);
  log(`memory: ${memory.mib} MiB (${memory.why}); CPUs: ${cpus.count} (${cpus.why})`);

  // The engine's files, whose kernel is the largest single download.
  say("Fetching the Linux engine…", 0.02);
  const urls = await assets([UNKERNEL_MAIN, UNKERNEL_KERNEL, UNKERNEL_GL, UNKERNEL_VK]);
  let engineTotal = 0;
  let engineBytes = 0;
  const engineReady = warmHttpCache(urls.get(UNKERNEL_KERNEL), {
    onTotal: (n) => (engineTotal = n),
    onBytes: (n) => {
      engineBytes += n;
      if (engineTotal > 0) {
        say(`Fetching the Linux engine… ${mb(engineBytes)} of ${mb(engineTotal)}`, 0.02 + 0.18 * (engineBytes / engineTotal));
      }
    },
  });

  // The demo's closure, from narinfos: its own paths from this site, the rest from
  // cache.nixos.org, each signed by a key the page knows.
  const listings = listingFetcher({ fetchOne: fetchListing, concurrency: FETCH_CONCURRENCY });
  const rootDigest = digestOf(DEMO_PATH);
  const closure = await walkClosure(rootDigest, (n, info) => listings.of(info));
  log(`closure: ${closure.size} paths`);
  await engineReady;

  say(`Checking ${closure.size} signatures…`, 0.22);
  const substituters = readSubstituters();
  const verdicts = await Promise.all([...closure.values()].map((i) => verify(i, substituters)));
  const unsigned = [...closure.values()].filter((_, i) => verdicts[i] === false);
  if (unsigned.length > 0) {
    throw new Error(
      `${unsigned.length} store paths are not signed by a known key, starting with ${unsigned[0].storePath}`,
    );
  }
  if (verdicts.includes(null)) {
    log("signatures: this browser cannot check Ed25519");
  }

  say("Starting the guest…", 0.26);
  const vmPromise = startUnkernel({
    ...engineElements(),
    soundPosition: Sound.OFF,
    onSoundSwitch: () => {},
    net: null,
    memory: { mib: memory.mib, fallbacks: fallbacksBelow(memory.mib) },
    cpus: cpus.count,
    vulkan: Vulkan.OFF,
    engine: {
      main: urls.get(UNKERNEL_MAIN),
      kernel: urls.get(UNKERNEL_KERNEL),
      gl: urls.get(UNKERNEL_GL),
      vk: urls.get(UNKERNEL_VK),
    },
  });

  // The store, served to the guest path by path as it opens files (lazy.js).
  let streamed = 0;
  const store = lazyStore({
    concurrency: NAR_CONCURRENCY,
    listing: (info) => listings.of(info),
    dropListing: forgetListing,
    // Each NAR's fetch is logged when it starts and when it ends or fails, so a stall names the
    // path it waits on.
    async fetch(info) {
      const name = basenameOf(info);
      const entry = { at: performance.now(), bytes: 0 };
      narsWaiting.set(name, entry);
      log(`nar: fetching ${name}`);
      try {
        const result = await fetchNar(
          info,
          (bytes) => {
            streamed += bytes;
            entry.bytes += bytes;
            lastBytesAt = performance.now();
          },
          () => {},
        );
        log(`nar: ${name} done, ${mb(entry.bytes)} in ${((performance.now() - entry.at) / 1000).toFixed(1)} s`);
        return result;
      } catch (err) {
        log(`nar: ${name} FAILED after ${mb(entry.bytes)}: ${err?.message ?? err}`);
        throw err;
      } finally {
        narsWaiting.delete(name);
      }
    },
    async write(basename, entries) {
      (await vmPromise).share.write(basename, entries);
    },
    onWritten: () => {},
  });
  const infos = [...closure.values()];
  store.add(infos);

  // The GL driver the site serves for the guest, and the X server, both fetched on first use.
  const basenames = infos.map(basenameOf);
  const glDriver = chooseDriver(basenames, await publishedDrivers()).driver?.storePath ?? null;
  for (const storePath of glDriver === null ? [X11_SERVER] : [X11_SERVER, glDriver]) {
    store.capability(storePath.slice(STORE_PREFIX.length), async (join) => {
      await walkClosure(digestOf(storePath), (n, info) => {
        listings.of(info);
        join([info]);
      }, closure);
      log(`capability: ${storePath} on first use`);
    });
  }

  // The demo's own programs on PATH, and its root fetched with everything it references.
  const root = infos.find((info) => digestOf(info.storePath) === rootDigest);
  const offered = (async () => {
    const listing = await store.list(basenameOf(root));
    if (listing !== null) {
      (await vmPromise).share.offer(basenameOf(root), programsOfListing(listing));
    }
  })();
  await store.touch(basenameOf(root), References.ALL);
  const vm = await vmPromise;
  await offered;

  const stats = () => ({ ...store.stats(), streamedBytes: streamed });
  window.trynix.downloads = stats;

  say("Booting Linux…", 0.32);
  if (drawsWithGl(basenames)) {
    vm.allowGlamor();
  }
  await vm.run([basenameOf(root)], {
    store,
    x11Server: X11_SERVER.slice(STORE_PREFIX.length),
    glDriver: glDriver?.slice(STORE_PREFIX.length) ?? null,
  });
  log(`guest at its prompt; typing: ${COMMAND}`);
  vm.type(`${COMMAND}\n`);
  guestReadyAt = performance.now();

  // newm-rs's files arrive as it opens them; the line counts what has come so far.
  const waiting = setInterval(() => {
    const { writtenPaths, paths } = store.stats(PathOrigin.SELECTION);
    say(
      `Starting newm-rs… ${mb(streamed)} fetched, ${writtenPaths} of ${paths} packages` +
        (noGl ? " (the engine reported no WebGL2; trying anyway)" : ""),
      0.35 + 0.6 * (paths > 0 ? writtenPaths / paths : 0),
    );
  }, 500);
  return waiting;
}

// newm-rs's window: the first the page's compositor has. Once it opens it is made full screen in
// the page, given the keyboard, and kept full screen.
const newmWindow = () => window.trynix?.desktop?.windows?.()[0];

function watchWindow(waiting) {
  let opened = false;
  const timer = setInterval(() => {
    const win = newmWindow();
    if (!win) {
      return;
    }
    if (!win.fullscreen && !win.minimized) {
      win.setFullscreen(true);
    }
    if (!opened) {
      opened = true;
      windowOpened = true;
      clearInterval(waiting);
      say("Running", 1);
      document.body.dataset.state = "running";
      log(`newm-rs window after ${((performance.now() - started) / 1000).toFixed(1)} s`);
      if (!helpOpen()) {
        win.raise();
      }
    }
  }, 300);
  return timer;
}

// The debug panel: what this browser is, what the page waits on, the guest's own logs and the
// page's log, live, with a button that copies it all. It opens with ?debug=1, from "details",
// and by itself when no window has come 45 s after the guest's prompt (or 4 min into a boot that
// never reaches it).
let guestReadyAt = null;
let windowOpened = false;
const debugPanel = $("debug");
const debugOpen = () => !debugPanel.hidden;
let guestText = "(read from the guest once it is at its prompt)";
let envText = "";

async function environment() {
  const lines = [
    `page: ${location.href}`,
    `user agent: ${navigator.userAgent}`,
    `viewport: ${innerWidth}x${innerHeight} at ${devicePixelRatio}x; touch points: ${navigator.maxTouchPoints ?? 0}`,
    `cpus: ${navigator.hardwareConcurrency ?? "?"}; device memory: ${navigator.deviceMemory ?? "?"} GB`,
    `cross-origin isolated: ${globalThis.crossOriginIsolated}; SharedArrayBuffer: ${typeof SharedArrayBuffer === "function"}; JSPI: ${hasJspi()}`,
    `service worker controlling the page: ${Boolean(navigator.serviceWorker?.controller)}`,
  ];
  const gl = document.createElement("canvas").getContext("webgl2");
  if (gl === null) {
    lines.push("WebGL2: none");
  } else {
    const info = gl.getExtension("WEBGL_debug_renderer_info");
    const renderer = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
    const software = /swiftshader|llvmpipe|software|softpipe/i.test(renderer);
    lines.push(`WebGL2: ${renderer}${software ? " (SOFTWARE)" : ""}; max texture ${gl.getParameter(gl.MAX_TEXTURE_SIZE)}`);
  }
  // The engine draws the guest's GL on an OffscreenCanvas, in a worker when it runs in one; a
  // browser can refuse that where it allows a page's canvas (seen: Chrome 158 on X11 with ANGLE
  // on Vulkan). The engine's own attributes.
  lines.push(`WebGL2 on an OffscreenCanvas, page thread: ${offscreenGl()}`);
  lines.push(`WebGL2 on an OffscreenCanvas, in a worker: ${await workerGl()}`);
  try {
    const adapter = navigator.gpu ? await navigator.gpu.requestAdapter() : null;
    lines.push(`WebGPU: ${adapter ? `${adapter.info?.vendor ?? "?"} ${adapter.info?.architecture ?? ""}${adapter.info?.isFallbackAdapter ? " (fallback)" : ""}` : navigator.gpu ? "no adapter" : "none"}`);
  } catch (err) {
    lines.push(`WebGPU: error ${err?.message ?? err}`);
  }
  return lines.join("\n");
}

function inflightText() {
  const now = performance.now();
  const age = (at) => `${((now - at) / 1000).toFixed(0)} s`;
  const lines = [
    `bytes fetched: ${mb(window.trynix?.downloads?.().streamedBytes ?? 0)}; last bytes ${age(lastBytesAt)} ago`,
    `NARs fetching (${narsWaiting.size}):`,
    ...[...narsWaiting].map(([name, e]) => `  ${name}: ${mb(e.bytes)} so far, for ${age(e.at)}`),
    `requests waiting for a response (${fetchesWaiting.size}):`,
    ...[...fetchesWaiting.values()].map((f) => `  ${shortUrl(f.url)}: ${age(f.at)}`),
  ];
  return lines.join("\n");
}

// The guest's side: the demo script's steps, newm-rs's log, the bar's, and the processes.
const GUEST_REPORT = [
  "echo '== /tmp/newm-rs-demo.log'; tail -n 20 /tmp/newm-rs-demo.log 2>&1",
  "echo '== /tmp/newm-rs.log'; tail -n 80 /tmp/newm-rs.log 2>&1",
  "echo '== /tmp/waybar.log'; tail -n 15 /tmp/waybar.log 2>&1",
  "echo '== processes'; ps -o pid,stat,args 2>/dev/null || ps 2>&1",
  "echo '== memory'; head -n 3 /proc/meminfo 2>&1",
].join("; ");
let guestBusy = false;

async function readGuest() {
  if (guestBusy || guestReadyAt === null || typeof window.trynix?.run !== "function") {
    return;
  }
  guestBusy = true;
  try {
    const result = await Promise.race([
      window.trynix.run(GUEST_REPORT),
      new Promise((resolve) => setTimeout(() => resolve({ timedOut: true }), 20000)),
    ]);
    guestText = result?.timedOut
      ? `(the guest did not answer within 20 s at ${new Date().toLocaleTimeString()}: its shell may be stuck on a file still being fetched)\n\n${guestText}`
      : // newm-rs's log is coloured for a terminal: the escapes go.
        String(result?.output ?? JSON.stringify(result)).replace(/\x1b\[[0-9;]*[A-Za-z]/g, "");
  } catch (err) {
    guestText = `(reading the guest failed: ${err?.message ?? err})`;
  } finally {
    guestBusy = false;
  }
}

function renderDebug() {
  $("debug-env").textContent = envText;
  $("debug-inflight").textContent = inflightText();
  $("debug-guest").textContent = guestText;
  const pre = $("debug-log");
  const atBottom = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 8;
  pre.textContent = logLines();
  if (atBottom) {
    pre.scrollTop = pre.scrollHeight;
  }
}

async function showDebug(why = "") {
  if (why) {
    $("debug-why").textContent = why;
  }
  debugPanel.hidden = false;
  envText = envText || (await environment());
  renderDebug();
  readGuest().then(renderDebug);
}

$("debug-close").addEventListener("click", () => (debugPanel.hidden = true));
for (const button of document.querySelectorAll("[data-debug-open]")) {
  button.addEventListener("click", () => showDebug());
}
$("debug-copy").addEventListener("click", async () => {
  const text = [
    "newm-rs browser demo: diagnostics",
    `taken ${new Date().toISOString()}, ${((performance.now() - started) / 1000).toFixed(0)} s after load`,
    $("debug-why").textContent,
    "", "== this browser", envText,
    "", "== downloads in flight", inflightText(),
    "", "== guest", guestText,
    "", "== page log", logLines(),
  ].join("\n");
  try {
    await navigator.clipboard.writeText(text);
    $("debug-copy").textContent = "copied";
  } catch {
    // No clipboard permission (some phones): select the text so it can be copied by hand.
    const pre = $("debug-log");
    pre.textContent = text;
    getSelection().selectAllChildren(pre);
    $("debug-copy").textContent = "selected; copy it";
  }
  setTimeout(() => ($("debug-copy").textContent = "copy diagnostics"), 3000);
});

// Every 5 s: note stalls in the log, open the panel when no window comes, and refresh it.
let stallLogged = false;
setInterval(() => {
  const now = performance.now();
  if (!windowOpened && narsWaiting.size > 0 && now - lastBytesAt > 20000) {
    if (!stallLogged) {
      stallLogged = true;
      log(`stall: no bytes for ${((now - lastBytesAt) / 1000).toFixed(0)} s, waiting on ${[...narsWaiting.keys()].join(", ")}`);
    }
  } else {
    stallLogged = false;
  }
  // Until the window comes, the guest's own log says whether newm-rs is still starting or gave
  // up: read it every 5 s (the panel need not be open).
  if (!windowOpened && guestReadyAt !== null && document.body.dataset.state !== "failed") {
    readGuest().then(() => {
      const exited = /newm-rs exited with code (\d+)/.exec(guestText);
      if (exited && !windowOpened && document.body.dataset.state !== "failed") {
        log(`newm-rs exited with code ${exited[1]} before its window opened`);
        newmExited(exited[1]);
      }
      if (debugOpen()) {
        renderDebug();
      }
    });
  }
  if (!windowOpened && !debugOpen()) {
    if (guestReadyAt !== null && now - guestReadyAt > 45000) {
      log("no newm-rs window 45 s after the guest's prompt; opening the debug panel");
      showDebug("newm-rs has not opened its window. The guest's log below says what it is doing; \"copy diagnostics\" copies everything.");
    } else if (guestReadyAt === null && now - started > 240000 && document.body.dataset.state === "booting") {
      log("the guest has not reached its prompt in 4 min; opening the debug panel");
      showDebug("The guest has not finished booting. \"Downloads in flight\" shows what it waits on; \"copy diagnostics\" copies everything.");
    }
  }
  if (debugOpen()) {
    renderDebug();
    readGuest().then(renderDebug);
  }
}, 5000);

if (DEBUG) {
  showDebug();
}

// The help: open on the first visit, and until the reader closes it once.
const HELP_KEY = "newm-rs-demo.help-closed";
const help = $("help");
const helpOpen = () => !help.hidden;

function remember(closed) {
  try {
    if (closed) {
      localStorage.setItem(HELP_KEY, "1");
    } else {
      localStorage.removeItem(HELP_KEY);
    }
  } catch {
    // No storage (a private window, blocked site data): the help just opens again next time.
  }
}

function closedBefore() {
  try {
    return localStorage.getItem(HELP_KEY) === "1";
  } catch {
    return false;
  }
}

function showHelp() {
  help.hidden = false;
  $("help-open").setAttribute("aria-expanded", "true");
  $("help-close").focus();
}

function hideHelp() {
  help.hidden = true;
  $("help-open").setAttribute("aria-expanded", "false");
  remember(true);
  newmWindow()?.raise();
}

$("help-close").addEventListener("click", hideHelp);
$("help-open").addEventListener("click", () => (helpOpen() ? hideHelp() : showHelp()));
help.addEventListener("click", (event) => {
  if (event.target === help) {
    hideHelp();
  }
});
document.addEventListener(
  "keydown",
  (event) => {
    if (event.key === "Escape" && helpOpen()) {
      event.preventDefault();
      event.stopPropagation();
      hideHelp();
    }
  },
  true,
);
if (closedBefore()) {
  help.hidden = true;
} else {
  showHelp();
}

setInterval(() => {
  if (document.body.dataset.state === "booting") {
    elapsed.textContent = `${Math.round((performance.now() - started) / 1000)} s`;
  }
}, 1000);

const isolation = isolationOf({
  crossOriginIsolated: globalThis.crossOriginIsolated,
  isSecureContext: globalThis.isSecureContext,
  serviceWorker: navigator.serviceWorker,
});
const lacking = missing();
if (lacking !== null && isolation === Isolation.RELOADING && hasJspi()) {
  // coi-serviceworker reloads the page once it controls it, cross-origin isolated.
  say("Setting up cross-origin isolation…", 0.01);
} else if (lacking !== null) {
  fail(...lacking);
} else {
  boot().then(watchWindow, (err) => {
    log(`boot failed: ${err.message ?? err}`);
    fail("The boot failed", String(err.message ?? err));
  });
}
