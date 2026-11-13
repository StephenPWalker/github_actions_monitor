"use strict";

const $ = (id) => document.getElementById(id);
let session = null;
let generation = 0;
let timer = null;
let controller = null;
let refreshing = false;
let lastSuccess = null;

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
}

function actionLink(job, className, text) {
  const link = element("a", className, text);
  // Build links from validated account/repository components, never API-supplied URLs.
  const parts = String(job.repo).split("/");
  if (parts.length === 2) link.href = `https://github.com/${parts.map(encodeURIComponent).join("/")}/actions`;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  return link;
}

function notice(id, message) {
  $(id).textContent = message;
  $(id).hidden = !message;
}

function empty(title, detail, icon = "⌘") {
  const node = element("div", "empty");
  const glyph = element("span", "empty-icon", icon);
  glyph.setAttribute("aria-hidden", "true");
  node.append(glyph, element("h3", "", title), element("p", "", detail));
  return node;
}

function render(data) {
  for (const key of ["available", "busy", "offline", "queued"]) $(key).textContent = data.summary[key];
  $("runner-count").textContent = data.runners.length;
  $("job-count").textContent = data.jobs.length;
  $("scope").textContent = `${data.username} / ${data.repositories} ${data.repositories === 1 ? "repository" : "repositories"} · ${data.organizations} ${data.organizations === 1 ? "organization" : "organizations"} · ${data.summary.running} running ${data.summary.running === 1 ? "job" : "jobs"}`;
  const runnerCards = data.runners.map((runner) => {
    const card = element("article", "runner");
    const heading = element("div", "runner-top");
    const name = element("div");
    name.append(element("h3", "", runner.name), element("div", "repo-name", runner.scope === "organization" ? `${runner.organization} · Organization` : runner.repo));
    heading.append(name, element("span", `badge ${runner.state}`, runner.state === "available" ? "Available" : runner.state === "busy" ? "Busy" : "Offline"));
    const tags = element("div", "tags");
    tags.append(...runner.labels.map((label) => element("span", "tag", label)));
    const work = element("div", "runner-work");
    const active = data.jobs.filter((job) => job.assigned_runner_key === runner.key && job.status === "in_progress");
    const waiting = data.jobs.filter((job) => job.status !== "in_progress" && job.matching_runner_keys.includes(runner.key));
    work.append(element("span", "", runner.state === "offline" ? "Runner is offline" : runner.state === "busy" ? (active.length ? "Currently running" : "Busy · job details not available") : "Ready for a job"));
    for (const job of active) work.append(actionLink(job, "", `↗ ${job.workflow || "Workflow"} / ${job.name || "Job"}`));
    if (waiting.length) {
      work.append(element("p", "", `${waiting.length} queued ${waiting.length === 1 ? "job matches" : "jobs match"} labels · not assigned`));
      for (const job of waiting) work.append(actionLink(job, "", `↗ ${job.name || "Job"} · ${job.repo}`));
    }
    card.append(heading, tags, work);
    return card;
  });
  $("runners").replaceChildren(...(runnerCards.length ? runnerCards : [empty("No runners found", "No repository or organization self-hosted runners were returned. Check any access warnings above.")]));
  const jobs = data.jobs.map((job) => {
    const running = job.status === "in_progress";
    const card = actionLink(job, `job-card${running ? " running" : ""}`);
    const top = element("div", "job-top");
    top.append(element("span", "", running ? "● RUNNING" : `◷ ${String(job.status).toUpperCase()}`), element("span", "", "↗"));
    card.append(top, element("h3", "", job.name || "Job"), element("p", "job-meta", `${job.repo} · ${job.workflow || "Workflow"}${job.run_number ? ` #${job.run_number}` : ""}${job.branch ? ` · ${job.branch}` : ""}`));
    const names = data.runners.filter((r) => job.matching_runner_keys.includes(r.key)).map((r) => `${r.name} (${r.state})`);
    const match = running ? `Assigned: ${job.runner_name || data.runners.find((r) => r.key === job.assigned_runner_key)?.name || "runner details unavailable"}` : names.length ? `Label matches: ${names.join(", ")}` : "No matching runner found · check runner labels and access";
    card.append(element("p", "job-match", match));
    card.setAttribute("aria-label", `${job.name || "Job"}, ${job.status}, ${job.repo}. Open repository Actions in a new tab.`);
    return card;
  });
  const list = element("div", "jobs-list");
  list.append(...jobs);
  $("jobs").replaceChildren(jobs.length ? list : empty("All clear", "No active self-hosted jobs were returned. Check any access warnings above.", "✓"));
  notice("warnings", data.warnings.length ? `Incomplete snapshot — counts cover returned data only.\n${data.warnings.join("\n")}` : "");
}

async function refresh() {
  if (!session || refreshing) return;
  clearTimeout(timer);
  refreshing = true;
  const started = Date.now();
  const current = generation;
  controller = new AbortController();
  const requestController = controller;
  const timeout = setTimeout(() => requestController.abort(), 120000);
  $("refresh").disabled = true;
  $("refresh-label").textContent = "Refreshing…";
  let delay = 5000;
  let followCadence = false;
  try {
    const response = await fetch("/api/snapshot", {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Monitor-Request": "1" },
      body: JSON.stringify(session),
      signal: requestController.signal,
      cache: "no-store",
      credentials: "omit",
    });
    const data = await response.json();
    if (current !== generation) return;
    if (!response.ok) throw new Error(data.error || "Could not refresh from GitHub.");
    render(data);
    lastSuccess = new Date();
    $("last-updated").textContent = `Last updated ${lastSuccess.toLocaleTimeString()}`;
    const limited = data.warnings.some((warning) => /rate limit/i.test(warning));
    if (limited) delay = 60000;
    followCadence = !limited;
    $("refresh-label").textContent = limited ? "Rate limited · retry in 60s" : data.warnings.length ? "Partial data · see warnings" : "Live · updating every 5s";
    $("live-dot").className = `dot ${data.warnings.length ? "amber" : "green"}`;
    notice("error", "");
  } catch (error) {
    if (current !== generation) return;
    const message = error.name === "AbortError" ? "Refresh timed out. Select fewer repositories or check your connection." : error.message;
    const limited = /rate limit/i.test(message);
    if (limited) delay = 60000;
    notice("error", `${message}${lastSuccess ? " Displaying the last successful snapshot; it may be out of date." : " No live data has been loaded."}`);
    $("refresh-label").textContent = limited ? "Rate limited · retry in 60s" : "Refresh failed · retrying in 5s";
    $("live-dot").className = "dot amber";
  } finally {
    clearTimeout(timeout);
    if (current === generation) {
      refreshing = false;
      controller = null;
      $("refresh").disabled = false;
      if (session) timer = setTimeout(refresh, followCadence ? Math.max(0, delay - (Date.now() - started)) : delay);
    }
  }
}

$("connect-form").addEventListener("submit", (event) => {
  event.preventDefault();
  if (session) return;
  session = { username: $("username").value.trim(), token: $("token").value.trim(), repos: $("repos").value.trim() };
  generation++;
  lastSuccess = null;
  $("token").value = "";
  for (const id of ["username", "token", "repos", "connect"]) $(id).disabled = true;
  $("connect").textContent = "Monitoring account";
  $("controls").hidden = false;
  refresh();
});

$("disconnect").addEventListener("click", () => {
  generation++;
  clearTimeout(timer);
  controller?.abort();
  controller = null;
  refreshing = false;
  session = null;
  lastSuccess = null;
  for (const id of ["username", "token", "repos"]) { $(id).value = ""; $(id).disabled = false; }
  $("connect").disabled = false;
  $("connect").textContent = "Connect account ↗";
  $("controls").hidden = true;
  $("refresh-label").textContent = "Not connected";
  $("live-dot").className = "dot muted";
  $("last-updated").textContent = "Disconnected · credentials cleared";
  $("scope").textContent = "Connect to discover runners across your personal repositories and organizations.";
  for (const key of ["available", "busy", "offline", "queued"]) $(key).textContent = "—";
  $("runner-count").textContent = "0";
  $("job-count").textContent = "0";
  $("runners").replaceChildren(empty("A home for your runners", "Connect your account to see your self-hosted runners and their current workload."));
  $("jobs").replaceChildren(empty("Nothing to show yet", "Your running and queued self-hosted jobs will appear here.", "≡"));
  notice("error", "");
  notice("warnings", "");
  $("username").focus();
});

$("refresh").addEventListener("click", refresh);
$("help-toggle").addEventListener("click", () => {
  $("help").hidden = !$("help").hidden;
  $("help-toggle").setAttribute("aria-expanded", String(!$("help").hidden));
});
