const hosts = ["youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com", "youtu.be"];
const patterns = hosts.map(host => `*://${host}/*`);
browser.contextMenus.create({id: "link", title: "Open in Grayjay", contexts: ["link"], targetUrlPatterns: patterns});
const pages = hosts.filter(host => host !== "youtu.be").flatMap(host => [`*://${host}/watch*`, `*://${host}/shorts/*`]);
browser.contextMenus.create({id: "page", title: "Open in Grayjay", contexts: ["page"], documentUrlPatterns: pages});
async function open(url, tabId) {
  try {
    const result = await browser.runtime.sendNativeMessage("open_in_grayjay", {url});
    if (!result.ok) throw new Error(result.error);
    await browser.action.setBadgeText({tabId, text: ""});
    await browser.action.setTitle({tabId, title: "Open in Grayjay"});
  } catch (error) {
    await browser.action.setBadgeBackgroundColor({tabId, color: "#c62828"});
    await browser.action.setBadgeText({tabId, text: "!"});
    await browser.action.setTitle({tabId, title: String(error)});
  }
}
browser.contextMenus.onClicked.addListener((info, tab) => open(info.linkUrl || info.pageUrl, tab.id));
browser.action.onClicked.addListener(tab => open(tab.url, tab.id));
