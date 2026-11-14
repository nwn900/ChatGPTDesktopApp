// Watches the page for a finished answer and asks the host for a notification.
//
// Cheap by construction: a MutationObserver marks the page dirty, a debounced
// scan reads the last assistant message's textContent (which does not force
// layout), and the scan backs off to a slower cadence while the page is hidden.
// Only the last 300 characters of the answer are sent, and only once per
// completion.
(() => {
  "use strict";
  if (window.top !== window || window.__desktopWatcher) return;
  const configs = {
    "chatgpt.com": ['[data-message-author-role="assistant"]', 'button[data-testid="stop-button"],button[aria-label*="Stop"]'],
    "gemini.google.com": ['model-response', 'button[aria-label*="Stop"],.stop-button'],
    "claude.ai": ['[data-is-streaming],.font-claude-response', 'button[aria-label*="Stop"],[data-is-streaming="true"]'],
    "grok.com": ['.message-bubble', 'button[aria-label*="Stop"],button[data-testid="stop-button"]']
  };
  const config = configs[location.hostname];
  if (!config) return;
  const bridge = window.chrome && window.chrome.webview;
  if (!bridge) return;
  window.__desktopWatcher = true;

  const SCAN_VISIBLE = 1000, SCAN_HIDDEN = 3000, QUIET_MS = 2500;
  const isHidden = () => document.hidden === true;
  const scanDelay = () => (isHidden() ? SCAN_HIDDEN : SCAN_VISIBLE);
  let baseline = null, active = false, changedAt = 0, timer = 0, route = location.pathname;
  let pending = false, hasChanged = false, dirty = true, lastBusy = false;

  function notify(text) {
    try {
      bridge.postMessage("notify\n" + text.slice(0, 300));
    } catch (error) {
      console.debug("Desktop notification:", error);
    }
  }

  async function scan() {
    timer = 0;
    const busyNow = !!document.querySelector(config[1]);
    const routeNow = location.pathname;
    if (routeNow !== route || baseline === null) {
      const nodes = document.querySelectorAll(config[0]);
      const last = nodes[nodes.length - 1];
      const text = last ? (last.textContent || "").trim() : "";
      route = routeNow; baseline = text; active = busyNow; lastBusy = busyNow;
      hasChanged = false; dirty = false; changedAt = Date.now();
      if (active) schedule();
      return;
    }
    // Fast path: nothing changed since the last scan.
    if (!dirty && busyNow === lastBusy && !(active && hasChanged && !busyNow)) {
      if (active) schedule();
      return;
    }
    dirty = false; lastBusy = busyNow;
    const nodes = document.querySelectorAll(config[0]);
    const last = nodes[nodes.length - 1];
    const text = last ? (last.textContent || "").trim() : "";
    const busy = busyNow;
    if (text !== baseline) { if (active || busy) hasChanged = true; changedAt = Date.now(); baseline = text; }
    if (busy) { active = true; changedAt = Date.now(); }
    if (active && hasChanged && !busy && text && Date.now() - changedAt >= QUIET_MS && !pending) {
      pending = true;
      notify(text);
      active = false; hasChanged = false;
      pending = false;
    }
    if (active) schedule();
  }

  function schedule() { if (!timer) timer = setTimeout(scan, scanDelay()); }
  function markDirty() { dirty = true; schedule(); }
  function kick() { active = true; dirty = true; changedAt = Date.now(); schedule(); }

  function start() {
    document.addEventListener("submit", kick, true);
    document.addEventListener("keydown", event => {
      if (event.key === "Enter" && !event.shiftKey && !event.isComposing &&
          event.target?.matches?.("textarea,[contenteditable=true]")) {
        kick();
      }
    }, true);
    document.addEventListener("visibilitychange", () => {
      if (isHidden()) {
        if (timer) clearTimeout(timer);
        timer = 0;
        if (active && !pending) timer = setTimeout(scan, SCAN_HIDDEN);
      } else {
        dirty = true;
        if (active) schedule();
      }
    }, true);
    new MutationObserver(markDirty).observe(document.documentElement,
      { subtree: true, childList: true, characterData: true, attributes: true,
        attributeFilter: ["aria-label", "data-is-streaming"] });
    scan();
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
  else start();
})();
