const IDM_URL = "http://127.0.0.1:6800/add";

async function addDownload(url, referrer) {
  const resp = await fetch(IDM_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ url, referrer: referrer || "", file_name: "", description: "" })
  });
  if (!resp.ok) throw new Error(`Server returned ${resp.status}`);
  const data = await resp.json();
  if (!data.ok) throw new Error("IDM rejected the request");
}

document.getElementById("add").addEventListener("click", async () => {
  const url = document.getElementById("url").value.trim();
  const msg = document.getElementById("msg");
  msg.className = "msg";
  msg.textContent = "";

  if (!url.startsWith("http://") && !url.startsWith("https://")) {
    msg.className = "msg err";
    msg.textContent = "Please enter a valid http(s) URL.";
    return;
  }

  try {
    const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
    await addDownload(url, tab?.url || "");
    msg.className = "msg ok";
    msg.textContent = "Added to Iran Download Manager!";
  } catch (e) {
    msg.className = "msg err";
    msg.textContent = `Could not reach IDM: ${e.message}`;
  }
});

browser.tabs.query({ active: true, currentWindow: true }).then(([tab]) => {
  if (tab?.url && (tab.url.startsWith("http://") || tab.url.startsWith("https://"))) {
    document.getElementById("url").value = tab.url;
  }
});
