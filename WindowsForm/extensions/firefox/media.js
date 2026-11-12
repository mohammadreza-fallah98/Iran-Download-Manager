// Remembers media responses (video/audio files and streams) seen in each tab,
// so the page script can offer them when a video is not a plain <video src>.
(() => {
  const api = typeof browser !== "undefined" ? browser : chrome;
  const MEDIA_EXT = /\.(mp4|webm|m4v|mkv|mov|avi|flv|mp3|m4a|aac|ogg|wav|m3u8|mpd)(\?|#|$)/i;
  const SEGMENT_EXT = /\.(ts|m4s)(\?|#|$)/i;
  const STREAM_EXT = /\.(m3u8|mpd)(\?|#|$)/i;
  const MIN_FILE_BYTES = 200 * 1024;
  const MAX_PER_TAB = 30;
  const found = new Map(); // tabId -> Map(url -> { url, size, kind, type })

  function header(details, name) {
    const h = (details.responseHeaders || []).find(x => x.name.toLowerCase() === name);
    return h ? String(h.value || "") : "";
  }

  function remember(tabId, item) {
    if (!found.has(tabId)) found.set(tabId, new Map());
    const m = found.get(tabId);
    m.set(item.url, item);
    while (m.size > MAX_PER_TAB) m.delete(m.keys().next().value);
    if (api.storage && api.storage.session) {
      api.storage.session.set({ ["media" + tabId]: [...m.values()] }).catch(() => {});
    }
  }

  api.webRequest.onHeadersReceived.addListener(details => {
    if (details.tabId < 0) return;
    if (details.statusCode !== 200 && details.statusCode !== 206) return;
    if (SEGMENT_EXT.test(details.url)) return;
    const type = header(details, "content-type").toLowerCase();
    const isStream = STREAM_EXT.test(details.url) || type.includes("mpegurl") || type.includes("dash+xml");
    const isFile = type.startsWith("video/") || type.startsWith("audio/") || MEDIA_EXT.test(details.url);
    if (!isStream && !isFile) return;
    const range = header(details, "content-range");
    const total = range.includes("/") ? parseInt(range.split("/").pop(), 10) : parseInt(header(details, "content-length"), 10);
    const size = Number.isFinite(total) ? total : 0;
    if (!isStream && size > 0 && size < MIN_FILE_BYTES) return;
    const hls = /\.m3u8(\?|#|$)/i.test(details.url) || type.includes("mpegurl");
    remember(details.tabId, { url: details.url, size, kind: isStream ? "stream" : "file", hls, dash: /\.mpd(\?|#|$)/i.test(details.url) || type.includes("dash+xml"), type });
  }, { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "other"] }, ["responseHeaders"]);

  api.tabs.onRemoved.addListener(tabId => {
    found.delete(tabId);
    if (api.storage && api.storage.session) api.storage.session.remove("media" + tabId).catch(() => {});
  });

  api.tabs.onUpdated.addListener((tabId, change) => {
    if (change.status === "loading" && change.url) {
      found.delete(tabId);
      if (api.storage && api.storage.session) api.storage.session.remove("media" + tabId).catch(() => {});
    }
  });

  api.runtime.onMessage.addListener((msg, sender, sendResponse) => {
    if (!msg || msg.type !== "GET_MEDIA" || !sender.tab) return false;
    const tabId = sender.tab.id;
    const mem = found.get(tabId);
    if (mem && mem.size) {
      sendResponse([...mem.values()]);
      return false;
    }
    if (api.storage && api.storage.session) {
      api.storage.session.get("media" + tabId).then(r => sendResponse(r["media" + tabId] || []), () => sendResponse([]));
      return true;
    }
    sendResponse([]);
    return false;
  });
})();
