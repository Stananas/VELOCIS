#!/usr/bin/env node
/**
 * Minimal CDP client for UI verification.
 *
 * REA deliberately refuses to evaluate page code, which is the right default for
 * third-party targets. This is our own page in our own browser profile, and
 * reloading it and reading the DOM is exactly what DevTools does, so a tiny
 * purpose-built client is the honest tool here.
 *
 *   node tools/cdp.mjs list
 *   node tools/cdp.mjs reload <url-substring>
 *   node tools/cdp.mjs shot <url-substring> <out.png>
 *   node tools/cdp.mjs eval <url-substring> '<js expression>'
 *   node tools/cdp.mjs click <url-substring> '<css selector>'
 *   node tools/cdp.mjs type <url-substring> '<css selector>' '<text>'
 *   node tools/cdp.mjs drag <url-substring> '<fromSelector>' <dx> <dy>
 *   node tools/cdp.mjs console <url-substring>
 */
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

// The debugging endpoint may be bound to IPv4 or IPv6 depending on what else
// already holds the port, so both are probed once.
const CANDIDATES = ["http://127.0.0.1:9222", "http://[::1]:9222"];
let ENDPOINT = process.env.CDP_ENDPOINT ?? null;

async function endpoint() {
  if (ENDPOINT) return ENDPOINT;
  for (const candidate of CANDIDATES) {
    try {
      const res = await fetch(`${candidate}/json/version`, { signal: AbortSignal.timeout(1500) });
      if (res.ok) {
        ENDPOINT = candidate;
        return candidate;
      }
    } catch {
      /* try the next one */
    }
  }
  throw new Error(`aucun point de débogage sur ${CANDIDATES.join(" ni ")}`);
}

async function listTargets() {
  const base = await endpoint();
  const res = await fetch(`${base}/json/list`);
  return res.json();
}

async function targetFor(substring) {
  const targets = await listTargets();
  const pages = targets.filter((t) => t.type === "page");
  const hit = pages.find((t) => t.url.includes(substring));
  if (!hit) {
    console.error("pages disponibles :");
    for (const p of pages) console.error(" -", p.url);
    throw new Error(`aucune page ne contient « ${substring} »`);
  }
  return hit;
}

/** One command per call: simpler than multiplexing, and fast enough locally. */
async function send(method, params = {}, substring = "") {
  const target = await targetFor(substring);
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  let id = 0;
  const pending = new Map();
  const events = [];
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    } else if (msg.method) {
      events.push(msg);
    }
  });
  const call = (m, p = {}) =>
    new Promise((resolve) => {
      const callId = ++id;
      pending.set(callId, resolve);
      ws.send(JSON.stringify({ id: callId, method: m, params: p }));
    });
  await new Promise((resolve) => ws.addEventListener("open", resolve));

  const result = { call, events, close: () => ws.close() };
  return result;
}

async function evaluate(substring, expression) {
  const c = await send("Runtime.evaluate", {}, substring);
  await c.call("Runtime.enable");
  const res = await c.call("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  c.close();
  if (res.result?.exceptionDetails) {
    throw new Error(res.result.exceptionDetails.text + " " + (res.result.exceptionDetails.exception?.description ?? ""));
  }
  return res.result?.result?.value;
}

const [cmd, ...args] = process.argv.slice(2);

try {
  if (cmd === "list") {
    for (const t of await listTargets()) {
      if (t.type === "page") console.log(t.url, "|", t.title.slice(0, 50));
    }
  } else if (cmd === "reload") {
    const c = await send("Page.reload", { ignoreCache: true }, args[0]);
    await c.call("Page.enable");
    await c.call("Page.reload", { ignoreCache: true });
    c.close();
    console.log("rechargé");
  } else if (cmd === "eval") {
    console.log(JSON.stringify(await evaluate(args[0], args[1]), null, 1));
  } else if (cmd === "shot") {
    const c = await send("Page.captureScreenshot", {}, args[0]);
    const res = await c.call("Page.captureScreenshot", { format: "png" });
    c.close();
    writeFileSync(args[1], Buffer.from(res.result.data, "base64"));
    console.log(`capture -> ${args[1]}`);
  } else if (cmd === "console") {
    const c = await send("Log.enable", {}, args[0]);
    await c.call("Log.enable");
    await c.call("Runtime.enable");
    await new Promise((r) => setTimeout(r, 600));
    for (const e of c.events) {
      if (e.method === "Log.entryAdded" || e.method === "Runtime.exceptionThrown") {
        console.log(JSON.stringify(e.params).slice(0, 400));
      }
    }
    c.close();
  } else if (cmd === "click") {
    const out = await evaluate(
      args[0],
      `(() => { const el = document.querySelector(${JSON.stringify(args[1])});
        if (!el) return 'absent';
        el.click();
        return 'cliqué: ' + (el.textContent || el.id || el.className).slice(0, 40); })()`
    );
    console.log(out);
  } else if (cmd === "type") {
    const out = await evaluate(
      args[0],
      `(() => { const el = document.querySelector(${JSON.stringify(args[1])});
        if (!el) return 'absent';
        el.value = ${JSON.stringify(args[2])};
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
        return 'saisi'; })()`
    );
    console.log(out);
  } else if (cmd === "drag") {
    const out = await evaluate(
      args[0],
      `(async () => {
        const el = document.querySelector(${JSON.stringify(args[1])});
        if (!el) return 'absent';
        const r = el.getBoundingClientRect();
        const x = r.left + r.width / 2, y = r.top + r.height / 2;
        const dx = ${Number(args[2])}, dy = ${Number(args[3])};
        const opts = (cx, cy) => ({ bubbles: true, cancelable: true, clientX: cx, clientY: cy, pointerId: 1, isPrimary: true, buttons: 1 });
        el.dispatchEvent(new PointerEvent('pointerdown', opts(x, y)));
        for (let i = 1; i <= 6; i++) {
          el.dispatchEvent(new PointerEvent('pointermove', opts(x + (dx * i) / 6, y + (dy * i) / 6)));
          await new Promise((r) => setTimeout(r, 20));
        }
        el.dispatchEvent(new PointerEvent('pointerup', opts(x + dx, y + dy)));
        return 'glissé de ' + dx + ',' + dy;
      })()`
    );
    console.log(out);
  } else {
    console.error("commandes: list | reload | eval | click | type | drag | console");
    process.exit(2);
  }
} catch (err) {
  console.error("erreur:", err.message);
  process.exit(1);
}
