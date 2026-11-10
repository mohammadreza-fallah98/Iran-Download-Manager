const API = typeof browser !== "undefined" ? browser : chrome;
const BADGE_COLOR = "#1a73e8";

let config = null; // { auto_extensions, excluded_sites, block_key, intercept, ... } from the app

function getExt(url) {
  try {
    const path = new URL(url).pathname;
    const dot = path.lastIndexOf(".");
    if (dot >= 0) return path.slice(dot + 1).toLowerCase();
  } catch (_) {}
  return "";
}

function extMatches(pattern, ext) {
  return pattern.endsWith("*") ? ext.startsWith(pattern.slice(0, -1)) : pattern === ext;
}

function siteExcluded(url) {
  let host = "";
  try { host = new URL(url).hostname.toLowerCase(); } catch (_) { return false; }
  return config.excluded_sites.split(/\s+/).filter(Boolean).some(p => {
    p = p.toLowerCase();
    return p.startsWith("*.") ? host === p.slice(2) || host.endsWith(p.slice(1)) : host === p;
  });
}

function isDownloadLink(url) {
  if (!config || !config.intercept) return false;
  if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) return false;
  const ext = getExt(url);
  if (!ext) return false;
  return config.auto_extensions.split(/\s+/).filter(Boolean).some(p => extMatches(p.toLowerCase(), ext)) && !siteExcluded(url);
}

// True while the user holds the key that lets the browser handle the download itself
function blockKeyHeld(e) {
  switch (config && config.block_key) {
    case "Alt": return e.altKey;
    case "Ctrl": return e.ctrlKey;
    case "Shift": return e.shiftKey;
    default: return false;
  }
}

function sendDownload(anchor) {
  API.runtime.sendMessage({ type: "ADD_DOWNLOAD", url: anchor.href, referrer: location.href, fileName: "" });
}

function injectButton(anchor) {
  if (anchor.dataset.idmInjected) return;
  anchor.dataset.idmInjected = "1";

  // Intercept direct click on the link itself
  anchor.addEventListener("click", e => {
    if (blockKeyHeld(e)) {
      API.runtime.sendMessage({ type: "BYPASS" });
      return; // let the browser download it
    }
    e.preventDefault();
    e.stopPropagation();
    sendDownload(anchor);
  }, true);

  // Shadow DOM keeps the site's CSS from overriding cursor / pointer-events
  const host = document.createElement("span");
  host.style.cssText = "display:inline-block !important; vertical-align:middle !important; margin-left:4px !important; pointer-events:auto !important; cursor:pointer !important;";
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML = `
    <style>
      :host { all: initial; }
      button {
        all: unset; box-sizing: border-box; display: inline-block;
        padding: 1px 6px; background: ${BADGE_COLOR}; color: #fff; border-radius: 3px;
        font: 11px/16px sans-serif; cursor: pointer !important; pointer-events: auto !important;
        user-select: none; transition: filter .12s, transform .12s;
      }
      button:hover { filter: brightness(1.2); transform: translateY(-1px); }
      button:focus-visible { outline: 2px solid #fff; outline-offset: 1px; }
    </style>
    <button type="button" role="button" title="Download with Iran Download Manager">⬇ IDM</button>`;
  root.querySelector("button").addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    sendDownload(anchor);
  });
  anchor.insertAdjacentElement("afterend", host);
}

function scanLinks() {
  document.querySelectorAll("a[href]").forEach(a => {
    if (isDownloadLink(a.href)) injectButton(a);
  });
}

API.runtime.sendMessage({ type: "GET_CONFIG" }).then(cfg => {
  config = cfg;
  scanLinks();
  const observer = new MutationObserver(scanLinks);
  observer.observe(document.body, { childList: true, subtree: true });
}).catch(() => {});
