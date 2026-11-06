const IDM_URL = "http://127.0.0.1:6800/add";

const KNOWN_EXT = new Set([
  "zip","rar","7z","gz","bz2","tar","arj","ace","sit","r00","r01","r02","r03",
  "exe","msi",
  "mp3","wav","wma","mpa","ram","ra","aac","aif","m4a",
  "avi","mpg","mpeg","asf","wmv","mov","qt","rm","mp4","flv","m4v","webm","ogv","mkv","ts",
  "doc","pdf","ppt","pps","docx","pptx",
  "iso","img","bin","dmg","apk","deb","rpm"
]);

function getExt(url) {
  try {
    const path = new URL(url).pathname;
    const dot = path.lastIndexOf(".");
    if (dot >= 0) return path.slice(dot + 1).toLowerCase();
  } catch (_) {}
  return "";
}

function isDownloadLink(url) {
  if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) return false;
  return KNOWN_EXT.has(getExt(url));
}

async function sendToIdm(url, referrer, fileName) {
  try {
    const resp = await fetch(IDM_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url, referrer: referrer || "", file_name: fileName || "", description: "" })
    });
    return resp.ok;
  } catch (_) {
    return false;
  }
}

// Context menu
browser.contextMenus.create({
  id: "idm-link",
  title: "Download with Iran Download Manager",
  contexts: ["link"]
});
browser.contextMenus.create({
  id: "idm-video",
  title: "Download with Iran Download Manager",
  contexts: ["video", "audio"]
});

browser.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (url) sendToIdm(url, tab?.url || "");
});

// Intercept downloads
browser.downloads.onCreated.addListener(async (item) => {
  if (!isDownloadLink(item.url)) return;
  browser.downloads.cancel(item.id);
  browser.downloads.erase({ id: item.id });
  const sent = await sendToIdm(item.url, item.referrer || "");
  if (!sent) {
    browser.downloads.download({ url: item.url });
  }
});

// Messages from content / popup
browser.runtime.onMessage.addListener((msg, sender) => {
  if (msg.type === "ADD_DOWNLOAD") {
    return sendToIdm(msg.url, msg.referrer || sender?.tab?.url || "", msg.fileName || "");
  }
});
