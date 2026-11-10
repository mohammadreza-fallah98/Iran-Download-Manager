// Shows a "Download this video" button over playing/hovered videos.
(() => {
  if (window.top !== window && window.innerWidth < 300) return; // tiny iframes (ads)
  const api = typeof browser !== "undefined" ? browser : chrome;
  const MIN_W = 220;
  const MIN_H = 120;
  const HIDE_AFTER_MS = 2500;

  let host = null;
  let root = null;
  let panel = null;
  let toast = null;
  let current = null;
  let hideTimer = null;
  let lastMove = 0;

  function build() {
    if (host) return;
    host = document.createElement("div");
    host.style.cssText = "position:fixed !important; z-index:2147483647 !important; display:none; pointer-events:auto !important; cursor:pointer !important;";
    root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .btn { all: unset; box-sizing: border-box; display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; background: #ff6611;
               color: #fff; border-radius: 6px; font: 600 13px/18px sans-serif; cursor: pointer !important; box-shadow: 0 2px 8px rgba(0,0,0,.45);
               user-select: none; transition: filter .12s, transform .12s; }
        .btn:hover { filter: brightness(1.15); transform: translateY(-1px); }
        .panel { margin-top: 6px; min-width: 280px; max-width: 420px; background: #1f2330; color: #e8eaf0; border-radius: 8px;
                 box-shadow: 0 4px 16px rgba(0,0,0,.5); font: 12px/16px sans-serif; overflow: hidden; }
        .item { display: block; width: 100%; box-sizing: border-box; padding: 8px 12px; text-align: left; background: none; border: 0; color: inherit;
                cursor: pointer; border-bottom: 1px solid #323849; }
        .item:hover:not([disabled]) { background: #2d3448; }
        .item[disabled] { opacity: .5; cursor: not-allowed; }
        .item small { display: block; opacity: .7; word-break: break-all; }
        .toast { margin-top: 6px; padding: 6px 10px; background: #1f2330; color: #fff; border-radius: 6px; font: 12px/16px sans-serif; }
      </style>
      <button class="btn" type="button" title="Download with Iran Download Manager">⬇ Download this video</button>
      <div class="panel" hidden></div>
      <div class="toast" hidden></div>`;
    root.querySelector(".btn").addEventListener("click", onButton);
    panel = root.querySelector(".panel");
    toast = root.querySelector(".toast");
    host.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    host.addEventListener("mouseleave", scheduleHide);
    document.documentElement.appendChild(host);
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hide, HIDE_AFTER_MS);
  }

  function hide() {
    if (!host) return;
    host.style.display = "none";
    panel.hidden = true;
    toast.hidden = true;
    current = null;
  }

  function usable(v) {
    const r = v.getBoundingClientRect();
    return r.width >= MIN_W && r.height >= MIN_H && r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth;
  }

  function show(v) {
    build();
    const r = v.getBoundingClientRect();
    host.style.display = "block";
    const width = host.offsetWidth || 190;
    host.style.top = Math.max(8, Math.min(innerHeight - 50, r.top + 10)) + "px";
    host.style.left = Math.max(8, Math.min(innerWidth - width - 8, r.right - width - 10)) + "px";
    current = v;
    scheduleHide();
  }

  function onMove(e) {
    const now = Date.now();
    if (now - lastMove < 100) return;
    lastMove = now;
    for (const v of document.querySelectorAll("video")) {
      if (!usable(v)) continue;
      const r = v.getBoundingClientRect();
      if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) {
        if (current !== v || !host || host.style.display === "none") show(v);
        else scheduleHide();
        return;
      }
    }
  }

  function sizeText(bytes) {
    if (!bytes) return "unknown size";
    const mb = bytes / 1048576;
    return mb >= 1024 ? (mb / 1024).toFixed(2) + " GB" : mb.toFixed(1) + " MB";
  }

  function fileExt(url, type) {
    try {
      const m = new URL(url).pathname.match(/\.([a-z0-9]{2,4})$/i);
      if (m) return m[1].toLowerCase();
    } catch (_) {}
    if (type && type.includes("webm")) return "webm";
    if (type && type.startsWith("audio/")) return "mp3";
    return "mp4";
  }

  function makeName(item) {
    const title = (document.title || "video").replace(/[\\/:*?"<>|\u0000-\u001f]/g, " ").replace(/\s+/g, " ").trim().slice(0, 80) || "video";
    return title + "." + fileExt(item.url, item.type);
  }

  async function candidates(v) {
    const list = [];
    const seen = new Set();
    const add = item => { if (!seen.has(item.url)) { seen.add(item.url); list.push(item); } };
    const direct = v.currentSrc || v.src;
    if (direct && /^https?:/i.test(direct)) add({ url: direct, size: 0, kind: "file", type: "" });
    try {
      const sniffed = await api.runtime.sendMessage({ type: "GET_MEDIA" });
      for (const m of sniffed || []) add(m);
    } catch (_) {}
    return list;
  }

  function say(text) {
    toast.textContent = text;
    toast.hidden = false;
    setTimeout(() => { toast.hidden = true; }, 4000);
  }

  function send(item) {
    api.runtime.sendMessage({ type: "ADD_DOWNLOAD", url: item.url, referrer: location.href, fileName: makeName(item) });
    panel.hidden = true;
    say("Sent to Iran Download Manager");
  }

  async function onButton(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!current) return;
    const list = await candidates(current);
    const files = list.filter(i => i.kind === "file" || i.hls || i.dash);
    if (list.length === 0) {
      say("No downloadable address found for this video.");
      return;
    }
    if (files.length === 1 && list.length === 1) {
      send(files[0]);
      return;
    }
    panel.textContent = "";
    for (const item of list) {
      const b = document.createElement("button");
      b.className = "item";
      b.type = "button";
      const isStream = false;
      b.disabled = isStream;
      const head = document.createElement("span");
      head.textContent = item.dash ? "DASH stream (best quality, video + audio)" : item.hls ? "HLS stream (best quality)" : fileExt(item.url, item.type).toUpperCase() + "  " + sizeText(item.size);
      const small = document.createElement("small");
      small.textContent = item.url.length > 90 ? item.url.slice(0, 90) + "..." : item.url;
      b.append(head, small);
      if (!isStream) b.addEventListener("click", () => send(item));
      panel.append(b);
    }
    panel.hidden = false;
  }

  api.runtime.sendMessage({ type: "GET_CONFIG" }).then(cfg => {
    if (cfg && cfg.video_button === false) return;
    document.addEventListener("mousemove", onMove, { passive: true, capture: true });
    window.addEventListener("scroll", () => { if (current && host.style.display !== "none") show(current); }, { passive: true });
  }).catch(() => {});
})();
