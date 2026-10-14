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
  window.__desktopWatcher = true;
  // Perf: visible scans at 1000ms (was 500ms), hidden at 3000ms so
  // background WebView2 wakeups drop ~6x while completion notices
  // still fire while tray-hidden. QUIET_MS unchanged to keep semantics.
  const SCAN_VISIBLE = 1000, SCAN_HIDDEN = 3000, QUIET_MS = 2500;
  const isHidden = () => document.hidden === true;
  const scanDelay = () => (isHidden() ? SCAN_HIDDEN : SCAN_VISIBLE);
  let baseline = null, active = false, changedAt = 0, timer = 0, route = location.pathname;
  let pending = false, hasChanged = false, dirty = true, lastBusy = false;
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
    // Fast path: no DOM mutations and stop-button state unchanged since
    // last scan — skip querySelectorAll + textContent alloc entirely.
    // Exception: while waiting out the quiet period (active + changed +
    // idle) time alone can trigger the notice, so fall through there.
    if (!dirty && busyNow === lastBusy && !(active && hasChanged && !busyNow)) {
      if (active) schedule();
      return;
    }
    dirty = false; lastBusy = busyNow;
    const nodes = document.querySelectorAll(config[0]);
    const last = nodes[nodes.length - 1];
    // textContent does not force layout, unlike innerText.
    const text = last ? (last.textContent || "").trim() : "";
    const busy = busyNow;
    if (text !== baseline) { if (active || busy) hasChanged = true; changedAt = Date.now(); baseline = text; }
    if (busy) { active = true; changedAt = Date.now(); }
    if (active && hasChanged && !busy && text && Date.now() - changedAt >= QUIET_MS && !pending) {
      const invoke = window.__TAURI__?.core?.invoke;
      if (!invoke) { schedule(); return; }
      pending = true;
      try {
        await invoke("notify_answer", { body: text.slice(0, 300) });
        active = false; hasChanged = false;
      } catch (error) { console.debug("Desktop notification:", error); }
      finally { pending = false; }
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
        if (timer && typeof clearTimeout === "function") clearTimeout(timer);
        timer = 0;
        // Re-arm at the slower hidden cadence so a completion that
        // finishes while tray-hidden still notifies. Chromium clamps
        // hidden timers to >=1000ms; SCAN_HIDDEN respects that.
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
