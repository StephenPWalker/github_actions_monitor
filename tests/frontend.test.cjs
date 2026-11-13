const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const path = require("node:path");

// A small DOM boundary double: no browser or JavaScript dependencies required.
class Element {
  constructor(tag = "div") {
    this.tagName = tag;
    this.children = [];
    this.events = new Map();
    this.attributes = new Map();
    this.value = "";
    this.hidden = false;
    this.disabled = false;
    this._text = "";
  }
  set textContent(value) { this._text = String(value); this.children = []; }
  get textContent() { return this._text + this.children.map((c) => c.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this._text = ""; this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, value); }
  addEventListener(name, handler) { this.events.set(name, handler); }
  fire(name) { return this.events.get(name)({ preventDefault() {} }); }
  focus() {}
}

function setup(fetch, now = () => Date.now()) {
  const nodes = new Map();
  const get = (id) => {
    if (!nodes.has(id)) nodes.set(id, new Element());
    return nodes.get(id);
  };
  const timers = new Map();
  let next = 0;
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, "../web/app.js"), "utf8"), {
    document: { getElementById: get, createElement: (tag) => new Element(tag) },
    fetch, AbortController, Date: class extends Date { static now() { return now(); } }, encodeURIComponent,
    setTimeout: (fn, delay) => { const id = ++next; timers.set(id, { fn, delay }); return id; },
    clearTimeout: (id) => timers.delete(id),
  });
  const connect = async () => {
    get("username").value = "octocat";
    get("token").value = "test-only-placeholder";
    get("connect-form").fire("submit");
    await new Promise(setImmediate);
  };
  return { get, timers, connect };
}

function data() {
  return {
    username: "octocat", repositories: 1, organizations: 0,
    summary: { available: 1, busy: 0, offline: 0, queued: 1, running: 0 },
    runners: [{ id: 1, key: "repository:octocat/alpha:1", name: "local-runner", repo: "octocat/alpha", scope: "repository", state: "available", labels: ["self-hosted"] }],
    jobs: [{ id: 2, name: "<script>untrusted</script>", repo: "octocat/alpha", status: "queued", labels: ["self-hosted"], matching_runner_keys: ["repository:octocat/alpha:1"], assigned_runner_key: null, runner_id: null }],
    warnings: [],
  };
}
const ok = (value = data()) => ({ ok: true, json: async () => value });

test("connect polls every five seconds and renders safe clickable job cards", async () => {
  const requests = [];
  const app = setup(async (url, options) => { requests.push({ url, options }); return ok(); });
  await app.connect();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].options.method, "POST");
  assert.equal(requests[0].options.headers["X-Monitor-Request"], "1");
  assert.equal(app.get("token").value, "");
  const poll = [...app.timers.values()].find((t) => t.delay <= 5000);
  assert.ok(poll);
  await poll.fn();
  assert.equal(requests.length, 2);
  const card = app.get("jobs").children[0].children[0];
  assert.equal(card.href, "https://github.com/octocat/alpha/actions");
  assert.equal(card.target, "_blank");
  assert.match(card.textContent, /<script>untrusted<\/script>/);
  assert.equal(card.children.some((node) => node.tagName === "script"), false);
});

test("slow requests never overlap and disconnect discards late responses", async () => {
  let resolve;
  let calls = 0;
  const app = setup(() => { calls++; return new Promise((done) => { resolve = done; }); });
  await app.connect();
  await app.get("refresh").fire("click");
  assert.equal(calls, 1);
  app.get("disconnect").fire("click");
  resolve(ok());
  await new Promise(setImmediate);
  assert.equal(app.timers.size, 0);
  assert.equal(app.get("username").value, "");
  assert.equal(app.get("token").value, "");
  assert.equal(app.get("available").textContent, "—");
  assert.equal(app.get("refresh-label").textContent, "Not connected");
});

test("refresh errors preserve previous data and mark it stale", async () => {
  let fail = false;
  const app = setup(async () => {
    if (fail) throw new Error("Connection unavailable");
    return ok();
  });
  await app.connect();
  fail = true;
  await app.get("refresh").fire("click");
  assert.equal(app.get("available").textContent, "1");
  assert.match(app.get("error").textContent, /last successful snapshot/);
  assert.equal(app.get("error").hidden, false);
});

test("rate limiting backs off and partial data remains visibly incomplete", async () => {
  const value = data();
  value.warnings = ["GitHub rate limit reached."];
  const app = setup(async () => ok(value));
  await app.connect();
  assert.match(app.get("warnings").textContent, /Incomplete snapshot/);
  assert.match(app.get("refresh-label").textContent, /retry in 60s/);
  assert.ok([...app.timers.values()].some((t) => t.delay === 60000));
});

test("five-second cadence includes request time instead of adding it", async () => {
  let now = 0;
  const app = setup(async () => { now += 2000; return ok(); }, () => now);
  await app.connect();
  assert.ok([...app.timers.values()].some((t) => t.delay === 3000));
});

test("shared organization runners show jobs from multiple repositories", async () => {
  const value = data();
  value.organizations = 1;
  value.repositories = 2;
  value.runners = [{ id: 5, key: "organization:octo-team:5", name: "shared-arm", repo: null, organization: "octo-team", scope: "organization", state: "busy", labels: ["self-hosted"] }];
  value.jobs = [
    { id: 10, name: "build-one", repo: "octo-team/one", status: "in_progress", runner_id: 5, assigned_runner_key: "organization:octo-team:5", matching_runner_keys: ["organization:octo-team:5"] },
    { id: 11, name: "build-two", repo: "octo-team/two", status: "queued", runner_id: null, assigned_runner_key: null, matching_runner_keys: ["organization:octo-team:5"] },
  ];
  const app = setup(async () => ok(value));
  await app.connect();
  const runner = app.get("runners").children[0];
  assert.match(runner.textContent, /octo-team · Organization/);
  assert.match(runner.textContent, /build-one/);
  assert.match(runner.textContent, /build-two/);
  assert.match(app.get("scope").textContent, /1 organization/);
  const cards = app.get("jobs").children[0].children;
  assert.equal(cards[0].href,"https://github.com/octo-team/one/actions");
  assert.equal(cards[1].href,"https://github.com/octo-team/two/actions");
  assert.match(cards[1].textContent,/shared-arm/);
});
