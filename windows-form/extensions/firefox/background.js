// Shared by the Chrome and Firefox extensions (media.js is loaded by the manifest in
// Firefox and with importScripts in the Chrome service worker).
if (typeof importScripts === "function") importScripts("media.js");

const API = typeof browser !== "undefined" ? browser : chrome;
const IDM_BASE = "http://127.0.0.1:6800";
const IDM_URL = `${IDM_BASE}/add`;
const CONFIG_TTL_MS = 30 * 1000;
const BYPASS_MS = 4000;

// Used until the app answers (or when it is not running)
const DEFAULT_CONFIG = {
  auto_extensions: "zip rar r0* r1* arj gz sit sitx sea ace bz2 7z doc pdf ppt pps docx pptx mp3 wav wma mpa ram ra aac aif m4a tsa exe msi avi mpg mpe mpeg asf wmv mov qt rm mp4 flv m4v webm ogv ogg mkv ts tsv",
  excluded_sites: "",
  block_key: "Alt",
  intercept: true,
  video_button: true
};

let config = { ...DEFAULT_CONFIG };
let configAt = 0;
let bypassUntil = 0;

async function getConfig() {
  if (Date.now() - configAt < CONFIG_TTL_MS) return config;
  try {
    const resp = await fetch(`${IDM_BASE}/config`);
    if (resp.ok) config = { ...DEFAULT_CONFIG, ...(await resp.json()) };
  } catch (_) {
    // the app is not running: keep the last known settings
  }
  configAt = Date.now();
  return config;
}

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

function siteExcluded(url, cfg) {
  let host = "";
  try { host = new URL(url).hostname.toLowerCase(); } catch (_) { return false; }
  return cfg.excluded_sites.split(/\s+/).filter(Boolean).some(p => {
    p = p.toLowerCase();
    return p.startsWith("*.") ? host === p.slice(2) || host.endsWith(p.slice(1)) : host === p;
  });
}

function isDownloadLink(url, cfg) {
  if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) return false;
  const ext = getExt(url);
  if (!ext) return false;
  return cfg.auto_extensions.split(/\s+/).filter(Boolean).some(p => extMatches(p.toLowerCase(), ext)) && !siteExcluded(url, cfg);
}

async function sendToIdm(url, referrer, fileName) {
  try {
    // Cookies and User-Agent let the app download from sites that need the browser session
    let cookies = "";
    try {
      const list = await API.cookies.getAll({ url });
      cookies = list.map(c => `${c.name}=${c.value}`).join("; ");
    } catch (_) {}
    const resp = await fetch(IDM_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url, referrer: referrer || "", file_name: fileName || "", description: "", cookies, user_agent: navigator.userAgent })
    });
    return resp.ok;
  } catch (_) {
    return false;
  }
}

// Context menu: right-click on a link, video or audio
API.runtime.onInstalled.addListener(() => {
  API.contextMenus.create({ id: "idm-link", title: "Download with Iran Download Manager", contexts: ["link"] });
  API.contextMenus.create({ id: "idm-video", title: "Download with Iran Download Manager", contexts: ["video", "audio"] });
});

API.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (url) sendToIdm(url, tab?.url || "");
});

// Take over browser downloads of the configured file types
API.downloads.onCreated.addListener(async item => {
  const cfg = await getConfig();
  if (!cfg.intercept || Date.now() < bypassUntil) return;
  if (!isDownloadLink(item.url, cfg)) return;
  API.downloads.cancel(item.id);
  API.downloads.erase({ id: item.id });
  const sent = await sendToIdm(item.url, item.referrer || "");
  if (!sent) {
    API.downloads.download({ url: item.url });
  }
});

// Messages from the page scripts and the popup
API.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg) return false;
  if (msg.type === "ADD_DOWNLOAD") {
    sendToIdm(msg.url, msg.referrer || sender?.tab?.url || "", msg.fileName || "").then(sendResponse);
    return true;
  }
  if (msg.type === "GET_CONFIG") {
    getConfig().then(sendResponse);
    return true;
  }
  if (msg.type === "BYPASS") {
    // The user held the "let the browser download" key while clicking
    bypassUntil = Date.now() + BYPASS_MS;
    return false;
  }
  return false;
});
