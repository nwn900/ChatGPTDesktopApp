const {test} = require("node:test");
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const path = require("node:path");

const SOURCE = fs.readFileSync(path.join(__dirname, "../src-tauri/src/links.js"), "utf8");
const RUST = fs.readFileSync(path.join(__dirname, "../src-tauri/src/links.rs"), "utf8");
const IN_APP_HOSTS = [...RUST
 .match(/const IN_APP_HOSTS: &\[&str\] = &\[([\s\S]*?)\];/)[1]
 .matchAll(/"([^"]+)"/g)].map(match => match[1]);

const settle = () => new Promise(resolve => setTimeout(resolve, 0));

function anchor(href, attributes = {}) {
 const attrs = {href, ...attributes};
 return {
  tagName: "A",
  getAttribute: name => (name in attrs ? attrs[name] : null),
  hasAttribute: name => name in attrs,
  closest: () => null
 };
}

function harness(options = {}) {
 const listeners = [], calls = [], opened = [];
 const document = {addEventListener: (name, fn, capture) => listeners.push({name, fn, capture})};
 const window = {
  __CHATGPT_IN_APP_HOSTS: options.hosts || IN_APP_HOSTS,
  open: url => opened.push(url)
 };
 window.top = options.subframe ? {subframe: true} : window;
 if (options.tauri !== false) {
  window.__TAURI__ = {core: {invoke: (command, payload) => {
   calls.push({command, payload});
   return options.reject ? Promise.reject(new Error("command denied")) : Promise.resolve();
  }}};
 }
 const context = {window, document, console, URL, location: {href: "https://chatgpt.com/c/1"}};
 vm.runInNewContext(SOURCE, context);
 if (options.twice) vm.runInNewContext(SOURCE, context);
 return {
  listeners, calls, opened,
  fire(name, node, extra = {}) {
   const event = {
    defaultPrevented: false, propagationStopped: false, target: node, button: 0,
    composedPath: () => [node],
    preventDefault() { this.defaultPrevented = true; },
    stopPropagation() { this.propagationStopped = true; },
    ...extra
   };
   for (const listener of listeners.filter(l => l.name === name)) listener.fn(event);
   return event;
  }
 };
}

test("the injected host list exists and covers the app and its providers", () => {
 assert.ok(IN_APP_HOSTS.includes("chatgpt.com"));
 assert.ok(IN_APP_HOSTS.includes("auth.openai.com"));
 assert.ok(IN_APP_HOSTS.includes("accounts.google.com"));
 assert.ok(!IN_APP_HOSTS.includes("wikipedia.org"));
});

test("external link clicks are handed to the native browser command", async () => {
 const h = harness();
 const event = h.fire("click", anchor("https://en.wikipedia.org/wiki/Rust"));
 await settle();
 assert.equal(event.defaultPrevented, true);
 assert.equal(event.propagationStopped, true);
 assert.equal(h.calls.length, 1);
 assert.equal(h.calls[0].command, "open_external");
 assert.equal(h.calls[0].payload.url, "https://en.wikipedia.org/wiki/Rust");
 assert.deepEqual(h.opened, []);
});

test("openai.com content links leave the app", async () => {
 const h = harness();
 h.fire("click", anchor("https://openai.com/policies/terms-of-use"));
 await settle();
 assert.equal(h.calls.length, 1);
 assert.equal(h.calls[0].payload.url, "https://openai.com/policies/terms-of-use");
});

test("relative links stay in the app and protocol relative links resolve", async () => {
 const h = harness();
 assert.equal(h.fire("click", anchor("/c/2")).defaultPrevented, false);
 h.fire("click", anchor("//example.com/path"));
 await settle();
 assert.equal(h.calls.length, 1);
 assert.equal(h.calls[0].payload.url, "https://example.com/path");
});

test("mailto links open the default mail client", async () => {
 const h = harness();
 h.fire("click", anchor("mailto:support@example.com"));
 await settle();
 assert.equal(h.calls.length, 1);
 assert.equal(h.calls[0].payload.url, "mailto:support@example.com");
});

test("app and sign-in hosts keep their in-app behavior", async () => {
 const h = harness();
 for (const href of ["https://chatgpt.com/c/2", "https://auth.openai.com/authorize",
   "https://accounts.google.com/o/oauth2/v2/auth", "https://login.microsoftonline.com/common",
   "https://appleid.apple.com/auth/authorize"]) {
  assert.equal(h.fire("click", anchor(href)).defaultPrevented, false, href);
 }
 await settle();
 assert.equal(h.calls.length, 0);
});

test("non web schemes and downloads are left alone", async () => {
 const h = harness();
 assert.equal(h.fire("click", anchor("about:blank")).defaultPrevented, false);
 assert.equal(h.fire("click", anchor("blob:https://chatgpt.com/9f2b")).defaultPrevented, false);
 assert.equal(h.fire("click", anchor("javascript:alert(1)")).defaultPrevented, false);
 assert.equal(h.fire("click", anchor("blob:https://chatgpt.com/9f2b", {download: "answer.md"})).defaultPrevented, false);
 assert.equal(h.fire("click", anchor("https://example.com/a", {download: "report.pdf"})).defaultPrevented, false);
 await settle();
 assert.equal(h.calls.length, 0);
});

test("an already handled click is not intercepted twice", async () => {
 const h = harness();
 h.fire("click", anchor("https://example.com/a"), {defaultPrevented: true});
 await settle();
 assert.equal(h.calls.length, 0);
});

test("middle clicks on links head to the browser too", async () => {
 const h = harness();
 h.fire("auxclick", anchor("https://example.com/a"), {button: 1});
 await settle();
 assert.equal(h.calls.length, 1);
 h.fire("auxclick", anchor("https://example.com/b"), {button: 2});
 await settle();
 assert.equal(h.calls.length, 1);
});

test("without native access the webview keeps handling external links", async () => {
 const h = harness({tauri: false});
 const event = h.fire("click", anchor("https://example.com/a"));
 await settle();
 assert.equal(event.defaultPrevented, false);
 assert.equal(h.calls.length, 0);
});

test("a refused command falls back to the webview new-window path", async () => {
 const h = harness({reject: true});
 h.fire("click", anchor("https://example.com/a"));
 await settle();
 assert.deepEqual(h.opened, ["https://example.com/a"]);
});

test("clicks that miss a link are ignored", async () => {
 const h = harness();
 assert.equal(h.fire("click", {tagName: "SPAN", closest: () => null}).defaultPrevented, false);
 await settle();
 assert.equal(h.calls.length, 0);
});

test("subframes and repeat injections install nothing extra", async () => {
 const frame = harness({subframe: true});
 frame.fire("click", anchor("https://example.com/a"));
 await settle();
 assert.equal(frame.calls.length, 0);
 const twice = harness({twice: true});
 assert.equal(twice.listeners.length, 2);
 twice.fire("click", anchor("https://example.com/a"));
 await settle();
 assert.equal(twice.calls.length, 1);
});
