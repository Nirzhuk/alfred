/* Alfred landing page. No dependencies, no build step, no network calls except
   one read of the public GitHub releases API for the version label. */
(() => {
  "use strict";

  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const sleep = (ms) => new Promise((r) => setTimeout(r, reduce ? Math.min(ms, 40) : ms));
  const clamp = (v, a = 0, b = 1) => Math.min(b, Math.max(a, v));
  const lerp = (a, b, t) => a + (b - a) * t;
  const easeOut = (t) => 1 - Math.pow(1 - t, 3);
  const span = (p, a, b) => clamp((p - a) / (b - a));
  const icon = (id, size = 16) => `<svg width="${size}" height="${size}" aria-hidden="true"><use href="${id}"/></svg>`;
  const svgEl = (name) => document.createElementNS("http://www.w3.org/2000/svg", name);
  const REPO = "https://github.com/Nirzhuk/alfred";

  /* Single rAF throttled scroll and resize bus. */
  const scrollSubs = [];
  let ticking = false;
  const requestTick = () => {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      ticking = false;
      scrollSubs.forEach((fn) => fn());
    });
  };
  addEventListener("scroll", requestTick, { passive: true });
  addEventListener("resize", requestTick);

  /* Toast */
  const toastEl = $("[data-toast]");
  let toastTimer;
  const toast = (text) => {
    toastEl.textContent = text;
    toastEl.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toastEl.hidden = true), 3000);
  };

  /* Agents and step types, named as they are in the app */
  const AGENTS = {
    claude: { label: "Claude Code", cli: "claude", file: "claude", color: "--agent-claude-code-color", model: "sonnet" },
    codex: { label: "Codex", cli: "codex", file: "codex", color: "--agent-codex-color", model: "gpt-5" },
    cursor: { label: "Cursor", cli: "cursor-agent", file: "cursor", color: "--agent-cursor-color", model: "default model" },
    opencode: { label: "OpenCode", cli: "opencode", file: "opencode", color: "--agent-opencode-color", model: "default model" },
    copilot: { label: "GitHub Copilot", cli: "copilot", file: "github-copilot", color: "--agent-github-copilot-color", model: "default model" },
    gemini: { label: "Gemini", cli: "gemini", file: "gemini", color: "--agent-gemini-color", model: "default model" },
    grok: { label: "Grok", cli: "grok", file: "grok", color: "--agent-grok-color", model: "default model" },
  };

  const mark = (key) =>
    `<svg class="mark" style="color:var(${AGENTS[key].color})" aria-hidden="true"><use href="#a-${AGENTS[key].file}"/></svg>`;

  const TYPES = {
    input: ["input", "#i-note-pencil"],
    identity: ["identity", "#i-fingerprint"],
    memory: ["memory", "#i-brain"],
    agent: ["agent", null],
    custom: ["custom-agent", "#i-terminal-window"],
    app: ["app-action", "#i-plug"],
    template: ["template", "#i-scroll"],
    file: ["file-inject", "#i-file-plus"],
    git: ["git-status", "#i-git-branch"],
    script: ["script", "#i-code"],
    http: ["http", "#i-globe"],
    notify: ["notify", "#i-bell"],
    write: ["write-file", "#i-file"],
    output: ["output", "#i-push-pin"],
    trigger: [null, "#i-clock"],
  };
  const accentOf = (type) => (TYPES[type][0] ? `var(--node-accent-${TYPES[type][0]})` : "var(--accent)");

  /* Dot grid that leans away from the pointer and ripples when a step finishes. */
  class DotField {
    constructor(host) {
      this.host = host;
      this.canvas = document.createElement("canvas");
      this.canvas.className = "graph-dots";
      this.canvas.setAttribute("aria-hidden", "true");
      host.prepend(this.canvas);
      this.ctx = this.canvas.getContext("2d");
      this.pointer = null;
      this.ripples = [];
      this.loop = false;
      this.refresh();
      new ResizeObserver(() => this.resize()).observe(host);
      if (!reduce) {
        host.addEventListener("pointermove", (e) => {
          const r = host.getBoundingClientRect();
          this.pointer = { x: e.clientX - r.left, y: e.clientY - r.top };
          this.start();
        });
        host.addEventListener("pointerleave", () => {
          this.pointer = null;
          this.start();
        });
      }
    }
    refresh() {
      const css = getComputedStyle(document.documentElement);
      this.dot = css.getPropertyValue("--canvas-dot").trim();
      this.accent = css.getPropertyValue("--accent").trim();
      this.draw(performance.now());
    }
    resize() {
      const dpr = Math.min(devicePixelRatio || 1, 2);
      this.w = this.host.clientWidth;
      this.h = this.host.clientHeight;
      this.canvas.width = this.w * dpr;
      this.canvas.height = this.h * dpr;
      this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      this.draw(performance.now());
    }
    ripple(x, y) {
      if (reduce) return;
      this.ripples.push({ x, y, t: performance.now() });
      this.start();
    }
    start() {
      if (this.loop) return;
      this.loop = true;
      requestAnimationFrame((t) => this.frame(t));
    }
    frame(t) {
      this.draw(t);
      if (this.pointer || this.ripples.length) requestAnimationFrame((n) => this.frame(n));
      else {
        this.loop = false;
        this.draw(t);
      }
    }
    draw(now) {
      const { ctx, w, h } = this;
      if (!w || !h) return;
      ctx.clearRect(0, 0, w, h);
      this.ripples = this.ripples.filter((r) => now - r.t < 1300);
      const gap = 20;
      const hot = [];
      ctx.fillStyle = this.dot;
      ctx.beginPath();
      for (let y = gap / 2; y < h; y += gap) {
        for (let x = gap / 2; x < w; x += gap) {
          let k = 0;
          let dx = 0;
          let dy = 0;
          if (this.pointer) {
            const px = x - this.pointer.x;
            const py = y - this.pointer.y;
            const d = Math.hypot(px, py);
            if (d < 120) {
              k = 1 - d / 120;
              dx = (px / (d || 1)) * k * 8;
              dy = (py / (d || 1)) * k * 8;
            }
          }
          for (const r of this.ripples) {
            const age = now - r.t;
            const d = Math.hypot(x - r.x, y - r.y);
            const band = 1 - Math.abs(d - age * 0.34) / 44;
            if (band > 0) k = Math.max(k, band * (1 - age / 1300));
          }
          if (k > 0.02) hot.push([x + dx, y + dy, k]);
          else {
            ctx.moveTo(x + 1, y);
            ctx.arc(x, y, 1, 0, 6.3);
          }
        }
      }
      ctx.fill();
      ctx.fillStyle = this.accent;
      for (const [x, y, k] of hot) {
        ctx.globalAlpha = 0.25 + k * 0.75;
        ctx.beginPath();
        ctx.arc(x, y, 1 + k * 1.5, 0, 6.3);
        ctx.fill();
      }
      ctx.globalAlpha = 1;
    }
  }
  const fields = [];

  /* A small version of the app's canvas: real DOM nodes, blended edges, drag to
     move, pull from a handle to connect, and a run that walks the graph. */
  class Graph {
    constructor(el, opts = {}) {
      this.el = el;
      this.opts = opts;
      this.nodes = new Map();
      this.edges = [];
      this.token = 0;
      this.running = false;
      this.dots = new DotField(el);
      fields.push(this.dots);
      this.world = document.createElement("div");
      this.world.className = "graph-world";
      this.svg = svgEl("svg");
      this.svg.classList.add("graph-edges");
      this.svg.setAttribute("width", "1");
      this.svg.setAttribute("height", "1");
      this.defs = svgEl("defs");
      this.svg.append(this.defs);
      this.world.append(this.svg);
      el.append(this.world);
      this.scale = 1;
      this.ox = 0;
      this.oy = 0;
      this.vertical = this.isVertical();
      el.classList.toggle("is-vertical", this.vertical);
      new ResizeObserver(() => this.onResize()).observe(el);
    }
    isVertical() {
      return this.opts.vertical || this.el.clientWidth < 640;
    }
    onResize() {
      const v = this.isVertical();
      if (v !== this.vertical && this.spec) {
        this.vertical = v;
        this.el.classList.toggle("is-vertical", v);
        this.render();
      } else this.fit();
    }
    /* Replace the whole graph */
    load(spec) {
      this.spec = spec;
      this.render();
    }
    render() {
      this.token++;
      this.running = false;
      this.nodes.forEach((n) => n.el.remove());
      this.edges.forEach((e) => {
        e.path.remove();
        e.grad.remove();
      });
      this.nodes.clear();
      this.edges = [];
      if (!this.spec) return;
      for (const s of this.spec.nodes) {
        if (this.vertical && s.desktopOnly) continue;
        this.mount(s);
      }
      for (const [a, b] of this.spec.edges) this.link(a, b);
      this.fit();
    }
    clear() {
      this.spec = { nodes: [], edges: [] };
      this.render();
    }
    mount(s) {
      const pos = document.createElement("div");
      pos.className = "node-pos";
      const [accent, ic] = [accentOf(s.type), TYPES[s.type][1]];
      const lead = s.agent ? mark(s.agent) : s.app ? `<span class="app-logo"><img src="assets/apps/${s.app}.svg" alt=""></span>` : icon(s.icon || ic);
      pos.innerHTML = `<div class="wf-node" style="--node-accent:${accent}">
          <i class="wf-handle in"></i>
          <div class="wf-node-header"><span class="wf-node-title-icon">${lead}</span><div class="wf-node-title"></div></div>
          <div class="wf-node-content">
            <p class="wf-node-body"></p>
            ${s.model ? '<p class="wf-node-model"></p>' : ""}
            ${s.skill ? '<p class="wf-node-skill"></p>' : ""}
            <p class="wf-node-status">Not run yet</p>
          </div>
          <i class="wf-handle out"></i>
        </div>`;
      $(".wf-node-title", pos).textContent = s.title;
      $(".wf-node-body", pos).textContent = s.body;
      if (s.model) $(".wf-node-model", pos).textContent = s.model;
      if (s.skill) $(".wf-node-skill", pos).textContent = s.skill;
      const n = { spec: s, el: pos, status: $(".wf-node-status", pos) };
      n.x = this.vertical ? (s.vx ?? 20) : s.x;
      n.y = this.vertical ? (s.vy ?? 10) : s.y;
      this.nodes.set(s.id, n);
      this.world.append(pos);
      this.place(n);
      if (this.opts.draggable !== false) this.bindDrag(n);
      return n;
    }
    place(n) {
      n.el.style.transform = `translate(${n.x}px, ${n.y}px)`;
    }
    link(a, b) {
      const from = this.nodes.get(a);
      const to = this.nodes.get(b);
      if (!from || !to) return;
      const id = `edge-${Math.random().toString(36).slice(2, 9)}`;
      const grad = svgEl("linearGradient");
      grad.id = id;
      grad.setAttribute("gradientUnits", "userSpaceOnUse");
      for (const [offset, type] of [[0, from.spec.type], [1, to.spec.type]]) {
        const stop = svgEl("stop");
        stop.setAttribute("offset", offset);
        stop.style.stopColor = accentOf(type);
        grad.append(stop);
      }
      this.defs.append(grad);
      const path = svgEl("path");
      path.setAttribute("stroke", `url(#${id})`);
      this.svg.append(path);
      const edge = { a, b, path, grad };
      this.edges.push(edge);
      this.route(edge);
    }
    anchor(n, side) {
      const w = n.el.offsetWidth;
      const h = n.el.offsetHeight;
      if (this.vertical) return { x: n.x + w / 2, y: side === "out" ? n.y + h : n.y };
      return { x: side === "out" ? n.x + w : n.x, y: n.y + h / 2 };
    }
    curve(p, q) {
      if (this.vertical) {
        const d = Math.max(30, Math.abs(q.y - p.y) * 0.5);
        return `M${p.x} ${p.y}C${p.x} ${p.y + d} ${q.x} ${q.y - d} ${q.x} ${q.y}`;
      }
      const d = Math.max(40, Math.abs(q.x - p.x) * 0.5);
      return `M${p.x} ${p.y}C${p.x + d} ${p.y} ${q.x - d} ${q.y} ${q.x} ${q.y}`;
    }
    route(e) {
      const p = this.anchor(this.nodes.get(e.a), "out");
      const q = this.anchor(this.nodes.get(e.b), "in");
      e.path.setAttribute("d", this.curve(p, q));
      e.grad.setAttribute("x1", p.x);
      e.grad.setAttribute("y1", p.y);
      e.grad.setAttribute("x2", q.x + 0.01);
      e.grad.setAttribute("y2", q.y + 0.01);
    }
    fit() {
      const W = this.el.clientWidth;
      const H = this.el.clientHeight;
      if (!W || !this.nodes.size) return;
      let maxX = 0;
      let maxY = 0;
      this.nodes.forEach((n) => {
        maxX = Math.max(maxX, n.x + n.el.offsetWidth);
        maxY = Math.max(maxY, n.y + n.el.offsetHeight);
      });
      const pad = this.vertical ? 12 : 20;
      const w = maxX + pad;
      const h = maxY + pad;
      this.scale = Math.min(1, W / w, H / h);
      this.ox = (W - w * this.scale) / 2;
      this.oy = (H - h * this.scale) / 2;
      this.world.style.transform = `translate(${this.ox}px, ${this.oy}px) scale(${this.scale})`;
      this.edges.forEach((e) => this.route(e));
    }
    toWorld(e) {
      const r = this.el.getBoundingClientRect();
      return { x: (e.clientX - r.left - this.ox) / this.scale, y: (e.clientY - r.top - this.oy) / this.scale };
    }
    moveTo(n, x, y) {
      const W = this.el.clientWidth / this.scale;
      const H = this.el.clientHeight / this.scale;
      const left = -this.ox / this.scale;
      const top = -this.oy / this.scale;
      n.x = clamp(x, left + 4, left + W - n.el.offsetWidth - 4);
      n.y = clamp(y, top + 4, top + H - n.el.offsetHeight - 4);
      if (this.vertical) {
        n.spec.vx = n.x;
        n.spec.vy = n.y;
      } else {
        n.spec.x = n.x;
        n.spec.y = n.y;
      }
      this.place(n);
      this.edges.filter((e) => e.a === n.spec.id || e.b === n.spec.id).forEach((e) => this.route(e));
    }
    bindDrag(n) {
      const pos = n.el;
      pos.tabIndex = 0;
      pos.setAttribute("role", "group");
      pos.setAttribute("aria-label", `${n.spec.title} step. Use the arrow keys to move it.`);
      pos.addEventListener("keydown", (e) => {
        const step = { ArrowLeft: [-20, 0], ArrowRight: [20, 0], ArrowUp: [0, -20], ArrowDown: [0, 20] }[e.key];
        if (!step) return;
        e.preventDefault();
        this.moveTo(n, n.x + step[0], n.y + step[1]);
      });
      pos.addEventListener("pointerdown", (e) => {
        if (e.button !== 0) return;
        if (e.target.closest(".wf-handle.out")) return this.startConnect(n, e);
        const start = this.toWorld(e);
        const ox = n.x;
        const oy = n.y;
        pos.setPointerCapture(e.pointerId);
        pos.classList.add("is-dragging");
        const move = (m) => {
          const p = this.toWorld(m);
          this.moveTo(n, ox + p.x - start.x, oy + p.y - start.y);
        };
        const up = () => {
          pos.classList.remove("is-dragging");
          pos.removeEventListener("pointermove", move);
          pos.removeEventListener("pointerup", up);
          pos.removeEventListener("pointercancel", up);
        };
        pos.addEventListener("pointermove", move);
        pos.addEventListener("pointerup", up);
        pos.addEventListener("pointercancel", up);
      });
    }
    startConnect(n, e) {
      e.preventDefault();
      const temp = svgEl("path");
      temp.classList.add("is-temp");
      this.svg.append(temp);
      const start = this.anchor(n, "out");
      const move = (m) => temp.setAttribute("d", this.curve(start, this.toWorld(m)));
      const up = (u) => {
        removeEventListener("pointermove", move);
        removeEventListener("pointerup", up);
        temp.remove();
        const hit = document.elementFromPoint(u.clientX, u.clientY)?.closest(".node-pos");
        const target = [...this.nodes.values()].find((t) => t.el === hit);
        if (!target || target === n) return;
        const a = n.spec.id;
        const b = target.spec.id;
        if (this.edges.some((x) => x.a === a && x.b === b)) return;
        if (this.reaches(b, a)) return toast("That connection would make a loop, so it was left out.");
        this.spec.edges.push([a, b]);
        this.link(a, b);
      };
      addEventListener("pointermove", move);
      addEventListener("pointerup", up);
    }
    reaches(from, to) {
      if (from === to) return true;
      return this.edges.filter((e) => e.a === from).some((e) => this.reaches(e.b, to));
    }
    sink() {
      let best = null;
      this.nodes.forEach((n) => {
        if (this.edges.some((e) => e.a === n.spec.id)) return;
        if (!best || (this.vertical ? n.y > best.y : n.x > best.x)) best = n;
      });
      return best;
    }
    /* Add one step, optionally at a point, and wire it to the current last step. */
    add(s, at, connect = true) {
      /* At a point: wire from the nearest step behind it. Otherwise: from the last step. */
      const behind = at ? [...this.nodes.values()].filter((n) => (this.vertical ? n.y < at.y : n.x + n.el.offsetWidth < at.x)) : [];
      const nearest = behind.sort((p, q) => Math.hypot(p.x - at.x, p.y - at.y) - Math.hypot(q.x - at.x, q.y - at.y))[0] ?? null;
      const last = !connect ? null : at ? nearest : this.sink();
      const spec = { ...s };
      const tail = last ?? { x: -220, y: 100 };
      spec.x = at ? at.x : (s.x ?? tail.x + 240);
      spec.y = at ? at.y : (s.y ?? tail.y);
      spec.vx = at ? at.x : (s.vx ?? 60);
      spec.vy = at ? at.y : (s.vy ?? (last ? last.y + 150 : 10));
      this.spec.nodes.push(spec);
      this.mount(spec);
      if (last) {
        this.spec.edges.push([last.spec.id, spec.id]);
        this.link(last.spec.id, spec.id);
      }
      this.fit();
      return spec;
    }
    setStatus(n, state, text) {
      n.el.classList.toggle("is-running", state === "running");
      n.el.classList.toggle("is-done", state === "done");
      n.status.textContent = text;
    }
    pulse(edge, ms) {
      if (reduce) return Promise.resolve();
      const dot = svgEl("circle");
      dot.classList.add("pulse");
      dot.setAttribute("r", "4");
      this.svg.append(dot);
      const len = edge.path.getTotalLength();
      const t0 = performance.now();
      return new Promise((res) => {
        const step = (t) => {
          const k = clamp((t - t0) / ms);
          const pt = edge.path.getPointAtLength(easeOut(k) * len);
          dot.setAttribute("transform", `translate(${pt.x} ${pt.y})`);
          if (k < 1) requestAnimationFrame(step);
          else {
            dot.remove();
            res();
          }
        };
        requestAnimationFrame(step);
      });
    }
    levels() {
      const left = new Set(this.nodes.keys());
      const out = [];
      while (left.size) {
        const ready = [...left].filter((id) => !this.edges.some((e) => e.b === id && left.has(e.a)));
        if (!ready.length) break;
        ready.forEach((id) => left.delete(id));
        out.push(ready.map((id) => this.nodes.get(id)));
      }
      return out;
    }
    async run() {
      const token = ++this.token;
      this.running = true;
      this.opts.onState?.(true);
      this.opts.log?.reset();
      this.nodes.forEach((n) => this.setStatus(n, "idle", "Queued"));
      this.edges.forEach((e) => e.path.classList.remove("is-flowing"));
      for (const level of this.levels()) {
        await Promise.all(
          level.map(async (n) => {
            const incoming = this.edges.filter((e) => e.b === n.spec.id);
            incoming.forEach((e) => e.path.classList.add("is-flowing"));
            await Promise.all(incoming.map((e) => this.pulse(e, 700)));
            incoming.forEach((e) => e.path.classList.remove("is-flowing"));
            if (token !== this.token) return;
            this.setStatus(n, "running", "Running");
            this.opts.log?.line(n.spec.title, "started");
            await sleep(n.spec.ms ?? 650 + Math.random() * 350);
            if (token !== this.token) return;
            this.setStatus(n, "done", n.spec.done ?? "Done");
            this.dots.ripple(
              this.ox + (n.x + n.el.offsetWidth / 2) * this.scale,
              this.oy + (n.y + n.el.offsetHeight / 2) * this.scale,
            );
            this.opts.log?.line(n.spec.title, n.spec.done ?? "done");
            if (n.spec.out) await this.opts.log?.stream(n.spec.out);
          }),
        );
        if (token !== this.token) return;
      }
      this.running = false;
      this.opts.onState?.(false);
      this.opts.onDone?.();
    }
  }

  /* Run activity log */
  function runLog(body, statusEl) {
    const add = (cls) => {
      const p = document.createElement("p");
      if (cls) p.className = cls;
      body.append(p);
      body.scrollTop = body.scrollHeight;
      return p;
    };
    return {
      reset() {
        body.textContent = "";
        statusEl.textContent = "Running";
      },
      line(title, text) {
        const p = add();
        const time = new Date().toLocaleTimeString("en-GB");
        p.append(`${time} `);
        const b = document.createElement("b");
        b.textContent = title;
        p.append(b, ` ${text}`);
        body.scrollTop = body.scrollHeight;
      },
      async stream(text) {
        const p = add("log-out");
        if (reduce) {
          p.textContent = text;
          return;
        }
        for (let i = 1; i <= text.length; i += 2) {
          p.textContent = text.slice(0, i + 1);
          body.scrollTop = body.scrollHeight;
          await new Promise((r) => setTimeout(r, 14));
        }
      },
      done(text) {
        statusEl.textContent = text;
      },
    };
  }

  /* Searchable list shared by the Add step menu and Quick Access */
  function listbox({ list, input, items, onPick, empty }) {
    let active = 0;
    let shown = [];
    const paint = () => $$(".menu-item", list).forEach((b, i) => {
      b.classList.toggle("is-active", i === active);
      if (i === active) b.scrollIntoView({ block: "nearest" });
    });
    const render = () => {
      const q = input.value.trim().toLowerCase();
      shown = items.filter((it) => !q || `${it.label} ${it.desc} ${it.keywords ?? ""}`.toLowerCase().includes(q));
      active = 0;
      list.textContent = "";
      if (!shown.length) {
        const p = document.createElement("p");
        p.className = "menu-empty";
        p.textContent = empty;
        list.append(p);
        return;
      }
      let group = null;
      shown.forEach((it, i) => {
        if (it.group && it.group !== group) {
          group = it.group;
          const g = document.createElement("div");
          g.className = "menu-group";
          g.textContent = group;
          list.append(g);
        }
        const b = document.createElement("button");
        b.type = "button";
        b.className = "menu-item";
        if (it.accent) b.style.setProperty("--node-accent", it.accent);
        b.innerHTML = `<span class="menu-icon">${it.icon}</span><span><b></b><small></small></span>`;
        $("b", b).textContent = it.label;
        $("small", b).textContent = it.desc;
        b.addEventListener("click", () => onPick(it));
        b.addEventListener("pointerenter", () => {
          active = i;
          $$(".menu-item", list).forEach((x, j) => x.classList.toggle("is-active", j === i));
        });
        list.append(b);
      });
      paint();
    };
    input.addEventListener("input", render);
    input.addEventListener("keydown", (e) => {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (!shown.length) return;
        active = (active + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length;
        paint();
      } else if (e.key === "Enter" && shown[active]) {
        e.preventDefault();
        onPick(shown[active]);
      }
    });
    return { render };
  }

  /* ---------- Download button, per platform ---------- */
  const ua = navigator.userAgent;
  const isTouchDevice = /iPhone|iPad|Android/.test(ua) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  const isMac = /Mac/.test(ua) && !isTouchDevice;
  const os = isTouchDevice ? null : isMac ? "macOS" : /Win/.test(ua) ? "Windows" : /Linux/.test(ua) ? "Linux" : null;
  if (os) $$("[data-os-text]").forEach((el) => (el.textContent = `Download free for ${os}`));
  const osNote = $("[data-os-note]");
  osNote.textContent = {
    macOS: "Open source under the GPL. Signed and notarized, for macOS 11 or later.",
    Windows: "Open source under the GPL. The Windows installer is an unsigned beta, so SmartScreen may warn you.",
    Linux: "Open source under the GPL. Ships as an AppImage.",
  }[os] ?? "A desktop app for macOS, Windows and Linux. Open source under the GPL. The Windows installer is an unsigned beta.";
  const quickKeys = isMac ? "⌘⇧Space" : "Ctrl+Shift+Space";
  $("[data-quick-keys]").textContent = quickKeys;

  /* Agent strip */
  $("[data-agent-strip]").innerHTML = Object.keys(AGENTS)
    .map((k) => `<li title="${AGENTS[k].label}">${mark(k)}<span class="caption-5">${AGENTS[k].label}</span></li>`)
    .join("");

  /* ---------- Theme ---------- */
  const root = document.documentElement;
  $(".theme-btn").addEventListener("click", () => {
    const next = root.dataset.theme === "dark" ? "light" : "dark";
    const apply = () => {
      root.dataset.theme = next;
      root.style.colorScheme = next;
      try {
        localStorage.setItem("alfred:theme", next);
      } catch (e) {}
      fields.forEach((f) => f.refresh());
    };
    if (document.startViewTransition && !reduce) document.startViewTransition(apply);
    else apply();
  });

  /* ---------- Navigation ---------- */
  const menuBtn = $(".menu-btn");
  const menu = $("#mobile-menu");
  const setMenu = (open) => {
    menu.hidden = !open;
    menuBtn.setAttribute("aria-expanded", String(open));
    menuBtn.setAttribute("aria-label", open ? "Close menu" : "Open menu");
    document.body.style.overflow = open ? "hidden" : "";
  };
  menuBtn.addEventListener("click", () => setMenu(menu.hidden));
  menu.addEventListener("click", (e) => e.target.closest("a") && setMenu(false));

  const spy = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        const link = $(`.nav-link[href="#${e.target.id}"]`);
        if (e.isIntersecting) link?.setAttribute("aria-current", "true");
        else link?.removeAttribute("aria-current");
      }
    },
    { rootMargin: "-45% 0px -50% 0px" },
  );
  ["how", "speak", "triggers", "faq"].forEach((id) => spy.observe($(`#${id}`)));

  /* The nav bow tie leans with scroll speed, then settles. */
  const tie = $(".brand-tie");
  if (!reduce) {
    let lastY = scrollY;
    let tilt = 0;
    let target = 0;
    let spinning = false;
    const settle = () => {
      tilt += (target - tilt) * 0.14;
      target *= 0.86;
      tie.style.transform = `rotate(${tilt.toFixed(2)}deg)`;
      if (Math.abs(tilt) > 0.05 || Math.abs(target) > 0.05) requestAnimationFrame(settle);
      else {
        spinning = false;
        tie.style.transform = "";
      }
    };
    scrollSubs.push(() => {
      target = clamp((scrollY - lastY) * 0.7, -18, 18);
      lastY = scrollY;
      if (!spinning) {
        spinning = true;
        requestAnimationFrame(settle);
      }
    });
  }

  /* ---------- Scroll reveals, once each ---------- */
  const revealer = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        e.target.classList.add("is-in");
        revealer.unobserve(e.target);
      }
    },
    { threshold: 0.12, rootMargin: "0px 0px -6% 0px" },
  );
  $$(".reveal").forEach((el) => revealer.observe(el));

  const once = (el, fn, threshold = 0.4) => {
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          fn();
        }
      },
      { threshold },
    );
    io.observe(el);
  };

  /* ---------- Hero canvas ---------- */
  const heroRunBtn = $("[data-hero-run]");
  const log = runLog($("[data-log]"), $("[data-log-status]"));
  const hero = new Graph($("#hero-graph"), {
    log,
    onState(running) {
      heroRunBtn.disabled = running;
      $("[data-run-label]", heroRunBtn).textContent = running ? "Running" : "Run";
    },
    onDone() {
      log.done("Finished");
    },
  });
  hero.load({
    nodes: [
      { id: "git", type: "git", title: "Git status", body: "This week on main", x: 20, y: 14, desktopOnly: true, done: "4 commits read" },
      { id: "input", type: "input", title: "Input", body: "Turn these commits into release notes", x: 20, y: 150, vx: 20, vy: 8, done: "Task sent" },
      { id: "mem", type: "memory", title: "Memories", body: "House style, changelog rules", x: 20, y: 306, desktopOnly: true, done: "2 memories added" },
      {
        id: "claude", type: "agent", agent: "claude", title: "Draft the notes", body: "Claude Code · CLI", model: "sonnet", skill: "/changelog",
        x: 300, y: 138, vx: 120, vy: 142, ms: 1400, done: "Draft ready",
        out: "Fixed: the updater now points at the right repository. Added: a Check for Updates item in the tray menu.",
      },
      {
        id: "codex", type: "agent", agent: "codex", title: "Check against the diff", body: "Codex · CLI", model: "gpt-5",
        x: 570, y: 150, vx: 20, vy: 318, ms: 1200, done: "1 fix, 2 checks",
        out: "Checked each line against the diff. Corrected one version number.",
      },
      { id: "out", type: "output", title: "Choose output", body: "Save, pin, final result", x: 830, y: 162, vx: 120, vy: 472, done: "Saved and pinned" },
    ],
    edges: [["git", "claude"], ["input", "claude"], ["mem", "claude"], ["claude", "codex"], ["codex", "out"]],
  });
  document.fonts?.ready.then(() => hero.fit());
  heroRunBtn.addEventListener("click", () => hero.run());
  once($("#hero-graph"), () => setTimeout(() => hero.run(), 700), 0.6);

  /* Add step menu: same groups and labels as the app's picker */
  const ITEMS = [
    ["Essentials", "input", "Input", "Give the agent a task and context"],
    ["Essentials", "identity", "Identity", "Set a role, personality, and instructions"],
    ["Essentials", "memory", "Memories", "Bring knowledge from previous runs"],
    ...Object.keys(AGENTS).map((k) => ["Agents", "agent", AGENTS[k].label, `Run a task with ${AGENTS[k].label}`, k]),
    ["Agents", "custom", "Custom agent", "Use your own agent command"],
    ["Tools", "app", "App action", "Use actions from connected apps"],
    ["Tools", "template", "Template", "Compose text with workflow values"],
    ["Tools", "file", "File inject", "Read a file into the agent's context"],
    ["Tools", "git", "Git status", "Include repository changes and diffs"],
    ["Tools", "script", "Script", "Run a script and use its output"],
    ["Tools", "http", "HTTP", "Call an API or fetch a URL"],
    ["Outputs", "output", "Choose output", "Save, pin, or mark a final result"],
    ["Outputs", "write", "Write file", "Write a result to a file"],
    ["Outputs", "notify", "Notify", "Send yourself a notification"],
  ].map(([group, type, label, desc, agent]) => ({
    group, type, label, desc, agent,
    keywords: agent ? "agent cli" : "",
    accent: accentOf(type),
    icon: agent ? mark(agent) : icon(TYPES[type][1]),
  }));

  const addMenu = $("[data-add-menu]");
  const addSearch = $("[data-add-search]");
  let addAt = null;
  let addReturn = null;
  let addCount = 0;
  const closeAdd = () => {
    if (addMenu.hidden) return;
    addMenu.hidden = true;
    addReturn?.focus({ preventScroll: true });
  };
  const addList = listbox({
    list: $("[data-add-list]"),
    input: addSearch,
    items: ITEMS,
    empty: "No steps match that search.",
    onPick(it) {
      closeAdd();
      if (hero.nodes.size >= 12) return toast("This preview holds 12 steps. The app has no such limit.");
      hero.add(
        {
          id: `added-${++addCount}`,
          type: it.type,
          agent: it.agent,
          title: it.label,
          body: it.agent ? `${it.label} · CLI` : it.desc,
          model: it.agent ? AGENTS[it.agent].model : undefined,
          done: "Done",
        },
        addAt,
      );
    },
  });
  const openAdd = (x, y, at) => {
    addReturn = document.activeElement;
    addAt = at;
    addSearch.value = "";
    addMenu.hidden = false;
    addList.render();
    const r = addMenu.getBoundingClientRect();
    addMenu.style.left = `${clamp(x, 8, innerWidth - r.width - 8)}px`;
    addMenu.style.top = `${clamp(y, 8, innerHeight - r.height - 8)}px`;
    addSearch.focus({ preventScroll: true });
  };
  $("#hero-graph").addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const p = hero.toWorld(e);
    openAdd(e.clientX, e.clientY, { x: p.x, y: p.y });
  });
  $("[data-add-step]").addEventListener("click", (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    openAdd(r.right - 280, r.bottom + 8, null);
  });
  addEventListener("pointerdown", (e) => {
    if (!addMenu.hidden && !addMenu.contains(e.target) && !e.target.closest("[data-add-step]")) closeAdd();
  });

  /* ---------- Problem scrub ---------- */
  const stage = $("[data-scrub-stage]");
  const scrubSection = $("#problem");
  const WINS = [
    { cli: "claude", agent: "claude", title: "Summarise the diff", lines: ["<b>&gt;</b> summarise this diff", "11 files changed. Main risk", "is the updater URL."] },
    { cli: "codex", agent: "codex", title: "Check the summary", lines: ["<b>&gt;</b> [pasted 41 lines]", "What would you like me", "to check in this text?"] },
    { cli: "cursor-agent", agent: "cursor", title: "Apply the fix", lines: ["<b>&gt;</b> wait, which branch?", "Please share the earlier", "output and the branch."] },
    { cli: "gemini", agent: "gemini", title: "Write the notes", lines: ["<b>&gt;</b> [pasted again]", "I have no context yet.", "Paste the previous result."] },
  ];
  const winEls = WINS.map((w) => {
    const el = document.createElement("div");
    el.className = "scrub-win";
    el.innerHTML = `<div class="scrub-face term">
        <div class="term-bar"><i></i><i></i><i></i><span>${w.cli}</span></div>
        <div class="term-text">${w.lines.join("<br>")}</div>
      </div>
      <div class="scrub-face wf-node" style="--node-accent:var(--node-accent-agent)">
        <div class="wf-node-header"><span class="wf-node-title-icon">${mark(w.agent)}</span><div class="wf-node-title">${w.title}</div></div>
        <div class="wf-node-content">
          <p class="wf-node-body">${AGENTS[w.agent].label} · CLI</p>
          <p class="wf-node-status">Receives the last result</p>
        </div>
      </div>`;
    stage.append(el);
    return { el, term: el.children[0], node: el.children[1] };
  });
  const edgeSvg = $("[data-scrub-edges]");
  const scrubEdges = [0, 1, 2].map(() => {
    const p = svgEl("path");
    p.setAttribute("pathLength", "1");
    p.setAttribute("stroke-dasharray", "1");
    p.setAttribute("stroke-dashoffset", "1");
    edgeSvg.append(p);
    return p;
  });
  const chip = $("[data-paste-chip]");
  const textA = $("[data-scrub-a]");
  const textB = $("[data-scrub-b]");
  let scrubLayout = null;

  const layoutScrub = () => {
    const cs = getComputedStyle(stage);
    const pad = parseFloat(cs.paddingLeft);
    const W = stage.clientWidth - pad * 2;
    const H = stage.clientHeight;
    const narrow = W < 900;
    const ww = narrow ? Math.min(W * 0.66, 230) : Math.min(240, (W - 3 * 24) / 4);
    const wh = 104;
    const CHAOS = [[0.04, 0.06, -5], [0.42, 0.0, 3], [0.14, 0.72, 4], [0.68, 0.52, -4]];
    const chaos = CHAOS.map(([fx, fy, r]) => ({ x: pad + fx * (W - ww), y: fy * (H - wh), r }));
    const row = narrow ? 0 : (W - 4 * ww) / 3;
    const final = WINS.map((_, i) =>
      narrow
        ? { x: pad + (i % 2 ? W - ww : 0), y: (i * (H - wh - 24)) / 3 + 8 }
        : { x: pad + i * (ww + row), y: (H - wh) / 2 },
    );
    winEls.forEach(({ el }) => {
      el.style.width = `${ww}px`;
      el.style.height = `${wh}px`;
    });
    scrubEdges.forEach((p, i) => {
      const a = final[i];
      const b = final[i + 1];
      if (narrow) {
        const y1 = a.y + wh;
        const y2 = b.y;
        const x1 = a.x + ww / 2;
        const x2 = b.x + ww / 2;
        p.setAttribute("d", `M${x1} ${y1}C${x1} ${y1 + 30} ${x2} ${y2 - 30} ${x2} ${y2}`);
      } else {
        const x1 = a.x + ww;
        const y = a.y + wh / 2;
        p.setAttribute("d", `M${x1} ${y}L${b.x} ${y}`);
      }
    });
    scrubLayout = { chaos, final, ww, wh };
  };

  const paintScrub = () => {
    const rect = scrubSection.getBoundingClientRect();
    if (!reduce && (rect.bottom < -100 || rect.top > innerHeight + 100)) return;
    if (!scrubLayout) layoutScrub();
    const p = reduce ? 1 : clamp(-rect.top / (rect.height - innerHeight));
    const { chaos, final } = scrubLayout;
    winEls.forEach((w, i) => {
      const t = easeOut(span(p, 0.2 + i * 0.05, 0.62 + i * 0.05));
      const x = lerp(chaos[i].x, final[i].x, t);
      const y = lerp(chaos[i].y, final[i].y, t);
      w.el.style.transform = `translate(${x}px, ${y}px) rotate(${lerp(chaos[i].r, 0, t)}deg)`;
      const face = span(t, 0.45, 0.85);
      w.term.style.opacity = 1 - face;
      w.node.style.opacity = face;
    });
    scrubEdges.forEach((e, i) => e.setAttribute("stroke-dashoffset", 1 - span(p, 0.68 + i * 0.07, 0.8 + i * 0.07)));
    if (reduce) {
      chip.hidden = true;
      return;
    }
    const a = span(p, 0.26, 0.38);
    const b = span(p, 0.4, 0.52);
    textA.style.opacity = 1 - a;
    textA.style.transform = `translateY(${-a * 16}px)`;
    textA.style.visibility = a >= 1 ? "hidden" : "visible";
    textB.style.opacity = b;
    textB.style.transform = `translateY(${(1 - b) * 16}px)`;
    /* The paste chip hops from window to window, as you do today. */
    const hops = clamp(p / 0.24) * 3;
    const i = Math.min(2, Math.floor(hops));
    const k = easeOut(hops - i);
    const { ww } = scrubLayout;
    const cx = lerp(chaos[i].x, chaos[i + 1].x, k) + ww / 2 - 20;
    const cy = lerp(chaos[i].y, chaos[i + 1].y, k) - Math.sin(k * Math.PI) * 48 - 12;
    chip.style.transform = `translate(${cx}px, ${cy}px)`;
    chip.style.opacity = 1 - span(p, 0.2, 0.28);
  };
  scrollSubs.push(paintScrub);
  addEventListener("resize", () => {
    layoutScrub();
    requestTick();
  });

  /* ---------- Tagline, one word at a time ---------- */
  const tagline = $("[data-tagline]");
  const words = [];
  $$(":scope > span", tagline).forEach((line) => {
    const parts = line.textContent.trim().split(/\s+/);
    line.textContent = "";
    parts.forEach((word, i) => {
      const w = document.createElement("span");
      w.className = "word";
      w.textContent = word;
      words.push(w);
      line.append(w);
      if (i < parts.length - 1) line.append(" ");
    });
  });
  scrollSubs.push(() => {
    if (reduce) return;
    const r = tagline.getBoundingClientRect();
    if (r.bottom < -200 || r.top > innerHeight + 200) return;
    const progress = clamp((innerHeight * 0.82 - r.top) / (r.height * 0.9 + innerHeight * 0.32));
    const lit = Math.round(progress * words.length);
    words.forEach((w, i) => w.classList.toggle("is-lit", i < lit));
  });

  /* ---------- Benefit: swap the agent ---------- */
  const rail = $("[data-agent-rail]");
  let agentTouched = false;
  let agentKey = "claude";
  const setAgent = (key) => {
    agentKey = key;
    const a = AGENTS[key];
    $$(".agent-pick", rail).forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.agent === key)));
    const holder = $("[data-agent-mark]");
    holder.innerHTML = mark(key);
    holder.firstElementChild.classList.add("is-new");
    $("[data-agent-body]").textContent = `${a.label} · CLI`;
    $("[data-agent-model]").textContent = a.model;
    $("[data-agent-cli]").textContent = a.cli;
  };
  Object.keys(AGENTS).forEach((k) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "agent-pick";
    b.dataset.agent = k;
    b.title = AGENTS[k].label;
    b.setAttribute("aria-label", AGENTS[k].label);
    b.innerHTML = mark(k);
    b.addEventListener("click", () => {
      agentTouched = true;
      setAgent(k);
    });
    rail.append(b);
  });
  setAgent("claude");
  if (!reduce) {
    let visible = false;
    new IntersectionObserver((e) => (visible = e[0].isIntersecting), { threshold: 0.4 }).observe($("#agents"));
    setInterval(() => {
      if (!visible || agentTouched || document.hidden) return;
      const keys = Object.keys(AGENTS);
      setAgent(keys[(keys.indexOf(agentKey) + 1) % keys.length]);
    }, 2400);
  }

  /* ---------- Benefit: pin memories ---------- */
  const MEMORIES = [
    "Preference: British spelling",
    "Constraint: never touch migrations",
    "Decision: Postgres over Mongo",
    "Lesson: run lint before commit",
    "Note: we release on Thursdays",
  ];
  const memTarget = $("[data-memory-target]");
  const memCount = $("[data-memory-count]");
  const memHolder = $("[data-memory-chips]");
  const updateMem = () => {
    const n = $$('[aria-pressed="true"]', memHolder).length;
    memCount.textContent = n === 0 ? "Nothing pinned yet" : `${n} ${n === 1 ? "memory" : "memories"} pinned to this agent`;
  };
  MEMORIES.forEach((text, i) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "toggle-pill caption-1";
    b.textContent = text;
    b.setAttribute("aria-pressed", String(i === 0));
    b.addEventListener("click", () => {
      const on = b.getAttribute("aria-pressed") !== "true";
      b.setAttribute("aria-pressed", String(on));
      updateMem();
      memTarget.classList.remove("is-bump");
      void memTarget.offsetWidth;
      memTarget.classList.add("is-bump");
      if (!on || reduce) return;
      const from = b.getBoundingClientRect();
      const to = memTarget.getBoundingClientRect();
      const fly = b.cloneNode(true);
      fly.classList.add("fly-chip");
      fly.removeAttribute("aria-pressed");
      fly.setAttribute("aria-hidden", "true");
      fly.tabIndex = -1;
      fly.style.width = `${from.width}px`;
      document.body.append(fly);
      fly.animate(
        [
          { transform: `translate(${from.left}px, ${from.top}px)`, opacity: 1 },
          { transform: `translate(${to.left + 16}px, ${to.top + 8}px) scale(0.6)`, opacity: 0 },
        ],
        { duration: 600, easing: "cubic-bezier(0.19, 1, 0.22, 1)" },
      ).onfinish = () => fly.remove();
    });
    memHolder.append(b);
  });
  updateMem();

  /* ---------- Benefit: choose output ---------- */
  const tagsEl = $("[data-receipt-tags]");
  const noteEl = $("[data-receipt-note]");
  const failBtn = $("[data-fail-save]");
  const outBtns = $$("[data-out]");
  const NOTE = "Output from Codex, step 3 of 4.";
  const setTags = (tags) => {
    tagsEl.textContent = "";
    tags.forEach(([text, cls]) => {
      const t = document.createElement("span");
      t.className = `tag ${cls ?? ""}`;
      t.textContent = text;
      tagsEl.append(t);
    });
  };
  const paintTags = () => {
    const on = outBtns.filter((b) => b.getAttribute("aria-pressed") === "true").map((b) => [b.dataset.out]);
    setTags(on);
    noteEl.textContent = on.length ? NOTE : "Nothing from this output will be kept.";
  };
  outBtns.forEach((b) =>
    b.addEventListener("click", () => {
      b.setAttribute("aria-pressed", String(b.getAttribute("aria-pressed") !== "true"));
      paintTags();
      failBtn.textContent = "Simulate a failed save";
      failBtn.dataset.state = "";
    }),
  );
  paintTags();
  failBtn.addEventListener("click", async () => {
    if (failBtn.dataset.state === "failed") {
      failBtn.disabled = true;
      setTags([["Saving", "is-busy"]]);
      await sleep(900);
      failBtn.disabled = false;
      failBtn.dataset.state = "";
      failBtn.textContent = "Simulate a failed save";
      outBtns[0].setAttribute("aria-pressed", "true");
      paintTags();
      noteEl.textContent = "Saved. The agent did not run again.";
      return;
    }
    failBtn.dataset.state = "failed";
    failBtn.textContent = "Retry save";
    setTags([["Failed", "is-fail"]]);
    noteEl.textContent = "Save failed. The disk was full. The output is safe, so retry when you are ready.";
  });

  /* ---------- Benefit: local trace ---------- */
  const localLab = $("[data-local]");
  const trace = () => {
    localLab.classList.remove("is-tracing");
    void localLab.offsetWidth;
    localLab.classList.add("is-tracing");
  };
  $("[data-trace]").addEventListener("click", trace);
  once(localLab, trace, 0.6);

  /* ---------- Speak with Alfred ---------- */
  const speakRun = $("[data-speak-run]");
  const reply = $("[data-speak-reply]");
  const orb = $("[data-speak-orb]");
  const speak = new Graph($("#speak-graph"), {
    onState(running) {
      speakRun.disabled = running;
      $("[data-run-label]", speakRun).textContent = running ? "Running" : "Run";
    },
    onDone() {
      reply.textContent = "Finished. Every step reported back, in order.";
    },
  });
  speak.spec = { nodes: [], edges: [] };

  const APPS = [
    [/\bgmail\b/, "Gmail", "gmail"],
    [/\bslack\b/, "Slack", "slack"],
    [/\blinear\b/, "Linear", "linear"],
    [/\bgithub\b/, "GitHub", "github"],
    [/\bnotion\b/, "Notion", "notion"],
    [/\btelegram\b/, "Telegram", "telegram"],
    [/\bwhatsapp\b/, "WhatsApp", "whatsapp"],
    [/\bsentry\b/, "Sentry", "sentry"],
    [/\bobsidian\b/, "Obsidian", "obsidian"],
    [/\b(google )?drive\b/, "Google Drive", "google-drive"],
    [/\bsharepoint\b/, "SharePoint", "sharepoint"],
    [/\b(outlook|microsoft|teams)\b/, "Microsoft", "microsoft"],
  ];
  const AGENT_WORDS = { claude: /\bclaude\b/, codex: /\bcodex\b/, cursor: /\bcursor\b/, opencode: /\bopencode\b/, copilot: /\bcopilot\b/, gemini: /\bgemini\b/, grok: /\bgrok\b/ };

  /* ponytail: keyword matching, not a model. The real tab uses the visitor's own CLI. */
  function planWorkflow(raw) {
    const t = raw.toLowerCase();
    const steps = [];
    if (/\bwebhook\b/.test(t)) steps.push({ type: "trigger", icon: "#i-link", title: "Webhook", body: "POST to the local listener", done: "Request received" });
    else if (/\b(file|folder)\b/.test(t) && /\b(when|changes?|saved|appears?)\b/.test(t)) steps.push({ type: "trigger", icon: "#i-folder", title: "File change", body: "Watches a path", done: "File changed" });
    else if (/\b(every|each|daily|weekly|nightly|morning|evening|monday|tuesday|wednesday|thursday|friday|cron|schedule)\b/.test(t)) steps.push({ type: "trigger", title: "Schedule", body: "Cron, while Alfred is open", done: "Time reached" });

    const found = APPS.map(([re, label, app]) => {
      const m = t.match(re);
      if (!m) return null;
      const before = t.slice(Math.max(0, m.index - 24), m.index);
      const sink = /\b(to|into|send|post|open|create|message|tell|update|on)\s+(a\s+|an\s+|my\s+|the\s+|me\s+on\s+)?$/.test(before) && !/\bwhen\b/.test(before);
      return { label, app, sink, at: m.index };
    }).filter(Boolean).sort((a, b) => a.at - b.at);

    if (/\b(commit|commits|diff|branch|repo|repository)\b/.test(t)) steps.push({ type: "git", title: "Git status", body: "Repository changes and diffs", done: "Diff read" });
    found.filter((f) => !f.sink).forEach((f) => steps.push({ type: "app", app: f.app, title: `Read ${f.label}`, body: "App action", done: "Read" }));
    if (/\b(remember|memory|memories)\b/.test(t)) steps.push({ type: "memory", title: "Memories", body: "Saved context", done: "Context added" });

    const verb = /summar/.test(t) ? "Summarise" : /review|check/.test(t) ? "Review" : /fix/.test(t) ? "Propose a fix" : /triage/.test(t) ? "Triage" : /draft|write/.test(t) ? "Draft" : "Do the work";
    const agents = Object.keys(AGENT_WORDS).map((k) => ({ k, m: t.match(AGENT_WORDS[k]) })).filter((x) => x.m).sort((a, b) => a.m.index - b.m.index).map((x) => x.k);
    (agents.length ? agents : ["claude"]).forEach((k) =>
      steps.push({ type: "agent", agent: k, title: verb, body: `${AGENTS[k].label} · CLI`, model: AGENTS[k].model, done: "Finished", ms: 1100 }),
    );
    if (/\b(script)\b/.test(t)) steps.push({ type: "script", title: "Script", body: "Run a script", done: "Exit 0" });
    if (/\b(api|http|url|fetch)\b/.test(t)) steps.push({ type: "http", title: "HTTP", body: "Call an API", done: "200 OK" });
    found.filter((f) => f.sink).forEach((f) => steps.push({ type: "app", app: f.app, title: `Send to ${f.label}`, body: "App action", done: "Sent" }));
    if (/\b(notify|notification|alert|remind|message me)\b/.test(t) && !found.some((f) => f.sink)) steps.push({ type: "notify", title: "Notify", body: "Desktop notification", done: "Sent" });
    if (/\b(save|write to|to a file)\b/.test(t)) steps.push({ type: "write", title: "Write file", body: "Write the result", done: "Written" });
    if (steps.length < 6) steps.push({ type: "output", title: "Choose output", body: "Save, pin, final result", done: "Saved" });
    return { steps: steps.slice(0, 7), matched: found.length > 0 || steps.length > 2 };
  }

  let speakToken = 0;
  async function buildWorkflow(text) {
    const token = ++speakToken;
    const { steps, matched } = planWorkflow(text);
    speak.clear();
    $("[data-speak-empty]").hidden = true;
    speakRun.hidden = true;
    orb.classList.add("is-busy");
    const names = steps.map((s) => s.title);
    const line = matched
      ? `Very good. ${steps.length} steps laid out: ${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}. Press Run when you are ready.`
      : "I could not spot an app or a schedule in that, so I started with one Claude Code step. Press Run when you are ready.";
    reply.textContent = "";
    if (reduce) reply.textContent = line;
    else {
      for (let i = 0; i <= line.length; i += 2) {
        if (token !== speakToken) return;
        reply.textContent = line.slice(0, i + 1);
        await new Promise((r) => setTimeout(r, 12));
      }
      reply.textContent = line;
    }
    for (let i = 0; i < steps.length; i++) {
      if (token !== speakToken) return;
      const s = steps[i];
      speak.add({
        ...s,
        id: `s${i}`,
        x: 16 + i * 230,
        y: i % 2 ? 40 : 110,
        vx: i % 2 ? 120 : 16,
        vy: 8 + i * 134,
      });
      await sleep(240);
    }
    orb.classList.remove("is-busy");
    speakRun.hidden = false;
  }
  speakRun.addEventListener("click", () => speak.run());

  const speakForm = $("[data-speak-form]");
  const speakInput = $("#speak-input");
  const speakError = $("[data-speak-error]");
  speakInput.addEventListener("input", () => {
    speakError.hidden = true;
    speakForm.classList.remove("is-invalid");
  });
  speakForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const text = speakInput.value.trim();
    if (!text) {
      speakError.textContent = "Describe what you want automated first. For example, name an app and a time.";
      speakError.hidden = false;
      speakForm.classList.add("is-invalid");
      speakInput.setAttribute("aria-invalid", "true");
      speakInput.focus();
      return;
    }
    speakInput.removeAttribute("aria-invalid");
    buildWorkflow(text);
  });
  const SUGGESTIONS = [
    "Every morning, summarise my Gmail and post it to Slack",
    "When a Sentry issue appears, ask Claude Code for a fix and open a GitHub pull request",
    "On every commit, have Codex review the diff and message me on Telegram",
  ];
  const askAlfred = (text) => {
    speakInput.value = text;
    speakError.hidden = true;
    speakForm.classList.remove("is-invalid");
    buildWorkflow(text);
  };
  SUGGESTIONS.forEach((text) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "toggle-pill caption-1";
    b.textContent = text;
    b.addEventListener("click", () => askAlfred(text));
    $("[data-speak-chips]").append(b);
  });

  /* ---------- Trigger lab ---------- */
  const triggerGraph = new Graph($("#trigger-graph"), { vertical: true, draggable: false });
  triggerGraph.load({
    nodes: [
      { id: "t", type: "trigger", title: "Schedule", body: "Cron, while Alfred is open", vx: 20, vy: 8, done: "Fired" },
      { id: "a", type: "agent", agent: "claude", title: "Triage it", body: "Claude Code · CLI", vx: 20, vy: 128, ms: 1000, done: "Finished" },
      { id: "n", type: "notify", title: "Notify", body: "Desktop notification", vx: 20, vy: 248, done: "Sent" },
    ],
    edges: [["t", "a"], ["a", "n"]],
  });
  const setTrigger = (title, body, iconId) => {
    const t = triggerGraph.nodes.get("t");
    $(".wf-node-title", t.el).textContent = title;
    $(".wf-node-body", t.el).textContent = body;
    $(".wf-node-title-icon", t.el).innerHTML = icon(iconId);
    triggerGraph.nodes.forEach((n) => triggerGraph.setStatus(n, "idle", "Not run yet"));
  };
  const TRIGGER_NODES = {
    schedule: ["Schedule", "Cron, while Alfred is open", "#i-clock"],
    file: ["File change", "Watches a path", "#i-folder"],
    webhook: ["Webhook", "POST to the local listener", "#i-link"],
  };

  const tabs = $$('[role="tab"]', $("[data-trigger-tabs]"));
  const selectTab = (tab) => {
    tabs.forEach((t) => {
      const on = t === tab;
      t.setAttribute("aria-selected", String(on));
      t.tabIndex = on ? 0 : -1;
      $(`#${t.getAttribute("aria-controls")}`).hidden = !on;
    });
    setTrigger(...TRIGGER_NODES[tab.id.replace("tab-", "")]);
  };
  tabs.forEach((t, i) => {
    t.addEventListener("click", () => selectTab(t));
    t.addEventListener("keydown", (e) => {
      const d = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
      if (!d) return;
      const next = tabs[(i + d + tabs.length) % tabs.length];
      selectTab(next);
      next.focus();
    });
  });

  /* Dial: drag or use arrow keys to set the time, and read the real cron out. */
  const dial = $("[data-dial]");
  const hand = $("[data-dial-hand]");
  const dialTime = $("[data-dial-time]");
  const cronEl = $("[data-cron]");
  const nextEl = $("[data-next-run]");
  const dayError = $("[data-day-error]");
  const ffBtn = $("[data-fast-forward]");
  const dayHolder = $("[data-days]");
  let minutes = 8 * 60 + 30;
  let handDeg = (minutes / 1440) * 360;
  const DAYS = [["M", 1, "Monday"], ["T", 2, "Tuesday"], ["W", 3, "Wednesday"], ["T", 4, "Thursday"], ["F", 5, "Friday"], ["S", 6, "Saturday"], ["S", 0, "Sunday"]];
  const active = new Set([1, 2, 3, 4, 5]);
  DAYS.forEach(([letter, n, name]) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "toggle-pill caption-1";
    b.textContent = letter;
    b.setAttribute("aria-label", name);
    b.setAttribute("aria-pressed", String(active.has(n)));
    b.addEventListener("click", () => {
      if (active.has(n)) active.delete(n);
      else active.add(n);
      b.setAttribute("aria-pressed", String(active.has(n)));
      paintSchedule();
    });
    dayHolder.append(b);
  });
  const ticks = $("[data-dial-ticks]");
  for (let i = 0; i < 24; i++) {
    const a = (i / 24) * Math.PI * 2;
    const [sin, cos] = [Math.sin(a), -Math.cos(a)];
    const big = i % 6 === 0;
    const l = svgEl("line");
    l.setAttribute("x1", 100 + sin * (big ? 78 : 81));
    l.setAttribute("y1", 100 + cos * (big ? 78 : 81));
    l.setAttribute("x2", 100 + sin * 85);
    l.setAttribute("y2", 100 + cos * 85);
    ticks.append(l);
    if (big) {
      const t = svgEl("text");
      t.setAttribute("x", 100 + sin * 93);
      t.setAttribute("y", 100 + cos * 93);
      t.textContent = String(i).padStart(2, "0");
      ticks.append(t);
    }
  }
  const hhmm = (m) => `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
  const cronDays = () => {
    const list = [...active].sort((a, b) => a - b);
    if (list.length === 7) return "*";
    const contiguous = list.length > 1 && list[list.length - 1] - list[0] === list.length - 1;
    return contiguous ? `${list[0]}-${list[list.length - 1]}` : list.join(",");
  };
  const nextRun = () => {
    const now = new Date();
    for (let i = 0; i < 8; i++) {
      const d = new Date(now);
      d.setDate(now.getDate() + i);
      d.setHours(Math.floor(minutes / 60), minutes % 60, 0, 0);
      if (active.has(d.getDay()) && d > now) return d;
    }
    return null;
  };
  function paintSchedule() {
    if (!hand.classList.contains("is-spinning")) {
      handDeg = (minutes / 1440) * 360;
      hand.style.transform = `rotate(${handDeg}deg)`;
    }
    dialTime.textContent = hhmm(minutes);
    dial.setAttribute("aria-valuenow", minutes);
    dial.setAttribute("aria-valuetext", hhmm(minutes));
    const none = active.size === 0;
    dayError.hidden = !none;
    ffBtn.disabled = none;
    cronEl.textContent = none ? "No days selected" : `${minutes % 60} ${Math.floor(minutes / 60)} * * ${cronDays()}`;
    const next = nextRun();
    if (next && !none) {
      const mins = Math.round((next - new Date()) / 60000);
      const day = next.toLocaleDateString("en-GB", { weekday: "short" });
      nextEl.textContent = `Next run: ${day} at ${hhmm(minutes)}, in ${mins >= 60 ? `${Math.floor(mins / 60)} h ` : ""}${mins % 60} min. If Alfred is closed then, you choose: catch up, skip, or ask.`;
    } else nextEl.textContent = "";
  }
  const setFromPointer = (e) => {
    const r = dial.getBoundingClientRect();
    const a = Math.atan2(e.clientX - (r.left + r.width / 2), -(e.clientY - (r.top + r.height / 2)));
    const m = ((a < 0 ? a + Math.PI * 2 : a) / (Math.PI * 2)) * 1440;
    minutes = (Math.round(m / 15) * 15) % 1440;
    paintSchedule();
  };
  dial.addEventListener("pointerdown", (e) => {
    dial.setPointerCapture(e.pointerId);
    setFromPointer(e);
    dial.addEventListener("pointermove", setFromPointer);
  });
  const stopDial = () => dial.removeEventListener("pointermove", setFromPointer);
  dial.addEventListener("pointerup", stopDial);
  dial.addEventListener("pointercancel", stopDial);
  dial.addEventListener("keydown", (e) => {
    const d = { ArrowRight: 15, ArrowUp: 15, ArrowLeft: -15, ArrowDown: -15, PageUp: 60, PageDown: -60 }[e.key];
    if (!d) return;
    e.preventDefault();
    minutes = (minutes + d + 1440) % 1440;
    paintSchedule();
  });
  paintSchedule();
  ffBtn.addEventListener("click", () => {
    if (!reduce) {
      hand.classList.add("is-spinning");
      hand.style.transform = `rotate(${handDeg + 360}deg)`;
      setTimeout(() => {
        hand.classList.remove("is-spinning");
        paintSchedule();
      }, 900);
    }
    triggerGraph.run();
  });

  /* File drop: only the file's name is read. */
  const drop = $("[data-drop]");
  const dropTitle = $("[data-drop-title]");
  const fireFile = (file) => {
    if (!file) return;
    dropTitle.textContent = `${file.name} changed. Run started.`;
    triggerGraph.run();
  };
  ["dragenter", "dragover"].forEach((t) =>
    drop.addEventListener(t, (e) => {
      e.preventDefault();
      drop.classList.add("is-over");
    }),
  );
  drop.addEventListener("dragleave", () => drop.classList.remove("is-over"));
  drop.addEventListener("drop", (e) => {
    e.preventDefault();
    drop.classList.remove("is-over");
    fireFile(e.dataTransfer.files[0]);
  });
  $("[data-file-input]").addEventListener("change", (e) => {
    fireFile(e.target.files[0]);
    e.target.value = "";
  });

  /* Webhook */
  const hookBtn = $("[data-send-hook]");
  const hookOut = $("[data-hook-response]");
  hookBtn.addEventListener("click", async () => {
    hookBtn.disabled = true;
    hookOut.classList.remove("is-ok");
    hookOut.textContent = "Sending request";
    await sleep(500);
    hookOut.textContent = `HTTP 202 {"ok":true,"runId":"run_${Math.random().toString(36).slice(2, 8)}"}`;
    hookOut.classList.add("is-ok");
    hookBtn.disabled = false;
    triggerGraph.run();
  });

  /* ---------- Copy buttons ---------- */
  document.addEventListener("click", async (e) => {
    const b = e.target.closest("[data-copy]");
    if (!b) return;
    try {
      await navigator.clipboard.writeText(b.dataset.copy);
      toast("Copied to clipboard.");
    } catch {
      toast("Copy failed. Select the text and copy it yourself.");
    }
  });

  /* ---------- Latest release, read live from GitHub ---------- */
  const releaseText = $("[data-release-text]");
  once($("[data-release]"), async () => {
    try {
      const res = await fetch("https://api.github.com/repos/Nirzhuk/alfred/releases/latest", { signal: AbortSignal.timeout(6000) });
      if (!res.ok) throw new Error(String(res.status));
      const r = await res.json();
      const date = new Date(r.published_at).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
      releaseText.textContent = `${r.tag_name}, published ${date}. Installers for macOS, Windows and Linux.`;
    } catch {
      releaseText.textContent = "The version could not be loaded just now. See every installer on GitHub Releases.";
    }
    releaseText.removeAttribute("aria-busy");
  }, 0.1);

  /* ---------- Final CTA: Alfred gets dressed ---------- */
  const finalSection = $("#final");
  const butler = $("[data-butler]");
  const finalCopy = $(".final-copy");
  const LAYERS = [
    ["jacket", 0, 0.2, { s: 0.8 }],
    ["lapels", 0.1, 0.34, { y: 340 }],
    ["shirt", 0.2, 0.44, { y: -420 }],
    ["button1", 0.42, 0.5, { s: 0 }],
    ["button2", 0.47, 0.55, { s: 0 }],
    ["wingL", 0.52, 0.72, { x: -640, r: -50 }],
    ["wingR", 0.52, 0.72, { x: 640, r: 50 }],
    ["knot", 0.68, 0.82, { y: -520, s: 1.5 }],
  ].map(([name, a, b, from]) => ({ el: $(`[data-layer="${name}"]`, butler), a, b, from }));
  const paintButler = () => {
    const r = finalSection.getBoundingClientRect();
    if (!reduce && (r.bottom < -100 || r.top > innerHeight + 100)) return;
    const p = reduce ? 1 : clamp((innerHeight * 0.5 - r.top) / (r.height - innerHeight * 0.5));
    for (const { el, a, b, from } of LAYERS) {
      const t = easeOut(span(p, a, b));
      const x = lerp(from.x ?? 0, 0, t);
      const y = lerp(from.y ?? 0, 0, t);
      const rot = lerp(from.r ?? 0, 0, t);
      const s = lerp(from.s ?? 1, 1, t);
      el.style.transform = `translate(${x}px, ${y}px) rotate(${rot}deg) scale(${s})`;
      el.style.opacity = span(p, a, a + (b - a) * 0.4);
    }
    const c = easeOut(span(p, 0.72, 0.92));
    finalCopy.style.opacity = c;
    finalCopy.style.transform = `translateY(${(1 - c) * 32}px)`;
    butler.classList.toggle("is-dressed", p >= 0.85);
  };
  scrollSubs.push(paintButler);
  const tieGroup = $("[data-tie]");
  butler.addEventListener("click", () => {
    tieGroup.classList.remove("is-twirl");
    void tieGroup.getBoundingClientRect();
    tieGroup.classList.add("is-twirl");
  });
  tieGroup.addEventListener("animationend", (e) => {
    if (e.animationName === "tie-twirl") tieGroup.classList.remove("is-twirl");
  });

  /* ---------- Quick Access ---------- */
  const quick = $("[data-quick]");
  const quickSearch = $("[data-quick-search]");
  let quickReturn = null;
  const closeQuick = () => {
    if (quick.hidden) return;
    quick.hidden = true;
    quickReturn?.focus({ preventScroll: true });
  };
  const QUICK_ITEMS = [
    { label: "Release notes", desc: "Manual run", icon: icon("#i-flow-arrow"), run: () => {
      $(".hero-demo").scrollIntoView({ block: "center" });
      hero.run();
    } },
    { label: "Triage Sentry issues", desc: "Next run: when an issue appears", icon: icon("#i-flow-arrow"), run: () => {
      $("#speak").scrollIntoView();
      askAlfred(SUGGESTIONS[1]);
    } },
    { label: "Morning inbox digest", desc: "Next run: tomorrow at 08:30", icon: icon("#i-flow-arrow"), run: () => {
      $("#speak").scrollIntoView();
      askAlfred(SUGGESTIONS[0]);
    } },
  ];
  const quickList = listbox({
    list: $("[data-quick-list]"),
    input: quickSearch,
    items: QUICK_ITEMS,
    empty: "No workflows match that search.",
    onPick(it) {
      closeQuick();
      it.run();
    },
  });
  const openQuick = () => {
    quickReturn = document.activeElement;
    quickSearch.value = "";
    quick.hidden = false;
    quickList.render();
    quickSearch.focus({ preventScroll: true });
  };
  $("[data-quick-open]").addEventListener("click", openQuick);
  quick.addEventListener("pointerdown", (e) => e.target === quick && closeQuick());
  addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.code === "Space") {
      e.preventDefault();
      if (quick.hidden) openQuick();
      else closeQuick();
    } else if (e.key === "Escape") {
      closeAdd();
      closeQuick();
      setMenu(false);
    }
  });

  requestTick();
})();
