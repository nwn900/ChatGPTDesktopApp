// Turns clicks on links that do not belong to the app into native calls that
// hand the URL to the operating system default browser. WebView2 own
// new-window/navigation events stay as the fallback for scripted navigation,
// but a plain click is routed explicitly so it cannot end up in an in-app
// popup window, in the app window itself, or nowhere at all.
(() => {
  "use strict";
  if (window.top !== window || window.__desktopLinks) return;
  const hosts = Array.isArray(window.__CHATGPT_IN_APP_HOSTS) ? window.__CHATGPT_IN_APP_HOSTS : [];
  const inApp = host => hosts.some(suffix => host === suffix || host.endsWith("." + suffix));
  window.__desktopLinks = true;

  function anchorOf(event) {
    const path = typeof event.composedPath === "function" ? event.composedPath() : [];
    const nodes = path.length ? path : [event.target];
    for (const node of nodes) {
      if (node && node.tagName === "A" && typeof node.getAttribute === "function" &&
          node.getAttribute("href") !== null) return node;
    }
    return event.target && typeof event.target.closest === "function"
      ? event.target.closest("a[href]") : null;
  }

  // Returns null when the click must keep its normal in-app behavior.
  function externalUrl(anchor) {
    if (anchor.hasAttribute("download")) return null;
    let url;
    try { url = new URL(anchor.getAttribute("href"), location.href); } catch { return null; }
    if (url.protocol !== "http:" && url.protocol !== "https:" && url.protocol !== "mailto:") return null;
    if (inApp(url.hostname)) return null;
    return url;
  }

  function intercept(event) {
    if (event.defaultPrevented) return;
    const anchor = anchorOf(event);
    if (!anchor) return;
    const url = externalUrl(anchor);
    if (!url) return;
    const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
    // Without native access the webview keeps its own external-link handling.
    if (!invoke) return;
    event.preventDefault();
    event.stopPropagation();
    const href = url.href;
    Promise.resolve(invoke("open_external", { url: href })).catch(error => {
      console.debug("External link:", error);
      // Refused or failed natively: fall back to the webview new-window path,
      // which also hands unknown hosts to the default browser.
      try { window.open(href, "_blank", "noopener,noreferrer"); } catch (fallback) {
        console.debug("External link fallback:", fallback);
      }
    });
  }

  document.addEventListener("click", intercept, true);
  document.addEventListener("auxclick", event => { if (event.button === 1) intercept(event); }, true);
})();
