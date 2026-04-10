/**
 * iroh-http Tauri example.
 *
 * Open two windows (or two app instances) and:
 *   1. Both windows show their Node ID.
 *   2. One clicks "Start serving" — it handles incoming requests.
 *   3. The other pastes the server's Node ID, sets a path, and clicks "Fetch".
 */

import { createNode } from "@momics/iroh-http-tauri";

// ── Bootstrap ─────────────────────────────────────────────────────────────────

const node = await createNode();
const nodeId = node.publicKey.toString();

const nodeIdEl = document.querySelector<HTMLElement>("#node-id")!;
const copyBtn = document.querySelector<HTMLButtonElement>("#copy-btn")!;

nodeIdEl.textContent = nodeId;
copyBtn.disabled = false;
copyBtn.addEventListener("click", async () => {
  await navigator.clipboard.writeText(nodeId);
  const prev = copyBtn.textContent;
  copyBtn.textContent = "Copied!";
  setTimeout(() => (copyBtn.textContent = prev), 1500);
});

// ── Server ────────────────────────────────────────────────────────────────────

const serveBtn = document.querySelector<HTMLButtonElement>("#serve-btn")!;
const serverStatus = document.querySelector<HTMLElement>("#server-status")!;
const serverLog = document.querySelector<HTMLElement>("#server-log")!;

let serving = false;
serveBtn.addEventListener("click", () => {
  if (serving) return;
  serving = true;
  serveBtn.textContent = "Serving…";
  serveBtn.disabled = true;
  serverStatus.textContent = "Listening for incoming requests";
  serverStatus.className = "status ok";

  node.serve({}, async (req) => {
    const path = new URL(req.url).pathname;
    const line = `${new Date().toLocaleTimeString()}  ${req.method} ${path}  ← ${req.headers.get("x-iroh-node-id") ?? "unknown"}`;
    serverLog.textContent = serverLog.textContent
      ? serverLog.textContent + "\n" + line
      : line;
    return new Response(`Hello from iroh-http Tauri! (path: ${path})`, {
      headers: { "content-type": "text/plain" },
    });
  });
});

// ── Client ────────────────────────────────────────────────────────────────────

const fetchForm = document.querySelector<HTMLFormElement>("#fetch-form")!;
const peerInput = document.querySelector<HTMLInputElement>("#peer-input")!;
const pathInput = document.querySelector<HTMLInputElement>("#path-input")!;
const responseStatus = document.querySelector<HTMLElement>("#response-status")!;
const responseBody = document.querySelector<HTMLElement>("#response-body")!;

fetchForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  const peerId = peerInput.value.trim();
  const path = pathInput.value.trim() || "/";
  if (!peerId) {
    peerInput.focus();
    return;
  }
  responseStatus.textContent = "fetching…";
  responseStatus.className = "status";
  responseBody.textContent = "";
  try {
    const res = await node.fetch(peerId, path);
    responseStatus.textContent = `HTTP ${res.status}`;
    responseStatus.className = `status ${res.ok ? "ok" : "error"}`;
    responseBody.textContent = await res.text();
  } catch (err) {
    responseStatus.textContent = "error";
    responseStatus.className = "status error";
    responseBody.textContent = String(err);
  }
});
