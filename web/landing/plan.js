(() => {
  document.documentElement.classList.add("js");

  const plan = document.querySelector("[data-plan]");
  if (!plan) return;

  const TILE_W = 72;
  const TILE_H = 46.5;
  const BOARD_W = 1008;
  const BOARD_H = 465;
  const BASE_W = 252;
  const BASE_H = 141.75;
  const STEP = 0.8;
  const LEVEL_MIN = -2;
  const LEVEL_MAX = 4;
  const MARGIN = 6;

  const svg = plan.querySelector(".plan-svg");
  const camRect = plan.querySelector("[data-cam-rect]");
  const camLabel = plan.querySelector("[data-cam-label]");
  const shade = plan.querySelector("[data-cam-shade]");
  const screen = plan.querySelector("[data-screen]");
  const controls = plan.querySelector("[data-controls]");
  const hint = plan.querySelector("[data-hint]");
  const out = (k) => plan.querySelector(`[data-o="${k}"]`);
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");

  const wins = [...plan.querySelectorAll(".win")].map((g) => {
    const r = g.querySelector("rect");
    const box = {
      x: +r.getAttribute("x"),
      y: +r.getAttribute("y"),
      w: +r.getAttribute("width"),
      h: +r.getAttribute("height"),
    };
    return { g, name: g.dataset.win, box, cx: box.x + box.w / 2, cy: box.y + box.h / 2 };
  });

  const state = { level: 0, cx: 192 + BASE_W / 2, cy: 72 + BASE_H / 2, focus: wins[0], moved: 0 };
  let drawn = { cx: state.cx, cy: state.cy, w: BASE_W, h: BASE_H };
  let anim = 0;

  const size = (level) => {
    const s = Math.pow(STEP, level);
    return { w: BASE_W / s, h: BASE_H / s, s };
  };

  const clampCenter = (cx, cy, w, h) => {
    const fx = (v, len, total) => (len >= total ? total / 2 : Math.min(Math.max(v, len / 2), total - len / 2));
    return { cx: fx(cx, w, BOARD_W), cy: fx(cy, h, BOARD_H) };
  };

  const paint = (v) => {
    const x = v.cx - v.w / 2;
    const y = v.cy - v.h / 2;
    camRect.setAttribute("x", x.toFixed(2));
    camRect.setAttribute("y", y.toFixed(2));
    camRect.setAttribute("width", v.w.toFixed(2));
    camRect.setAttribute("height", v.h.toFixed(2));
    camLabel.setAttribute("x", x.toFixed(2));
    camLabel.setAttribute("y", (y > 18 ? y - 6 : y + v.h + 16).toFixed(2));
    shade.setAttribute(
      "d",
      `M0 0H${BOARD_W}V${BOARD_H}H0Z M${x.toFixed(2)} ${y.toFixed(2)}h${v.w.toFixed(2)}v${v.h.toFixed(2)}h${(-v.w).toFixed(2)}Z`
    );
    screen.setAttribute("viewBox", `${x.toFixed(2)} ${y.toFixed(2)} ${v.w.toFixed(2)} ${v.h.toFixed(2)}`);
  };

  const ease = (t) => (t === 1 ? 1 : 1 - Math.pow(2, -10 * t));

  const moveTo = (target) => {
    cancelAnimationFrame(anim);
    if (reduce.matches) {
      drawn = target;
      paint(drawn);
      return;
    }
    const from = { ...drawn };
    let t0 = -1;
    const dur = 420;
    const tick = (now) => {
      if (t0 < 0) t0 = now;
      const t = Math.min(1, Math.max(0, (now - t0) / dur));
      const k = ease(t);
      drawn = {
        cx: from.cx + (target.cx - from.cx) * k,
        cy: from.cy + (target.cy - from.cy) * k,
        w: from.w + (target.w - from.w) * k,
        h: from.h + (target.h - from.h) * k,
      };
      paint(drawn);
      if (t < 1) anim = requestAnimationFrame(tick);
    };
    anim = requestAnimationFrame(tick);
  };

  const render = () => {
    const { w, h, s } = size(state.level);
    const c = clampCenter(state.cx, state.cy, w, h);
    state.cx = c.cx;
    state.cy = c.cy;
    moveTo({ cx: c.cx, cy: c.cy, w, h });
    const scaleText = state.level === 0 ? "1:1" : `${s.toFixed(2)}×`;
    camLabel.textContent = `camera · ${scaleText}`;
    out("level").textContent = state.level > 0 ? `+${state.level}` : String(state.level).replace("-", "−");
    out("scale").textContent = `${s.toFixed(2)}×`;
    out("focus").textContent = state.focus.name;
    const m = Math.round(state.moved * 10) / 10;
    out("moved").textContent = m === 0 ? "0 tiles" : `${m} tile${m === 1 ? "" : "s"}`;
    wins.forEach((win) => {
      const on = win === state.focus;
      win.g.classList.toggle("is-focus", on);
      win.g.setAttribute("aria-pressed", on ? "true" : "false");
    });
    controls.querySelector('[data-act="in"]').disabled = state.level <= LEVEL_MIN;
    controls.querySelector('[data-act="out"]').disabled = state.level >= LEVEL_MAX;
    controls.querySelector('[data-act="reset"]').disabled = state.level === 0;
  };

  const zoom = (level) => {
    state.level = Math.max(LEVEL_MIN, Math.min(LEVEL_MAX, level));
    state.moved = 0;
    render();
  };

  const focusWin = (win) => {
    const { w, h } = size(state.level);
    const b = win.box;
    let { cx, cy } = state;
    const axis = (c, len, lo, hi) => {
      const half = len / 2;
      if (hi - lo + 2 * MARGIN > len) return (lo + hi) / 2;
      if (lo - MARGIN < c - half) return lo - MARGIN + half;
      if (hi + MARGIN > c + half) return hi + MARGIN - half;
      return c;
    };
    const nx = axis(cx, w, b.x, b.x + b.w);
    const ny = axis(cy, h, b.y, b.y + b.h);
    const c = clampCenter(nx, ny, w, h);
    state.moved = Math.abs(c.cx - cx) / TILE_W + Math.abs(c.cy - cy) / TILE_H;
    state.cx = c.cx;
    state.cy = c.cy;
    state.focus = win;
    render();
  };

  const neighbour = (dir) => {
    const f = state.focus;
    const dx = { left: -1, right: 1, up: 0, down: 0 }[dir];
    const dy = { left: 0, right: 0, up: -1, down: 1 }[dir];
    let best = null;
    let bestScore = Infinity;
    for (const win of wins) {
      if (win === f) continue;
      const vx = win.cx - f.cx;
      const vy = win.cy - f.cy;
      const along = vx * dx + vy * dy;
      if (along <= 1) continue;
      const across = Math.abs(dx ? vy : vx);
      const score = along + across * 2;
      if (score < bestScore) {
        bestScore = score;
        best = win;
      }
    }
    return best;
  };

  svg.setAttribute("role", "group");
  svg.removeAttribute("aria-labelledby");
  svg.setAttribute("aria-label", "The tile canvas. Each window is a button that focuses it.");
  wins.forEach((win) => {
    win.g.setAttribute("tabindex", "0");
    win.g.setAttribute("role", "button");
    win.g.setAttribute("aria-label", `Focus ${win.name}`);
    win.g.addEventListener("click", () => focusWin(win));
    win.g.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        focusWin(win);
      }
    });
  });

  controls.addEventListener("click", (e) => {
    const btn = e.target.closest("button[data-act]");
    if (!btn) return;
    const act = btn.dataset.act;
    if (act === "in") zoom(state.level - 1);
    else if (act === "out") zoom(state.level + 1);
    else if (act === "reset") zoom(0);
    else {
      const n = neighbour(act);
      if (n) focusWin(n);
      else state.moved = 0, render();
    }
  });

  controls.hidden = false;
  hint.hidden = false;
  render();
})();
