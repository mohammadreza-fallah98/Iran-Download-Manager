const API = typeof browser !== "undefined" ? browser : chrome;
const BADGE_COLOR = "#ff6611";

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

// File types that are worth a download badge even when they are not in the app's list
const EXTRA_EXT = new Set([
  "xpi", "crx", "msi", "msix", "msu", "cab", "deb", "rpm", "dmg", "pkg", "apk", "aab", "ipa", "iso", "img", "bin",
  "torrent", "jar", "whl", "tar", "tgz", "xz", "zst", "lz", "7z", "rar", "zip", "gz", "bz2", "exe",
  "epub", "mobi", "csv", "xls", "xlsx", "xlsm", "ppt", "pptx", "odt", "ods", "odp", "rtf", "pdf", "doc", "docx",
  "mkv", "mp4", "webm", "mov", "avi", "flv", "m4v", "wmv", "mp3", "flac", "ogg", "wav", "aac", "m4a"
]);

function isDownloadLink(url, anchor) {
  if (!config || !config.intercept) return false;
  if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) return false;
  if (siteExcluded(url)) return false;
  // A link that asks for a download, or points to a file of a known type
  if (anchor && anchor.hasAttribute("download")) return true;
  const ext = getExt(url);
  if (!ext) return false;
  return EXTRA_EXT.has(ext) || config.auto_extensions.split(/\s+/).filter(Boolean).some(p => extMatches(p.toLowerCase(), ext));
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
  const style = document.createElement("style");
  style.textContent = `
      :host { all: initial; }
      button {
        all: unset; box-sizing: border-box; display: inline-block;
        padding: 1px 6px; background: ${BADGE_COLOR}; color: #fff; border-radius: 3px;
        font: 11px/16px sans-serif; cursor: pointer !important; pointer-events: auto !important;
        user-select: none; transition: filter .12s, transform .12s;
      }
      button:hover { filter: brightness(1.2); transform: translateY(-1px); }
      button:focus-visible { outline: 2px solid #fff; outline-offset: 1px; }
  `;
  // Built with DOM calls only
  const button = document.createElement("button");
  button.type = "button";
  button.setAttribute("role", "button");
  button.title = "Download with Iran Download Manager";
  button.textContent = "⬇ IDM";
  root.append(style, button);
  button.addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    sendDownload(anchor);
  });
  anchor.insertAdjacentElement("afterend", host);
}

function scanLinks() {
  document.querySelectorAll("a[href]").forEach(a => {
    if (isDownloadLink(a.href, a)) injectButton(a);
  });
}

API.runtime.sendMessage({ type: "GET_CONFIG" }).then(cfg => {
  config = cfg;
  scanLinks();
  const observer = new MutationObserver(scanLinks);
  observer.observe(document.body, { childList: true, subtree: true });
}).catch(() => {});
