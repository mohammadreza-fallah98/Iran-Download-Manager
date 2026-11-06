const KNOWN_EXT = new Set([
  "zip","rar","7z","gz","bz2","tar","arj","ace","sit",
  "exe","msi",
  "mp3","wav","wma","mpa","ram","ra","aac","aif","m4a",
  "avi","mpg","mpeg","asf","wmv","mov","qt","rm","mp4","flv","m4v","webm","ogv","mkv","ts",
  "doc","pdf","ppt","pps","docx","pptx",
  "iso","img","bin","apk","deb","rpm"
]);

function getExt(url) {
  try {
    const path = new URL(url).pathname;
    const dot = path.lastIndexOf(".");
    if (dot >= 0) return path.slice(dot + 1).toLowerCase().split("?")[0];
  } catch (_) {}
  return "";
}

function isDownloadLink(url) {
  if (!url || (!url.startsWith("http://") && !url.startsWith("https://"))) return false;
  return KNOWN_EXT.has(getExt(url));
}

function injectButton(anchor) {
  if (anchor.dataset.idmInjected) return;
  anchor.dataset.idmInjected = "1";

  // Intercept direct click on the link itself
  anchor.addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    chrome.runtime.sendMessage({ type: "ADD_DOWNLOAD", url: anchor.href, referrer: location.href, fileName: "" });
  }, true);

  const btn = document.createElement("span");
  btn.textContent = "⬇ IDM";
  btn.title = "Download with Iran Download Manager";
  btn.style.cssText = `
    display:inline-block; margin-left:4px; padding:1px 5px;
    background:#1a73e8; color:#fff; border-radius:3px;
    font-size:11px; cursor:pointer; vertical-align:middle;
    font-family:sans-serif; line-height:16px;
  `;
  btn.addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    chrome.runtime.sendMessage({ type: "ADD_DOWNLOAD", url: anchor.href, referrer: location.href, fileName: "" });
  });
  anchor.insertAdjacentElement("afterend", btn);
}

function scanLinks() {
  document.querySelectorAll("a[href]").forEach(a => {
    if (isDownloadLink(a.href)) injectButton(a);
  });
}

scanLinks();

const observer = new MutationObserver(scanLinks);
observer.observe(document.body, { childList: true, subtree: true });
