/* ============================================================================
   Velocis — UI
   ----------------------------------------------------------------------------
   No framework and no bundler: the editor talks to a local service over JSON,
   and the state that matters (the project) lives there. This file is therefore
   a *renderer* plus a thin interaction layer — it never mutates the project
   itself, it asks the service to, and re-renders from the answer. That is what
   keeps undo/redo honest: there is exactly one writer.
   ========================================================================= */

import { LivePreview } from "./preview.js";
import { StageManipulator } from "./stage.js";

const API = "";

/* ------------------------------------------------------------------ helpers */

const $ = (id) => document.getElementById(id);
const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

const US = 1e6; // microseconds per second

/** `TimeRange` travels as `{start, duration}`, so its end is derived. */
function clipEnd(clip) {
  return clip.timeline.start + clip.timeline.duration;
}

function fmtTime(us, withFrames = false) {
  const total = Math.max(0, us) / US;
  const m = Math.floor(total / 60);
  const s = Math.floor(total % 60);
  const frac = total - Math.floor(total);
  if (withFrames) {
    const f = Math.floor(frac * 30);
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}:${String(f).padStart(2, "0")}`;
  }
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(Math.floor(frac * 100)).padStart(2, "0")}`;
}

function fmtDuration(us) {
  const total = Math.max(0, us) / US;
  if (total < 60) return `${total.toFixed(1).replace(".", ",")} s`;
  let minutes = Math.floor(total / 60);
  const seconds = Math.round(total % 60);
  // Rounding the seconds can reach 60: carry it instead of printing "3 min 60 s".
  if (seconds === 60) {
    minutes += 1;
    return `${minutes} min`;
  }
  return `${minutes} min ${seconds} s`;
}

/** Compact form for dense surfaces: 3:59 or 4 h 49, never "289:24". */
function fmtShort(us) {
  const total = Math.max(0, us) / US;
  if (total < 60) return `${total.toFixed(1).replace(".", ",")} s`;
  if (total < 3600) {
    const minutes = Math.floor(total / 60);
    const seconds = Math.floor(total % 60);
    return `${minutes}:${String(seconds).padStart(2, "0")}`;
  }
  const hours = Math.floor(total / 3600);
  const minutes = Math.round((total % 3600) / 60);
  return minutes === 60 ? `${hours + 1} h` : `${hours} h ${String(minutes).padStart(2, "0")}`;
}

function fmtBytes(n) {
  if (!n) return "0 o";
  const units = ["o", "Ko", "Mo", "Go"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1).replace(".", ",")} ${units[i]}`;
}

function clamp(v, lo, hi) {
  return Math.min(hi, Math.max(lo, v));
}

/** File-name slug that keeps accented letters meaningful instead of dropping them. */
function slugify(name) {
  const plain = String(name)
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return plain || "clip";
}

function hexToRgba(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return { r: 255, g: 255, b: 255, a: 255 };
  const v = parseInt(m[1], 16);
  return { r: (v >> 16) & 255, g: (v >> 8) & 255, b: v & 255, a: 255 };
}

function rgbaToHex(c) {
  const h = (n) => n.toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

const ICONS = {
  play: `<svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 2.5l9 5.5-9 5.5z"/></svg>`,
  pause: `<svg viewBox="0 0 16 16" fill="currentColor"><path d="M4 3h3v10H4zM9 3h3v10H9z"/></svg>`,
  undo: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M3 8h7a3 3 0 010 6H7M3 8l3-3M3 8l3 3"/></svg>`,
  redo: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M13 8H6a3 3 0 000 6h3M13 8l-3-3M13 8l-3 3"/></svg>`,
  sun: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="8" cy="8" r="3"/><path d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.5 1.5M11.5 11.5L13 13M13 3l-1.5 1.5M4.5 11.5L3 13"/></svg>`,
  moon: `<svg viewBox="0 0 16 16" fill="currentColor"><path d="M9 1a6.5 6.5 0 106 8.5A5.5 5.5 0 019 1z"/></svg>`,
  video: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="2" y="4" width="9" height="8" rx="1.5"/><path d="M11 7l3-2v6l-3-2z"/></svg>`,
  audio: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M6 3v10M10 5v6M2 7v2M14 6v4"/></svg>`,
  image: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M2 11l3-3 3 3 2-2 4 4"/></svg>`,
  trash: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M3 5h10M6 5V3h4v2M5 5l.7 8h4.6L11 5"/></svg>`,
  split: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M8 2v12M4 5l-2 3 2 3M12 5l2 3-2 3"/></svg>`,
  plus: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M8 3v10M3 8h10"/></svg>`,
  mute: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M3 6h2.5L8.5 3.5v9L5.5 10H3zM11 6l3 4M14 6l-3 4"/></svg>`,
  eye: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M1.5 8S4 4 8 4s6.5 4 6.5 4S12 12 8 12 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="1.6"/></svg>`,
  eyeOff: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><path d="M2 2l12 12M6 4.5A6.6 6.6 0 018 4c4 0 6.5 4 6.5 4a11 11 0 01-2.3 2.6M4.3 5.6A11 11 0 001.5 8s2.5 4 6.5 4c.8 0 1.5-.2 2.1-.4"/></svg>`,
  lock: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3.5" y="7" width="9" height="6" rx="1.2"/><path d="M5.8 7V5.2a2.2 2.2 0 014.4 0V7"/></svg>`,
  unlock: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3.5" y="7" width="9" height="6" rx="1.2"/><path d="M5.8 7V5.2a2.2 2.2 0 014.4 0"/></svg>`,
  panelLeft: `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2.5" y="3.5" width="15" height="13" rx="2.5"/><path d="M8 3.5v13"/></svg>`,
  panelRight: `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2.5" y="3.5" width="15" height="13" rx="2.5"/><path d="M12 3.5v13"/></svg>`,
};

/* -------------------------------------------------------------- effect model */

const EFFECTS = [
  { kind: "brightness", label: "Luminosité", params: [{ name: "amount", min: -1, max: 1, step: 0.01, def: 0.15 }] },
  { kind: "contrast", label: "Contraste", params: [{ name: "amount", min: -1, max: 1, step: 0.01, def: 0.2 }] },
  { kind: "saturation", label: "Saturation", params: [{ name: "amount", min: -1, max: 1, step: 0.01, def: 0.3 }] },
  { kind: "temperature", label: "Température", params: [{ name: "kelvin", min: 2000, max: 12000, step: 100, def: 8500 }] },
  { kind: "hue", label: "Teinte", params: [{ name: "degrees", min: -180, max: 180, step: 1, def: 30 }] },
  { kind: "gamma", label: "Gamma", params: [{ name: "amount", min: 0.2, max: 3, step: 0.05, def: 1.2 }] },
  { kind: "blur", label: "Flou", params: [{ name: "amount", min: 0, max: 1, step: 0.01, def: 0.2 }] },
  { kind: "sharpen", label: "Netteté", params: [{ name: "amount", min: 0, max: 2, step: 0.05, def: 0.6 }] },
  { kind: "grayscale", label: "Noir et blanc", params: [{ name: "amount", min: 0, max: 1, step: 0.01, def: 1 }] },
  { kind: "sepia", label: "Sépia", params: [{ name: "amount", min: 0, max: 1, step: 0.01, def: 0.7 }] },
  { kind: "invert", label: "Inverser", params: [{ name: "amount", min: 0, max: 1, step: 0.01, def: 1 }] },
  { kind: "vignette", label: "Vignette", params: [{ name: "amount", min: 0, max: 3.14, step: 0.02, def: 0.8 }] },
  {
    kind: "chroma_key",
    label: "Incrustation couleur",
    color: true,
    params: [
      { name: "similarity", min: 0, max: 1, step: 0.01, def: 0.3 },
      { name: "blend", min: 0, max: 1, step: 0.01, def: 0.1 },
    ],
  },
];

const effectSpec = (kind) => EFFECTS.find((e) => e.kind === kind);

/* -------------------------------------------------------------------- state */

const S = {
  snap: null,
  caps: null,
  selection: null,
  zoom: 64,
  playhead: 0,
  tab: "media",
  inspectorTab: "transform",
  theme: localStorage.getItem("velocis.theme") || "dark",
  panel: localStorage.getItem("velocis.panel") !== "closed",
  inspector: localStorage.getItem("velocis.inspector") !== "collapsed",
  playing: false,
  drag: null,
  job: null,
  busy: false,
};

let stage = null;
let manipulator = null;

const project = () => S.snap?.project;
const session = () => S.snap?.session;

function selectedClip() {
  const p = project();
  if (!p || !S.selection) return null;
  for (const track of p.tracks) {
    const clip = track.clips.find((c) => c.id === S.selection);
    if (clip) return { clip, track };
  }
  return null;
}

function assetOf(clip) {
  return project()?.assets.find((a) => a.id === clip.asset_id) || null;
}

/* ---------------------------------------------------------------------- api */

async function request(method, path, body, isRaw, filename) {
  const init = { method, headers: {} };
  if (body !== undefined && !isRaw) {
    init.headers["content-type"] = "application/json";
    init.body = JSON.stringify(body);
  } else if (isRaw) {
    // The bytes travel as the request body; the name rides in a header because
    // a browser cannot hand out the path of a file the user picked.
    init.body = body;
    if (filename) init.headers["x-filename"] = encodeURIComponent(filename);
  }
  const res = await fetch(API + path, init);
  const text = await res.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = { error: text };
    }
  }
  if (!res.ok) throw new Error(data?.error || `Erreur ${res.status}`);
  return data;
}

const api = {
  get: (p) => request("GET", p),
  post: (p, b) => request("POST", p, b ?? {}),
};

/* ---------------------------------------------------------------- panneaux */

/** The rail stays visible; only the content panel folds away. */
function applyPanelState() {
  const middle = $("middle");
  if (!middle) return;
  middle.dataset.sidebar = S.panel ? "open" : "collapsed";
  middle.dataset.inspector = S.inspector ? "open" : "collapsed";
  $("btn-panel").setAttribute("aria-pressed", String(S.panel));
  $("btn-inspector").setAttribute("aria-pressed", String(S.inspector));
  $("btn-panel").title = S.panel ? "Masquer le panneau des médias ( [ )" : "Afficher le panneau des médias ( [ )";
  $("btn-inspector").title = S.inspector ? "Masquer les propriétés ( ] )" : "Afficher les propriétés ( ] )";
}

function setPanel(open) {
  S.panel = open;
  localStorage.setItem("velocis.panel", open ? "open" : "closed");
  applyPanelState();
}

function setInspector(open) {
  S.inspector = open;
  localStorage.setItem("velocis.inspector", open ? "open" : "collapsed");
  applyPanelState();
}

function togglePanel() {
  setPanel(!S.panel);
}

function toggleInspector() {
  setInspector(!S.inspector);
}

/* ------------------------------------------------------------------- toasts */

function toast(message, kind = "info", action) {
  const host = $("toasts");
  const node = document.createElement("div");
  node.className = `toast ${kind}`;
  node.innerHTML = `<span class="dot"></span><span>${esc(message)}</span>`;
  if (action) {
    const btn = document.createElement("button");
    btn.textContent = action.label;
    btn.onclick = () => {
      action.run();
      node.remove();
    };
    node.appendChild(btn);
  }
  host.appendChild(node);
  const life = action ? 9000 : 3600;
  setTimeout(() => {
    node.style.transition = "opacity 200ms";
    node.style.opacity = "0";
    setTimeout(() => node.remove(), 220);
  }, life);
}

/** Wrap a mutation so failures surface as an actionable message, never a dead UI. */
async function act(fn, okMessage) {
  if (S.busy) return;
  S.busy = true;
  try {
    const result = await fn();
    if (okMessage) toast(okMessage, "success");
    return result;
  } catch (err) {
    toast(err.message, "error", {
      label: "Réessayer",
      run: () => act(fn, okMessage),
    });
    return null;
  } finally {
    S.busy = false;
  }
}

/* ------------------------------------------------------------------ mutations */

async function applySnapshot(snap) {
  if (!snap) return;
  S.snap = snap;
  render();
}

async function refresh() {
  await applySnapshot(await api.get("/api/state"));
}

const mutate = (path, body) => act(async () => applySnapshot(await api.post(path, body)));

async function importFiles(files) {
  let last = null;
  for (const file of files) {
    const result = await act(async () => {
      const buf = await file.arrayBuffer();
      return request("POST", "/api/upload", buf, true, file.name);
    });
    if (result?.imported?.length) {
      last = result.imported[0];
      // Preparation runs in the background; following it here is what turns the
      // first slow scrub into an instant one.
      followPreparation(result.imported[0], result.prepare_jobs);
    } else if (result?.failed?.length) toast(result.failed[0].error, "error");
  }
  await refresh();
  if (files.length === 1) {
    toast(`« ${files[0].name} » importé`, "success", last ? { label: "Ajouter à la timeline", run: addToTimeline } : undefined);
  } else {
    toast(`${files.length} médias importés`, "success");
  }
}

/** Add the most recently imported asset to the end of the first compatible track. */
function addToTimeline(assetId) {
  const p = project();
  if (!p) return;
  const asset = assetId ? p.assets.find((a) => a.id === assetId) : p.assets[p.assets.length - 1];
  if (!asset) return;
  const wanted = asset.kind === "audio" ? "audio" : "video";
  const track = p.tracks.find((t) => t.kind === wanted);
  if (!track) {
    toast("Aucune piste compatible : ajoutez-en une", "error");
    return;
  }
  mutate("/api/clip/append", { track_id: track.id, asset_id: asset.id });
}

/* ------------------------------------------------------------------ rendering */

function render() {
  renderHeader();
  renderSidebar();
  renderStage();
  renderInspector();
  renderTimeline();
}

function renderHeader() {
  const p = project();
  const s = session();
  if (!p) return;
  const nameInput = $("project-name");
  if (document.activeElement !== nameInput) nameInput.value = p.name;

  const undo = $("btn-undo");
  undo.disabled = !s.can_undo;
  undo.title = s.undo_label ? `Annuler : ${s.undo_label}` : "Annuler";
  const redo = $("btn-redo");
  redo.disabled = !s.can_redo;
  redo.title = s.redo_label ? `Rétablir : ${s.redo_label}` : "Rétablir";
  undo.innerHTML = ICONS.undo;
  redo.innerHTML = ICONS.redo;
  $("btn-theme").innerHTML = S.theme === "dark" ? ICONS.sun : ICONS.moon;
  $("btn-panel").innerHTML = ICONS.panelLeft;
  $("btn-inspector").innerHTML = ICONS.panelRight;
  applyPanelState();

  // Aspect presets, with the reason each one exists — the same "say why" habit
  // that makes a preset list readable instead of cryptic.
  const aspects = [
    ["landscape16x9", "16:9", "YouTube, écran"],
    ["portrait9x16", "9:16", "Reels, TikTok"],
    ["square1x1", "1:1", "Fil d'actualité"],
    ["portrait4x5", "4:5", "Post Instagram"],
  ];
  const current = p.canvas.width / p.canvas.height;
  const match = aspects.find(([key]) => {
    const dims = { landscape16x9: 16 / 9, portrait9x16: 9 / 16, square1x1: 1, portrait4x5: 4 / 5 };
    return Math.abs(dims[key] - current) < 0.02;
  });
  $("aspect-switch").innerHTML = aspects
    .map(
      ([key, label, why]) =>
        `<button class="toggle" data-aspect="${key}" aria-pressed="${match?.[0] === key}" title="${esc(why)}">${label}</button>`
    )
    .join("");
}

function renderSidebar() {
  const p = project();
  if (!p) return;
  // Scoped to the rail: the inspector's own tabs use a different data attribute,
  // and a broad selector used to reset them on every render.
  document.querySelectorAll(".rail-btn[data-tab]").forEach((t) => {
    t.setAttribute("aria-selected", String(t.dataset.tab === S.tab));
  });
  const titles = { media: "Média", text: "Texte", effects: "Effets" };
  $("panel-title").textContent = titles[S.tab] ?? "Média";
  document.querySelectorAll(".panel[data-panel]").forEach((panel) => {
    panel.hidden = panel.dataset.panel !== S.tab;
  });

  $("media-grid").innerHTML = p.assets
    .map((a) => {
      const icon = a.kind === "audio" ? ICONS.audio : a.kind === "image" ? ICONS.image : ICONS.video;
      const thumb = a.thumbnail_path ? `style="background-image:url('${esc(fileUrl(a.thumbnail_path))}')"` : "";
      const dims = a.width ? `${a.width}×${a.height}` : a.audio_channels ? `${a.audio_channels} canaux` : "";
      const details = `${a.name} — ${dims} ${a.video_codec || a.audio_codec || ""}`.trim();
      return `<div class="media-card" draggable="true" data-asset="${esc(a.id)}" title="${esc(details)}">
        <div class="thumb" ${thumb}>${a.thumbnail_path ? "" : icon}
          <button class="add" data-add-asset="${esc(a.id)}" title="Ajouter à la fin de la timeline" aria-label="Ajouter ${esc(a.name)} à la timeline">
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M8 3v10M3 8h10"/></svg>
          </button>
        </div>
        <div class="meta"><b>${esc(a.name)}</b><span>${fmtShort(a.duration)}</span></div>
      </div>`;
    })
    .join("");
  $("media-hint").hidden = p.assets.length > 0;

  renderEffectsCatalog();
}

/* Preview proxies: a source the browser cannot decode is prepared slice by
   slice, so a four-hour Matroska file stays editable without transcoding it all. */
const proxyJobs = new Map();
let proxyPending = 0;

function setLiveLabel(label) {
  const badge = document.querySelector(".live-badge");
  if (badge) badge.innerHTML = `<span class="dot"></span> ${esc(label)}`;
}

async function ensureProxy(assetId, index) {
  const key = `${assetId}:${index}`;
  if (proxyJobs.has(key)) return proxyJobs.get(key);
  proxyPending += 1;
  setLiveLabel("Préparation de l'aperçu…");
  const promise = (async () => {
    const job = await api.post("/api/proxy", { asset_id: assetId, index });
    const result = await pollJob(job.id);
    if (result?.status === "done" && result.output) return result.output;
    throw new Error(result?.message || "segment d'aperçu indisponible");
  })();
  proxyJobs.set(key, promise);
  promise
    .catch((error) => {
      proxyJobs.delete(key);
      toast(error.message, "error");
    })
    .finally(() => {
      proxyPending = Math.max(0, proxyPending - 1);
      if (proxyPending === 0) setLiveLabel("Composition en direct");
    });
  return promise;
}

/** A toast that stays up while work is in progress, then disappears. */
function progressToast(initial) {
  const host = $("toasts");
  const node = document.createElement("div");
  node.className = "toast";
  node.innerHTML = `<span class="dot"></span><span class="msg"></span>`;
  const label = node.querySelector(".msg");
  label.textContent = initial;
  host.appendChild(node);
  return {
    update(text) {
      label.textContent = text;
    },
    close() {
      node.remove();
    },
  };
}

/** Follow the preparation jobs started by an import, and refresh when they land. */
async function followPreparation(asset, jobIds) {
  if (!jobIds?.length) return;
  const toast = progressToast(`Préparation de « ${asset.name} »…`);
  let changed = false;
  for (const id of jobIds) {
    const result = await pollJob(id, (progress) => {
      toast.update(`Préparation de « ${asset.name} »… ${Math.round(progress * 100)} %`);
    });
    if (result?.status === "done") changed = true;
    else if (result) toast.update(result.message);
  }
  toast.close();
  if (changed) await refresh();
}

/** The service writes thumbnails inside its media directory. */
function fileUrl(path) {
  const name = String(path).split("/").pop();
  return `/media/${encodeURIComponent(name)}`;
}

function renderEffectsCatalog() {
  const host = $("effects-catalog");
  const selection = selectedClip();
  const intro = $("effects-intro");
  if (!selection) {
    intro.hidden = false;
    host.innerHTML = "";
    return;
  }
  intro.hidden = true;
  const applied = new Set(selection.clip.effects.map((e) => e.kind));
  host.innerHTML = EFFECTS.map(
    (spec) => `<button class="chip" data-add-effect="${spec.kind}" ${applied.has(spec.kind) ? "disabled" : ""}>
      ${applied.has(spec.kind) ? "✓ " : "+ "}${esc(spec.label)}</button>`
  ).join("");
}

/** The Velocis mark, inlined so it needs no asset request. */
function markSvg(size, id) {
  return `<svg class="mark" viewBox="0 0 32 32" fill="none" aria-hidden="true" width="${size}" height="${size}">
    <defs>
      <linearGradient id="velocis-mark-${id}" x1="2" y1="0" x2="30" y2="32" gradientUnits="userSpaceOnUse">
        <stop offset="0" stop-color="#7C5CFF" />
        <stop offset="1" stop-color="#C084FC" />
      </linearGradient>
    </defs>
    <rect width="32" height="32" rx="9" fill="url(#velocis-mark-${id})" />
    <g transform="skewX(-7)">
      <path d="M10 9.5 L16.6 22.5 L23.4 9.5" stroke="#fff" stroke-width="3.1" stroke-linecap="round" stroke-linejoin="round" />
    </g>
  </svg>`;
}

/** What the stage shows before there is anything to watch. */
function welcomeCard() {
  return `<div class="welcome">
    ${markSvg(60, "welcome")}
    <h1>Votre vidéo commence ici</h1>
    <p>
      Glissez vos fichiers sur la fenêtre, ou importez-les : Velocis analyse chaque média,
      compose les pistes et encode avec le GPU de la machine.
    </p>
    <div class="actions">
      <button class="btn primary" data-action="import">Importer une vidéo</button>
      <button class="btn" data-action="text">Ajouter un texte</button>
    </div>
    <div class="shortcuts">
      <span><kbd>Espace</kbd> lecture</span>
      <span><kbd>S</kbd> découper</span>
      <span><kbd>Suppr</kbd> supprimer</span>
      <span><kbd>Ctrl</kbd> <kbd>Z</kbd> annuler</span>
    </div>
  </div>`;
}

function renderStage() {
  const p = project();
  if (!p) return;
  const host = $("stage-empty");
  const hasContent = p.tracks.some((t) => t.clips.length > 0);
  host.hidden = hasContent;
  if (!hasContent) host.innerHTML = welcomeCard();
  if (stage) {
    stage.setProject(p);
    updateTransport();
  }
  manipulator?.refresh();
}

/** Transport bar and playhead, driven by the composition clock. */
function updateTransport() {
  const total = stage?.duration() ?? 0;
  $("tc-total").textContent = fmtTime(total);
  $("tc-current").textContent = fmtTime(S.playhead);
  const ratio = total > 0 ? clamp(S.playhead / total, 0, 1) : 0;
  $("scrub-fill").style.width = `${ratio * 100}%`;
  $("scrub-thumb").style.left = `${ratio * 100}%`;
  $("btn-play").innerHTML = S.playing ? ICONS.pause : ICONS.play;
  const playhead = $("playhead");
  if (playhead) playhead.style.left = `${(S.playhead / US) * S.zoom}px`;
  if (S.playing) followPlayhead();
}

/** Keep the playhead in view while playing, without fighting a manual scroll. */
function followPlayhead() {
  const wrap = document.querySelector(".ruler-wrap");
  if (!wrap) return;
  const x = (S.playhead / US) * S.zoom;
  const left = wrap.scrollLeft;
  const right = left + wrap.clientWidth;
  if (x < left + 40) wrap.scrollLeft = Math.max(0, x - 40);
  else if (x > right - 80) wrap.scrollLeft = x - wrap.clientWidth + 80;
}

function renderInspector() {
  const selection = selectedClip();
  const tabsHost = $("inspector-tabs");
  const body = $("inspector-body");

  if (!selection) {
    // With nothing selected there are no tabs, but the header must still say what
    // the panel is.
    tabsHost.innerHTML = `<span class="panel-title">Propriétés</span>`;
    body.innerHTML = `<div class="empty-state">
      <strong>Rien de sélectionné</strong>
      Cliquez un clip sur la timeline pour régler sa position, son volume,
      ses fondus ou ses effets.
    </div>`;
    return;
  }

  const { clip, track } = selection;
  manipulator?.refresh();
  const isText = Boolean(clip.text);
  const isAudioTrack = track.kind === "audio";
  const tabs = [];
  if (isText) tabs.push(["text", "Texte"]);
  if (!isAudioTrack) tabs.push(["transform", "Transform"]);
  tabs.push(["effects", "Effets"]);
  tabs.push(["audio", "Audio"]);
  if (!S.inspectorTab || !tabs.some(([k]) => k === S.inspectorTab)) S.inspectorTab = tabs[0][0];

  tabsHost.innerHTML = tabs
    .map(
      ([key, label]) =>
        `<button class="tab" role="tab" data-inspector-tab="${key}" aria-selected="${S.inspectorTab === key}">${label}</button>`
    )
    .join("");

  const t = clip.transform;
  const canvas = project().canvas;

  if (S.inspectorTab === "text") {
    body.innerHTML = `
      <div class="field">
        <label for="f-text">Texte</label>
        <textarea id="f-text" data-clip-text>${esc(clip.text.content)}</textarea>
      </div>
      <div class="field">
        <label for="f-size">Taille <b id="f-size-v">${clip.text.font_size}</b></label>
        <input type="range" id="f-size" data-text-field="font_size" min="16" max="220" step="2" value="${clip.text.font_size}" />
      </div>
      <div class="field">
        <label>Style</label>
        <div class="toggle-row">
          <button class="toggle" data-text-toggle="bold" aria-pressed="${clip.text.bold}">Gras</button>
          <button class="toggle" data-text-toggle="italic" aria-pressed="${clip.text.italic}">Italique</button>
        </div>
      </div>
      <div class="field">
        <label>Fond</label>
        <div class="toggle-row">
          <button class="toggle" data-text-background aria-pressed="${Boolean(clip.text.background)}">
            ${clip.text.background ? "Fond activé" : "Sans fond"}
          </button>
        </div>
        <p class="hint why">Un fond semi-transparent rend le texte lisible sur n'importe quelle image.</p>
      </div>
      <div class="field">
        <label for="f-color">Couleur</label>
        <input type="text" id="f-color" data-text-color value="${rgbaToHex(clip.text.color)}" />
      </div>
      ${commonTimingFields(clip)}
    `;
    return;
  }

  if (S.inspectorTab === "transform") {
    const maxX = canvas.width;
    const maxY = canvas.height;
    body.innerHTML = `
      <div class="field">
        <label>Position horizontale <b id="f-x-v">${Math.round(t.x)} px</b></label>
        <input type="range" id="f-x" data-transform="x" min="${-maxX}" max="${maxX}" step="2" value="${t.x}" />
      </div>
      <div class="field">
        <label>Position verticale <b id="f-y-v">${Math.round(t.y)} px</b></label>
        <input type="range" id="f-y" data-transform="y" min="${-maxY}" max="${maxY}" step="2" value="${t.y}" />
      </div>
      <div class="field">
        <label>Échelle <b id="f-scale-v">${Math.round(t.scale * 100)} %</b></label>
        <input type="range" id="f-scale" data-transform="scale" min="0.05" max="3" step="0.01" value="${t.scale}" />
      </div>
      <div class="field">
        <label>Rotation <b id="f-rot-v">${Math.round(t.rotation_deg)}°</b></label>
        <input type="range" id="f-rot" data-transform="rotation_deg" min="-180" max="180" step="1" value="${t.rotation_deg}" />
      </div>
      <div class="field">
        <label>Miroir</label>
        <div class="toggle-row">
          <button class="toggle" data-transform-toggle="mirror_x" aria-pressed="${t.mirror_x}">Horizontal</button>
          <button class="toggle" data-transform-toggle="mirror_y" aria-pressed="${t.mirror_y}">Vertical</button>
          <button class="toggle" data-transform-reset>Recentrer</button>
        </div>
      </div>
      <div class="field">
        <label>Cadrage</label>
        <div class="toggle-row">
          <button class="toggle" data-crop-toggle aria-pressed="${Boolean(manipulator?.cropMode)}">Recadrer</button>
          <button class="toggle" data-crop-reset>Réinitialiser</button>
        </div>
        <p class="hint why">En mode recadrage, tirez les bords du cadre dans l'aperçu.</p>
      </div>
      <div class="field">
        <label>Opacité <b id="f-opacity-v">${Math.round(clip.opacity * 100)} %</b></label>
        <input type="range" id="f-opacity" data-clip-field="opacity" min="0" max="1" step="0.01" value="${clip.opacity}" />
      </div>
      ${commonTimingFields(clip)}
    `;
    return;
  }

  if (S.inspectorTab === "effects") {
    const applied = clip.effects
      .map((effect, index) => {
        const spec = effectSpec(effect.kind);
        if (!spec) return "";
        const controls = spec.params
          .map((param) => {
            const value = effect[param.name] ?? param.def;
            return `<div class="field">
              <label>${esc(param.name)} <b>${Number(value).toFixed(param.step < 1 ? 2 : 0)}</b></label>
              <input type="range" data-effect="${index}" data-effect-param="${param.name}"
                min="${param.min}" max="${param.max}" step="${param.step}" value="${value}" />
            </div>`;
          })
          .join("");
        const color = spec.color
          ? `<div class="field"><label>Couleur à retirer</label>
             <input type="text" data-effect="${index}" data-effect-color value="${rgbaToHex(effect.color)}" /></div>`
          : "";
        return `<div class="effect-row"><span class="name">${esc(spec.label)}</span>
          <button class="btn small" data-remove-effect="${index}">Retirer</button></div>${controls}${color}`;
      })
      .join("");
    body.innerHTML = applied || `<div class="empty-state">
      <strong>Aucun effet</strong>Ouvrez l'onglet « Effets » de la bibliothèque pour en ajouter un.
    </div>`;
    return;
  }

  // audio
  body.innerHTML = `
    <div class="field">
      <label>Volume <b id="f-vol-v">${Math.round(clip.volume * 100)} %</b></label>
      <input type="range" id="f-vol" data-clip-field="volume" min="0" max="2" step="0.01" value="${clip.volume}" />
    </div>
    ${commonTimingFields(clip)}
  `;
}

function commonTimingFields(clip) {
  const speed = clip.text
    ? `<p class="hint why">La vitesse ne s'applique pas à un texte.</p>`
    : `<div class="field">
      <label>Vitesse <b id="f-speed-v">${clip.speed.toFixed(2).replace(".", ",")}×</b></label>
      <input type="range" id="f-speed" data-clip-field="speed" min="0.25" max="4" step="0.05" value="${clip.speed}" />
      <p class="hint why">Ralentir allonge le clip ; accélérer le raccourcit.</p>
    </div>`;
  return `
    <div class="field">
      <label>Fondu d'entrée <b id="f-fi-v">${fmtDuration(clip.fade_in)}</b></label>
      <input type="range" id="f-fi" data-clip-field="fade_in" min="0" max="3000000" step="50000" value="${clip.fade_in}" />
    </div>
    <div class="field">
      <label>Fondu de sortie <b id="f-fo-v">${fmtDuration(clip.fade_out)}</b></label>
      <input type="range" id="f-fo" data-clip-field="fade_out" min="0" max="3000000" step="50000" value="${clip.fade_out}" />
    </div>
    ${speed}
    <div class="field">
      <label>Durée <b>${fmtDuration(clip.timeline.duration)}</b></label>
      <p class="hint why">Modifiable en tirant les poignées du clip sur la timeline.</p>
    </div>
  `;
}

/* ----------------------------------------------------------------- timeline */

function timelineTotal() {
  const duration = session()?.duration || 0;
  // Always leave some empty room after the last clip, so there is somewhere to
  // drop new media.
  return Math.max(duration + 3 * US, 8 * US);
}

function renderTimeline() {
  const p = project();
  if (!p) return;
  const pxPerSec = S.zoom;
  const total = timelineTotal();
  const width = Math.max((total / US) * pxPerSec, 400);

  $("track-heads").innerHTML = p.tracks
    .map((track) => {
      const isAudio = track.kind === "audio";
      return `<div class="track-head ${isAudio ? "audio" : ""}" data-track-head="${esc(track.id)}">
        <div class="name">${isAudio ? ICONS.audio : ICONS.video}<span>${esc(track.name)}</span></div>
        <div class="actions">
          <button data-track-toggle="muted" data-track="${esc(track.id)}" aria-pressed="${track.muted}"
            title="${track.muted ? "Rétablir le son" : "Couper le son de la piste"}" aria-label="Couper le son">${ICONS.mute}</button>
          <button data-track-toggle="hidden" data-track="${esc(track.id)}" aria-pressed="${track.hidden}"
            title="${track.hidden ? "Afficher dans l'aperçu" : "Masquer de l'aperçu"}" aria-label="Masquer">${track.hidden ? ICONS.eyeOff : ICONS.eye}</button>
          <button data-track-toggle="locked" data-track="${esc(track.id)}" aria-pressed="${track.locked}"
            title="${track.locked ? "Déverrouiller" : "Verrouiller la piste"}" aria-label="Verrouiller">${track.locked ? ICONS.lock : ICONS.unlock}</button>
          <button data-track-remove="${esc(track.id)}" title="Supprimer la piste" aria-label="Supprimer la piste">${ICONS.trash}</button>
        </div>
      </div>`;
    })
    .join("");

  const lanes = p.tracks
    .map((track) => {
      const isAudio = track.kind === "audio";
      const clips = track.clips
        .map((clip) => renderClip(clip, track, pxPerSec))
        .join("");
          const gaps = renderGaps(track, pxPerSec);
      const hint = track.clips.length
        ? ""
        : `<div class="lane-hint">${isAudio ? "Déposez un son ici" : "Déposez un média ici"}</div>`;
      return `<div class="track-lane ${isAudio ? "audio" : ""}" data-lane="${esc(track.id)}" data-hidden="${track.hidden}">${hint}${clips}${gaps}</div>`;
    })
    .join("");

  const content = `<div class="tl-content" id="tl-content" style="width:${width}px">
    <div class="ruler" id="ruler">${renderRuler(total, pxPerSec)}</div>
    <div class="tracks" id="tracks">${lanes}</div>
    <div class="playhead" id="playhead" style="left:${(S.playhead / US) * pxPerSec}px"></div>
    <div class="snap-line" id="snap-line" hidden></div>
  </div>`;

  const wrap = document.querySelector(".ruler-wrap");
  const scrollLeft = wrap.scrollLeft;
  wrap.querySelectorAll(":scope > *").forEach((n) => n.remove());
  wrap.insertAdjacentHTML("beforeend", content);
  wrap.scrollLeft = scrollLeft;
  $("tracks").scrollTop = $("track-heads").scrollTop;
}

function renderRuler(total, pxPerSec) {
  const steps = [0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600];
  const step = steps.find((s) => s * pxPerSec >= 76) ?? 600;
  let out = "";
  for (let t = 0; t <= total / US; t += step) {
    out += `<div class="tick" style="left:${t * pxPerSec}px">${fmtTime(t * US)}</div>`;
  }
  return out;
}

function renderClip(clip, track, pxPerSec) {
  const left = (clip.timeline.start / US) * pxPerSec;
  const width = Math.max((clip.timeline.duration / US) * pxPerSec, 8);
  const asset = assetOf(clip);
  const isText = Boolean(clip.text);
  const kindClass = isText ? "text" : track.kind === "audio" || asset?.kind === "audio" ? "audio" : "";

  let visual = "";
  if (!isText && asset?.thumbnail_path && kindClass !== "audio") {
    visual = `<div class="filmstrip" style="background-image:url('${esc(fileUrl(asset.thumbnail_path))}')"></div>`;
  }
  if (asset?.waveform?.peaks?.length && (kindClass === "audio" || asset.kind !== "image")) {
    visual += waveformSvg(asset, clip, width);
  }

  const label = isText ? clip.text.content.slice(0, 40) : asset?.name || "média";
  const badges = [];
  if (clip.speed !== 1) badges.push(`${clip.speed.toFixed(2)}×`);
  if (clip.opacity < 1) badges.push(`${Math.round(clip.opacity * 100)}%`);
  if (clip.fade_in || clip.fade_out) badges.push("fondu");
  if (clip.effects.length) badges.push(`${clip.effects.length} effet${clip.effects.length > 1 ? "s" : ""}`);
  if (clip.volume === 0) badges.push("muet");

  return `<div class="clip ${kindClass}" data-clip="${esc(clip.id)}" data-track="${esc(track.id)}"
      style="left:${left}px;width:${width}px" aria-selected="${S.selection === clip.id}"
      title="${esc(label)} — ${fmtDuration(clip.timeline.duration)}">
    ${visual}
    <div class="label">${esc(label)}</div>
    ${badges.length ? `<div class="badges">${badges.map(esc).join(" · ")}</div>` : ""}
    <div class="handle left" data-handle="left"></div>
    <div class="handle right" data-handle="right"></div>
  </div>`;
}

/** Draw only the part of the waveform the clip actually uses. */
function waveformSvg(asset, clip, width) {
  const peaks = asset.waveform.peaks;
  const from = clip.source.start / asset.duration;
  const to = (clip.source.start + clip.source.duration) / asset.duration;
  const start = clamp(Math.floor(from * peaks.length), 0, peaks.length - 1);
  const end = clamp(Math.ceil(to * peaks.length), start + 1, peaks.length);
  const slice = peaks.slice(start, end);
  if (!slice.length) return "";
  const points = slice.length;
  const mid = 50;
  let up = "";
  let down = "";
  for (let i = 0; i < points; i += 1) {
    const x = ((i / (points - 1)) * 100).toFixed(2);
    up += `${x},${(mid - slice[i][1] * 46).toFixed(1)} `;
    down = `${x},${(mid - slice[i][0] * 46).toFixed(1)} ${down}`;
  }
  return `<svg class="wave" viewBox="0 0 100 100" preserveAspectRatio="none" style="width:${width}px">
    <polyline points="${up}" fill="none" stroke="rgba(255,255,255,0.75)" stroke-width="1.1"/>
    <polyline points="${down}" fill="none" stroke="rgba(255,255,255,0.75)" stroke-width="1.1"/>
  </svg>`;
}

/** Gaps are first-class: an editor that hides them makes "why is there a hole"
 *  a mystery, so each one offers to close itself. */
function renderGaps(track, pxPerSec) {
  const sorted = [...track.clips].sort((a, b) => a.timeline.start - b.timeline.start);
  let out = "";
  let cursor = 0;
  for (const clip of sorted) {
    const gap = clip.timeline.start - cursor;
    if (gap > 400000 && (gap / US) * pxPerSec > 40) {
      out += `<button class="gap" data-gap-track="${esc(track.id)}" data-gap-end="${clipEnd(clip)}"
        style="left:${(cursor / US) * pxPerSec}px;width:${(gap / US) * pxPerSec}px"
        title="Supprimer cet espace">espace ${fmtDuration(gap)}</button>`;
    }
    cursor = Math.max(cursor, clipEnd(clip));
  }
  return out;
}

/* -------------------------------------------------------------- interactions */

/** Snap a time to clip edges and the playhead, within a few pixels.
 *  Returns the resulting time and the edge that caught it (null when free). */
function snapTime(time, ignoreClipId) {
  const p = project();
  const threshold = (8 / S.zoom) * US;
  const candidates = [0, S.playhead];
  for (const track of p.tracks) {
    for (const clip of track.clips) {
      if (clip.id === ignoreClipId) continue;
      candidates.push(clip.timeline.start, clipEnd(clip));
    }
  }
  let best = time;
  let bestDelta = threshold;
  for (const candidate of candidates) {
    const delta = Math.abs(candidate - time);
    if (delta < bestDelta) {
      bestDelta = delta;
      best = candidate;
    }
  }
  return { time: best, snappedAt: best === time ? null : best };
}

/** Show the vertical guide that says "you are landing exactly here". */
function showSnapLine(at) {
  const line = $("snap-line");
  if (!line) return;
  if (at === null) {
    line.hidden = true;
    return;
  }
  line.hidden = false;
  line.style.left = `${(at / US) * S.zoom}px`;
}

function hideSnapLine() {
  const line = $("snap-line");
  if (line) line.hidden = true;
}

/** Highlight the lane a dragged clip would land on. */
function highlightLane(trackId) {
  for (const lane of document.querySelectorAll(".track-lane")) {
    if (trackId && lane.dataset.lane === trackId) lane.dataset.drop = "true";
    else delete lane.dataset.drop;
  }
}

function clearLaneHighlight() {
  for (const lane of document.querySelectorAll(".track-lane")) delete lane.dataset.drop;
}

function laneFromPoint(clientY) {
  const lanes = [...document.querySelectorAll(".track-lane")];
  for (const lane of lanes) {
    const rect = lane.getBoundingClientRect();
    if (clientY >= rect.top && clientY <= rect.bottom) return lane.dataset.lane;
  }
  return null;
}

function timeFromPoint(clientX) {
  const content = $("tl-content");
  const rect = content.getBoundingClientRect();
  return Math.max(0, ((clientX - rect.left) / S.zoom) * US);
}

function startClipDrag(event, clipEl, mode) {
  const p = project();
  const clipId = clipEl.dataset.clip;
  const trackId = clipEl.dataset.track;
  let found = null;
  for (const track of p.tracks) {
    const clip = track.clips.find((c) => c.id === clipId);
    if (clip) found = clip;
  }
  if (!found) return;
  const track = p.tracks.find((t) => t.id === trackId);
  if (track?.locked) {
    toast("Cette piste est verrouillée : déverrouillez-la pour la modifier", "error");
    return;
  }
  const asset = assetOf(found);

  S.drag = {
    mode,
    clipId,
    trackId,
    startX: event.clientX,
    origin: { ...found, timeline: { ...found.timeline }, source: { ...found.source } },
    assetDuration: asset?.duration ?? found.source.duration,
    isText: Boolean(found.text),
    isAudio: asset?.kind === "audio" && !found.text,
    el: clipEl,
    moved: false,
    targetTrack: trackId,
    previewStart: found.timeline.start,
  };
  // Capture keeps events flowing when the pointer leaves the clip, but it is an
  // optimisation: a pointer that is already gone must not abort the drag.
  try {
    clipEl.setPointerCapture(event.pointerId);
  } catch {
    /* synthetic or released pointer */
  }
  event.preventDefault();
}

function updateClipDrag(event) {
  const drag = S.drag;
  if (!drag) return;
  const deltaUs = ((event.clientX - drag.startX) / S.zoom) * US;
  if (Math.abs(event.clientX - drag.startX) > 2) drag.moved = true;
  const el = drag.el;
  const speed = drag.origin.speed || 1;
  const minDuration = 33000; // one frame at 30 fps

  if (drag.mode === "move") {
    let start = Math.max(0, drag.origin.timeline.start + deltaUs);
    const snap = snapTime(start, drag.clipId);
    start = snap.time;
    showSnapLine(snap.snappedAt);
    drag.previewStart = start;
    el.style.transform = `translateX(${((start - drag.origin.timeline.start) / US) * S.zoom}px)`;
    const lane = laneFromPoint(event.clientY);
    if (lane && lane !== drag.targetTrack) {
      const target = project().tracks.find((t) => t.id === lane);
      const asset = assetOf(drag.origin);
      if (target && compatibleTrack(target, asset, drag.isText)) drag.targetTrack = lane;
    }
    highlightLane(drag.targetTrack === drag.trackId ? null : drag.targetTrack);
    return;
  }

  if (drag.mode === "trim-left") {
    const maxDelta = drag.origin.timeline.duration - minDuration;
    const minDelta = drag.isText ? -drag.origin.timeline.start : -drag.origin.source.start / speed;
    const delta = clamp(deltaUs, minDelta, maxDelta);
    const newStart = Math.max(0, drag.origin.timeline.start + delta);
    const newDuration = drag.origin.timeline.duration - (newStart - drag.origin.timeline.start);
    const newSourceStart = drag.isText ? 0 : drag.origin.source.start + delta * speed;
    const snap = snapTime(newStart, drag.clipId);
    showSnapLine(snap.snappedAt);
    drag.previewStart = newStart;
    drag.previewTrim = { timelineStart: newStart, timelineDuration: newDuration, sourceStart: newSourceStart };
    el.style.left = `${(newStart / US) * S.zoom}px`;
    el.style.width = `${Math.max((newDuration / US) * S.zoom, 8)}px`;
    return;
  }

  // trim-right
  const available = drag.isText
    ? Number.POSITIVE_INFINITY
    : (drag.assetDuration - drag.origin.source.start) / speed;
  const newDuration = clamp(drag.origin.timeline.duration + deltaUs, minDuration, available);
  drag.previewTrim = {
    timelineStart: drag.origin.timeline.start,
    timelineDuration: newDuration,
    sourceStart: drag.origin.source.start,
  };
  el.style.width = `${Math.max((newDuration / US) * S.zoom, 8)}px`;
}

async function endClipDrag() {
  const drag = S.drag;
  S.drag = null;
  hideSnapLine();
  clearLaneHighlight();
  if (!drag) return;
  if (!drag.moved) return;

  if (drag.mode === "move") {
    const changed = drag.previewStart !== drag.origin.timeline.start || drag.targetTrack !== drag.trackId;
    if (!changed) {
      renderTimeline();
      return;
    }
    await mutate("/api/clip/move", {
      clip_id: drag.clipId,
      track_id: drag.targetTrack,
      timeline_start: Math.round(drag.previewStart),
    });
    return;
  }
  const trim = drag.previewTrim;
  if (!trim) {
    renderTimeline();
    return;
  }
  await mutate("/api/clip/trim", {
    clip_id: drag.clipId,
    timeline_start: Math.round(trim.timelineStart),
    timeline_duration: Math.round(trim.timelineDuration),
    source_start: Math.round(trim.sourceStart),
  });
}

/* ------------------------------------------------------------ preview/export */

async function pollJob(id, onProgress) {
  for (;;) {
    await new Promise((r) => setTimeout(r, 350));
    let job;
    try {
      job = await api.get(`/api/jobs/${id}`);
    } catch {
      return null;
    }
    if (onProgress) onProgress(job.progress, job);
    if (job.status !== "running") return job;
  }
}

/* ------------------------------------------------------------------ modals */

function openModal(html) {
  const host = $("modal-host");
  host.innerHTML = `<div class="modal-backdrop"><div class="modal">${html}</div></div>`;
  const backdrop = host.querySelector(".modal-backdrop");
  backdrop.addEventListener("pointerdown", (e) => {
    if (e.target === backdrop) closeModal();
  });
  return host.querySelector(".modal");
}

function closeModal() {
  $("modal-host").innerHTML = "";
}

function exportModal() {
  const p = project();
  const s = session();
  if (s.duration === 0) {
    toast("La timeline est vide : ajoutez au moins un clip", "error");
    return;
  }
  const options = [
    [480, "480p", "Pour un brouillon"],
    [720, "720p", "Fichier plus léger, export plus rapide"],
    [1080, "1080p", "Haute qualité"],
    [2160, "4K", "Pour exploiter vos sources 4K"],
  ];
  const qualities = [
    ["draft", "Rapide", "Compression plus visible"],
    ["standard", "Équilibré", "Le bon choix par défaut"],
    ["high", "Soigné", "Fichier plus lourd"],
    ["best", "Maximal", "Le plus lent"],
  ];
  const modal = openModal(`
    <h2>Exporter la vidéo</h2>
    <p class="sub">${fmtDuration(s.duration)} · ${p.canvas.width}×${p.canvas.height} ·
      encodage ${s.hardware_accelerated ? "accéléré par le GPU" : "sur le processeur"}</p>

    <div class="hint">Résolution</div>
    <div class="option-grid" id="opt-resolution">
      ${options
        .map(
          ([value, label, why]) =>
            `<button class="option" data-resolution="${value}" aria-pressed="${value === 1080}">
              <b>${label}</b><span>${why}</span></button>`
        )
        .join("")}
    </div>

    <div class="hint">Qualité</div>
    <div class="option-grid" id="opt-quality">
      ${qualities
        .map(
          ([value, label, why]) =>
            `<button class="option" data-quality="${value}" aria-pressed="${value === "standard"}">
              <b>${label}</b><span>${why}</span></button>`
        )
        .join("")}
    </div>

    <div class="field">
      <label for="export-path">Emplacement du fichier</label>
      <input type="text" id="export-path" value="${esc(slugify(p.name))}.mp4" />
      <p class="hint why">Un nom relatif est écrit dans le dossier de travail du service.</p>
    </div>

    <div id="export-progress" hidden>
      <div class="progress"><i id="export-bar"></i></div>
      <p class="hint" id="export-status">Préparation…</p>
    </div>

    <div class="modal-actions">
      <button class="btn left" id="export-close">Annuler</button>
      <button class="btn primary" id="export-start">Exporter</button>
    </div>
  `);

  let resolution = 1080;
  let quality = "standard";
  modal.querySelectorAll("[data-resolution]").forEach((btn) => {
    btn.onclick = () => {
      resolution = Number(btn.dataset.resolution);
      modal.querySelectorAll("[data-resolution]").forEach((b) => b.setAttribute("aria-pressed", String(b === btn)));
    };
  });
  modal.querySelectorAll("[data-quality]").forEach((btn) => {
    btn.onclick = () => {
      quality = btn.dataset.quality;
      modal.querySelectorAll("[data-quality]").forEach((b) => b.setAttribute("aria-pressed", String(b === btn)));
    };
  });

  modal.querySelector("#export-close").onclick = () => closeModal();
  modal.querySelector("#export-start").onclick = async () => {
    const path = modal.querySelector("#export-path").value.trim();
    const job = await act(() =>
      api.post("/api/export", { height: resolution, quality, container: "mp4", name: path })
    );
    if (!job) return;
    modal.querySelector("#export-progress").hidden = false;
    modal.querySelector("#export-start").disabled = true;
    const result = await pollJob(job.id, (progress, j) => {
      modal.querySelector("#export-bar").style.width = `${progress * 100}%`;
      modal.querySelector("#export-status").textContent = j.message;
    });
    if (!result) return;
    modal.querySelector("#export-close").textContent = "Fermer";
    modal.querySelector("#export-close").disabled = false;
    if (result.status === "done") {
      modal.querySelector("#export-status").textContent =
        `Terminé · ${fmtBytes(result.bytes)} · ${result.elapsed?.toFixed(1)} s · ${result.message}`;
      modal.querySelector("#export-bar").style.width = "100%";
      const link = document.createElement("a");
      link.className = "btn";
      link.href = result.output;
      link.download = "";
      link.textContent = "Télécharger le fichier";
      modal.querySelector("#export-start").replaceWith(link);
      toast(`Export terminé : ${result.message}`, "success");
    } else {
      modal.querySelector("#export-status").textContent = result.message;
      modal.querySelector("#export-start").disabled = false;
      toast(result.message, "error");
    }
  };
}

function helpModal() {
  const rows = [
    ["Espace", "Lecture / pause de l'aperçu"],
    ["S", "Découper au curseur"],
    ["Suppr", "Supprimer le clip sélectionné"],
    ["Ctrl + D", "Dupliquer le clip"],
    ["Ctrl + Z", "Annuler"],
    ["Ctrl + Maj + Z", "Rétablir"],
    ["← / →", "Déplacer le curseur d'une seconde"],
    ["Maj + ← / →", "Déplacer le curseur d'une image"],
    ["Début / Fin", "Aller au début ou à la fin"],
    ["+ / −", "Zoomer ou dézoomer la timeline"],
    ["[", "Afficher ou masquer le panneau des médias"],
    ["]", "Afficher ou masquer les propriétés"],
  ];
  const modal = openModal(`
    <h2>Raccourcis clavier</h2>
    <p class="sub">Les mêmes gestes que dans n'importe quel monteur, pour ne rien réapprendre.</p>
    <table style="width:100%;border-collapse:collapse">
      ${rows
        .map(
          ([key, what]) =>
            `<tr><td style="padding:6px 0;font-family:var(--font-mono);font-size:12px;color:var(--text-dim);width:130px">${esc(key)}</td>
             <td style="padding:6px 0">${esc(what)}</td></tr>`
        )
        .join("")}
    </table>
    <div class="modal-actions"><button class="btn primary" id="help-close">Compris</button></div>
  `);
  modal.querySelector("#help-close").onclick = () => closeModal();
}

/* --------------------------------------------------------------- inspector IO */

/** Inspector edits are debounced and applied optimistically to the DOM, so a
 *  slider never fights a re-render while the user is dragging it. */
const pendingPatch = { timer: null, patch: {} };

function queueClipPatch(patch, label) {
  Object.assign(pendingPatch.patch, patch);
  clearTimeout(pendingPatch.timer);
  pendingPatch.timer = setTimeout(async () => {
    const body = { clip_id: S.selection, ...pendingPatch.patch };
    pendingPatch.patch = {};
    const snap = await act(() => api.post("/api/clip/update", { ...body, label }));
    if (snap) {
      S.snap = snap;
      renderStage();
      renderTimeline();
      renderEffectsCatalog();
    }
  }, 160);
}

/* ------------------------------------------------------------------- events */

function bindEvents() {
  // --- header
  $("btn-undo").onclick = () => mutate("/api/history/undo");
  $("btn-redo").onclick = () => mutate("/api/history/redo");
  $("btn-export").onclick = exportModal;
  $("btn-help").onclick = helpModal;
  $("btn-theme").onclick = () => {
    S.theme = S.theme === "dark" ? "light" : "dark";
    localStorage.setItem("velocis.theme", S.theme);
    document.documentElement.dataset.theme = S.theme;
    toast(`Thème ${S.theme === "dark" ? "sombre" : "clair"} activé`, "success");
    renderHeader();
  };

  $("project-name").addEventListener("change", async (e) => {
    const name = e.target.value.trim() || "Projet sans titre";
    e.target.value = name;
    await mutate("/api/project/rename", { name });
  });

  $("aspect-switch").addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-aspect]");
    if (!btn) return;
    const { height, width } = project().canvas;
    // Only the canvas changes: every clip stays exactly where it is.
    await mutate("/api/project/canvas", { aspect: btn.dataset.aspect, height });
    toast(`Format ${width}×${height} → ${project().canvas.width}×${project().canvas.height}`, "success");
  });

  // --- sidebar
  // The rail selects a section; clicking the current one folds the panel, which
  // is the quickest way to give the stage more room.
  $("rail").addEventListener("click", (e) => {
    const tab = e.target.closest("[data-tab]");
    if (!tab) return;
    if (!S.panel) {
      S.tab = tab.dataset.tab;
      setPanel(true);
    } else if (S.tab === tab.dataset.tab) {
      setPanel(false);
    } else {
      S.tab = tab.dataset.tab;
    }
    renderSidebar();
  });

  $("btn-panel").onclick = togglePanel;
  $("btn-panel-close").onclick = () => setPanel(false);
  $("btn-panel-collapse").onclick = () => setPanel(false);
  $("btn-inspector").onclick = toggleInspector;
  $("btn-inspector-close").onclick = () => setInspector(false);

  // Timeline height: drag the strip at its top edge. Remembered across sessions.
  const resize = $("timeline-resize");
  resize.addEventListener("pointerdown", (e) => {
    try {
      resize.setPointerCapture(e.pointerId);
    } catch {
      /* synthetic or released pointer */
    }
    const startY = e.clientY;
    const startH = document.querySelector(".timeline").getBoundingClientRect().height;
    const move = (ev) => {
      const height = clamp(startH - (ev.clientY - startY), 150, window.innerHeight * 0.62);
      document.documentElement.style.setProperty("--timeline-h", `${Math.round(height)}px`);
      localStorage.setItem("velocis.timelineH", String(Math.round(height)));
    };
    const up = () => {
      resize.removeEventListener("pointermove", move);
      resize.removeEventListener("pointerup", up);
    };
    resize.addEventListener("pointermove", move);
    resize.addEventListener("pointerup", up);
  });

  const dropzone = $("dropzone");
  dropzone.onclick = () => $("file-input").click();
  dropzone.onkeydown = (e) => {
    if (e.key === "Enter" || e.key === " ") $("file-input").click();
  };

  // Files can be dropped anywhere in the window, not only on the dropzone: an
  // editor that requires you to aim at a small target is annoying to use.
  const shell = document.querySelector(".app");
  const hasFiles = (e) => Boolean(e.dataTransfer?.types?.includes("Files"));
  let dragDepth = 0;
  window.addEventListener("dragenter", (e) => {
    if (!hasFiles(e)) return;
    dragDepth += 1;
    shell.dataset.dropping = "true";
  });
  window.addEventListener("dragleave", () => {
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) delete shell.dataset.dropping;
  });
  window.addEventListener("dragover", (e) => {
    if (hasFiles(e)) e.preventDefault();
  });
  window.addEventListener("drop", async (e) => {
    // Media dragged from the library carries a custom type, not files: leave it
    // to the timeline's own handler.
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    delete shell.dataset.dropping;
    if (e.dataTransfer.files.length) await importFiles([...e.dataTransfer.files]);
  });
  $("file-input").addEventListener("change", async (e) => {
    if (e.target.files.length) await importFiles([...e.target.files]);
    e.target.value = "";
  });
  $("btn-import-path").onclick = async () => {
    const path = prompt("Chemin absolu du fichier à importer :");
    if (!path) return;
    const result = await act(() => api.post("/api/import", { paths: [path] }));
    if (result) {
      if (result.failed?.length) toast(result.failed[0].error, "error");
      await refresh();
      if (result.imported?.length) toast(`« ${result.imported[0].name} » importé`, "success");
    }
  };

  $("media-grid").addEventListener("click", (e) => {
    const add = e.target.closest("[data-add-asset]");
    if (!add) return;
    e.stopPropagation();
    addToTimeline(add.dataset.addAsset);
  });

  $("media-grid").addEventListener("dblclick", (e) => {
    const card = e.target.closest("[data-asset]");
    if (!card) return;
    const asset = project().assets.find((a) => a.id === card.dataset.asset);
    const wanted = asset.kind === "audio" ? "audio" : "video";
    const track = project().tracks.find((t) => t.kind === wanted);
    if (!track) return;
    mutate("/api/clip/append", { track_id: track.id, asset_id: asset.id }, );
  });
  $("media-grid").addEventListener("dragstart", (e) => {
    const card = e.target.closest("[data-asset]");
    if (card) e.dataTransfer.setData("text/velocis-asset", card.dataset.asset);
  });

  $("btn-add-text").onclick = async () => {
    const content = $("text-content").value.trim();
    if (!content) {
      toast("Saisissez d'abord un texte", "error");
      return;
    }
    const duration = Number($("text-duration").value) * 1000;
    const size = Number($("text-size").value);
    await mutate("/api/clip/text", {
      content,
      timeline_start: Math.round(S.playhead),
      duration,
      font_size: size,
    });
    toast("Texte ajouté au curseur, sur sa propre piste", "success");
  };
  $("text-size").oninput = (e) => ($("text-size-value").textContent = e.target.value);
  $("text-duration").oninput = (e) =>
    ($("text-duration-value").textContent = `${(Number(e.target.value) / 1000).toFixed(1).replace(".", ",")} s`);

  $("effects-catalog").addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-add-effect]");
    if (!btn || !S.selection) return;
    const spec = effectSpec(btn.dataset.addEffect);
    if (!spec) return;
    const selection = selectedClip();
    const effect = { kind: spec.kind };
    for (const param of spec.params) effect[param.name] = param.def;
    if (spec.color) effect.color = { r: 0, g: 255, b: 0, a: 255 };
    await mutate("/api/clip/update", {
      clip_id: selection.clip.id,
      effects: [...selection.clip.effects, effect],
      label: `Ajouter l'effet ${spec.label}`,
    });
    toast(`Effet « ${spec.label} » appliqué`, "success");
  });

  // --- transport
  $("btn-play").onclick = togglePlay;

  // Actions offered by the stage's welcome card. Delegated from the stage itself
  // because the card is re-rendered whenever the project changes.
  document.querySelector(".stage").addEventListener("click", (e) => {
    const btn = e.target.closest("[data-action]");
    if (!btn) return;
    if (btn.dataset.action === "import") $("file-input").click();
    if (btn.dataset.action === "text") {
      S.tab = "text";
      renderSidebar();
      $("text-content").focus();
    }
  });
  $("scrub").addEventListener("pointerdown", (e) => {
    try {
      $("scrub").setPointerCapture(e.pointerId);
    } catch {
      /* synthetic or released pointer */
    }
    seekFromScrub(e);
    const move = (ev) => seekFromScrub(ev);
    const up = () => {
      $("scrub").removeEventListener("pointermove", move);
      $("scrub").removeEventListener("pointerup", up);
    };
    $("scrub").addEventListener("pointermove", move);
    $("scrub").addEventListener("pointerup", up);
  });

  // --- inspector
  $("inspector-tabs").addEventListener("click", (e) => {
    const tab = e.target.closest("[data-inspector-tab]");
    if (!tab) return;
    S.inspectorTab = tab.dataset.inspectorTab;
    renderInspector();
  });

  const inspector = $("inspector-body");
  inspector.addEventListener("input", onInspectorInput);
  inspector.addEventListener("change", onInspectorInput);
  inspector.addEventListener("click", onInspectorClick);

  // --- timeline
  // One delegation root: the ruler, lanes and clips are re-created on every
  // render, so listeners must live on a node that survives.
  const wrap = document.querySelector(".ruler-wrap");
  wrap.addEventListener("scroll", () => {
    $("track-heads").scrollTop = wrap.scrollTop;
  });

  wrap.addEventListener("pointerdown", (e) => {
    const ruler = e.target.closest(".ruler");
    if (ruler) {
      try {
        ruler.setPointerCapture(e.pointerId);
      } catch {
        /* synthetic or released pointer */
      }
      setPlayhead(timeFromPoint(e.clientX));
      const move = (ev) => setPlayhead(timeFromPoint(ev.clientX));
      const up = () => {
        ruler.removeEventListener("pointermove", move);
        ruler.removeEventListener("pointerup", up);
      };
      ruler.addEventListener("pointermove", move);
      ruler.addEventListener("pointerup", up);
      return;
    }

    const clipEl = e.target.closest(".clip");
    if (!clipEl) {
      // Only the ruler moves the playhead. Clicking a lane used to seek too, but
      // it made every stray click jump the timeline: deselecting is enough.
      S.selection = null;
      renderTimeline();
      renderInspector();
      renderEffectsCatalog();
      return;
    }
    const handle = e.target.closest("[data-handle]");
    S.selection = clipEl.dataset.clip;
    // Selecting a clip that is not under the playhead would show a selection box
    // over unrelated frames. Move the playhead onto it so direct manipulation
    // always acts on what is on screen.
    const selected = selectedClip();
    if (selected) {
      const { clip } = selected;
      if (S.playhead < clip.timeline.start || S.playhead >= clipEnd(clip)) {
        // Land a little inside: at the exact start a clip with a fade-in is
        // still fully transparent, so the box would float over nothing.
        setPlayhead(clip.timeline.start + Math.min(500_000, Math.floor(clip.timeline.duration / 2)));
      }
    }
    // Deliberately *not* a full re-render: rebuilding the timeline would replace
    // the very node being dragged, and pointer events on a detached node no
    // longer reach the delegation root. Selection is reflected in place, and the
    // full render happens when the gesture ends.
    for (const node of document.querySelectorAll(".clip")) {
      node.setAttribute("aria-selected", String(node === clipEl));
    }
    renderInspector();
    renderEffectsCatalog();
    startClipDrag(e, clipEl, handle ? (handle.dataset.handle === "left" ? "trim-left" : "trim-right") : "move");
  });

  wrap.addEventListener("pointermove", (e) => updateClipDrag(e));
  wrap.addEventListener("pointerup", () => endClipDrag());
  wrap.addEventListener("pointercancel", () => endClipDrag());

  wrap.addEventListener("click", (e) => {
    const gap = e.target.closest("[data-gap-track]");
    if (gap) closeGap(gap.dataset.gapTrack, Number(gap.dataset.gapEnd));
  });

  // Drop media straight onto a lane: the drop point is the insert point.
  wrap.addEventListener("dragover", (e) => {
    if (e.dataTransfer.types.includes("text/velocis-asset")) e.preventDefault();
  });
  wrap.addEventListener("drop", async (e) => {
    const assetId = e.dataTransfer.getData("text/velocis-asset");
    if (!assetId) return;
    e.preventDefault();
    const lane = laneFromPoint(e.clientY);
    if (!lane) return;
    const at = Math.round(snapTime(timeFromPoint(e.clientX)).time);
    await mutate("/api/clip/place", { track_id: lane, asset_id: assetId, timeline_start: at });
  });

  $("track-heads").addEventListener("click", (e) => {
    const toggle = e.target.closest("[data-track-toggle]");
    if (toggle) {
      const track = project().tracks.find((t) => t.id === toggle.dataset.track);
      const field = toggle.dataset.trackToggle;
      mutate("/api/track/update", {
        track_id: toggle.dataset.track,
        [field]: !track[field],
      });
      return;
    }
    const remove = e.target.closest("[data-track-remove]");
    if (remove) {
      if (!confirm("Supprimer cette piste et les clips qu'elle contient ?")) return;
      mutate("/api/track/remove", { track_id: remove.dataset.trackRemove });
    }
  });

  $("btn-add-video-track").onclick = () => mutate("/api/track/add", { kind: "video" });
  $("btn-add-audio-track").onclick = () => mutate("/api/track/add", { kind: "audio" });
  $("btn-split").onclick = splitAtPlayhead;
  $("btn-delete").onclick = deleteSelection;
  $("btn-duplicate").onclick = duplicateSelection;
  $("btn-fit").onclick = () => {
    const duration = session()?.duration || 1;
    const width = document.querySelector(".ruler-wrap").clientWidth - 40;
    S.zoom = clamp(width / (duration / US), 6, 320);
    $("zoom").value = String(Math.round(S.zoom));
    renderTimeline();
  };
  $("zoom").oninput = (e) => {
    S.zoom = Number(e.target.value);
    renderTimeline();
  };

  document.addEventListener("keydown", onKeyDown);
  window.addEventListener("resize", () => renderTimeline());
}

/** Track kind a dropped asset belongs on. */
function compatibleTrack(track, asset, isText) {
  if (track.locked) return false;
  if (track.kind === "audio") return asset?.kind === "audio";
  return isText || asset?.kind !== "audio";
}

function seekFromScrub(e) {
  const rect = $("scrub").getBoundingClientRect();
  const ratio = clamp((e.clientX - rect.left) / rect.width, 0, 1);
  const total = session()?.duration || 0;
  setPlayhead(ratio * total);
}

function setPlayhead(us) {
  const total = stage?.duration() ?? 0;
  S.playhead = clamp(us, 0, Math.max(total, 0));
  stage?.seek(S.playhead);
  updateTransport();
}

function togglePlay() {
  if (!stage) return;
  if (stage.duration() <= 0) {
    toast("Ajoutez au moins un clip pour lancer la lecture", "error");
    return;
  }
  S.playing = stage.toggle();
  updateTransport();
}

function splitAtPlayhead() {
  if (!project().tracks.some((t) => t.clips.length)) {
    toast("Rien à découper : la timeline est vide", "error");
    return;
  }
  mutate("/api/clip/split-all", { at: Math.round(S.playhead) });
}

function deleteSelection() {
  if (!S.selection) {
    toast("Sélectionnez d'abord un clip", "error");
    return;
  }
  mutate("/api/clip/delete", { clip_id: S.selection, ripple: true });
  S.selection = null;
}

function duplicateSelection() {
  if (!S.selection) {
    toast("Sélectionnez d'abord un clip", "error");
    return;
  }
  mutate("/api/clip/duplicate", { clip_id: S.selection });
}

/** Close a gap by pulling everything after it to the left. */
async function closeGap(trackId, gapEnd) {
  const track = project().tracks.find((t) => t.id === trackId);
  const sorted = [...track.clips].sort((a, b) => a.timeline.start - b.timeline.start);
  const after = sorted.find((c) => c.timeline.start >= gapEnd - 1000);
  if (!after) return;
  const previous = sorted.filter((c) => clipEnd(c) <= gapEnd + 1000).pop();
  const target = previous ? clipEnd(previous) : 0;
  const shift = after.timeline.start - target;
  if (shift <= 0) return;
  // Move the clip that starts the gap, then every following clip in turn.
  const chain = sorted.filter((c) => c.timeline.start >= after.timeline.start - 1000);
  for (const clip of chain) {
    await mutate("/api/clip/move", {
      clip_id: clip.id,
      track_id: trackId,
      timeline_start: Math.round(clip.timeline.start - shift),
    });
  }
  toast("Espace supprimé", "success");
}

function onKeyDown(e) {
  const tag = document.activeElement?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
  const meta = e.ctrlKey || e.metaKey;

  if (e.code === "Space") {
    e.preventDefault();
    togglePlay();
    return;
  }
  if (meta && e.key.toLowerCase() === "z") {
    e.preventDefault();
    mutate(e.shiftKey ? "/api/history/redo" : "/api/history/undo");
    return;
  }
  if (meta && e.key.toLowerCase() === "y") {
    e.preventDefault();
    mutate("/api/history/redo");
    return;
  }
  if (meta && e.key.toLowerCase() === "d") {
    e.preventDefault();
    duplicateSelection();
    return;
  }
  if (e.key === "Delete" || e.key === "Backspace") {
    e.preventDefault();
    deleteSelection();
    return;
  }
  if (e.key.toLowerCase() === "s" && !meta) {
    e.preventDefault();
    splitAtPlayhead();
    return;
  }
  const frame = US / 30;
  if (e.key === "ArrowLeft") {
    e.preventDefault();
    setPlayhead(S.playhead - (e.shiftKey ? frame : US));
  } else if (e.key === "ArrowRight") {
    e.preventDefault();
    setPlayhead(S.playhead + (e.shiftKey ? frame : US));
  } else if (e.key === "Home") {
    setPlayhead(0);
  } else if (e.key === "End") {
    setPlayhead(session()?.duration || 0);
  } else if (e.key === "+" || e.key === "=") {
    $("zoom").value = String(clamp(S.zoom * 1.3, 6, 320));
    S.zoom = Number($("zoom").value);
    renderTimeline();
  } else if (e.key === "-") {
    $("zoom").value = String(clamp(S.zoom / 1.3, 6, 320));
    S.zoom = Number($("zoom").value);
    renderTimeline();
  } else if (e.key === "[") {
    togglePanel();
  } else if (e.key === "]") {
    toggleInspector();
  } else if (e.key === "Escape") {
    closeModal();
  }
}

/* ---------------------------------------------------------- inspector events */

function onInspectorInput(e) {
  const target = e.target;
  const selection = selectedClip();
  if (!selection) return;
  const { clip } = selection;

  // Transform sliders
  if (target.dataset.transform) {
    const field = target.dataset.transform;
    const value = Number(target.value);
    const label = document.querySelector(`#f-${shortLabel(field)}-v`);
    if (label) {
      label.textContent =
        field === "scale" ? `${Math.round(value * 100)} %` : field === "rotation_deg" ? `${Math.round(value)}°` : `${Math.round(value)} px`;
    }
    queueClipPatch({ transform: { ...clip.transform, [field]: value } }, "Ajuster la transformation");
    return;
  }

  // Plain clip fields
  if (target.dataset.clipField) {
    const field = target.dataset.clipField;
    const value = Number(target.value);
    const labelMap = {
      opacity: ["f-opacity-v", `${Math.round(value * 100)} %`],
      volume: ["f-vol-v", `${Math.round(value * 100)} %`],
      fade_in: ["f-fi-v", fmtDuration(value)],
      fade_out: ["f-fo-v", fmtDuration(value)],
      speed: ["f-speed-v", `${value.toFixed(2).replace(".", ",")}×`],
    };
    const [id, text] = labelMap[field] || [];
    if (id && $(id)) $(id).textContent = text;
    queueClipPatch({ [field]: value }, "Ajuster le clip");
    return;
  }

  // Text fields
  if (target.dataset.textField === "font_size") {
    $("f-size-v").textContent = target.value;
    queueClipPatch({ text: { ...clip.text, font_size: Number(target.value) } }, "Modifier le texte");
    return;
  }
  if (target.hasAttribute("data-clip-text")) {
    queueClipPatch({ text: { ...clip.text, content: target.value } }, "Modifier le texte");
    return;
  }
  if (target.hasAttribute("data-text-color")) {
    queueClipPatch({ text: { ...clip.text, color: hexToRgba(target.value) } }, "Changer la couleur");
    return;
  }

  // Effect parameters
  if (target.dataset.effect !== undefined && target.dataset.effectParam) {
    const index = Number(target.dataset.effect);
    const effects = clip.effects.map((effect, i) =>
      i === index ? { ...effect, [target.dataset.effectParam]: Number(target.value) } : effect
    );
    queueClipPatch({ effects }, "Régler l'effet");
    return;
  }
  if (target.dataset.effect !== undefined && target.hasAttribute("data-effect-color")) {
    const index = Number(target.dataset.effect);
    const effects = clip.effects.map((effect, i) => (i === index ? { ...effect, color: hexToRgba(target.value) } : effect));
    queueClipPatch({ effects }, "Changer la couleur d'incrustation");
  }
}

function shortLabel(field) {
  return { x: "x", y: "y", scale: "scale", rotation_deg: "rot" }[field] ?? field;
}

function onInspectorClick(e) {
  const selection = selectedClip();
  if (!selection) return;
  const { clip } = selection;

  const mirror = e.target.closest("[data-transform-toggle]");
  if (mirror) {
    const field = mirror.dataset.transformToggle;
    queueClipPatch({ transform: { ...clip.transform, [field]: !clip.transform[field] } }, "Miroir");
    renderInspector();
    return;
  }
  if (e.target.closest("[data-crop-toggle]")) {
    manipulator?.setCropMode(!manipulator.cropMode);
    renderInspector();
    return;
  }
  if (e.target.closest("[data-crop-reset]")) {
    queueClipPatch(
      { transform: { ...clip.transform, crop_left: 0, crop_right: 0, crop_top: 0, crop_bottom: 0 } },
      "Recadrage réinitialisé"
    );
    renderInspector();
    return;
  }
  if (e.target.closest("[data-transform-reset]")) {
    queueClipPatch(
      { transform: { ...clip.transform, x: 0, y: 0, scale: 1, rotation_deg: 0 } },
      "Transformation réinitialisée"
    );
    renderInspector();
    return;
  }
  const textToggle = e.target.closest("[data-text-toggle]");
  if (textToggle) {
    const field = textToggle.dataset.textToggle;
    queueClipPatch({ text: { ...clip.text, [field]: !clip.text[field] } }, "Style du texte");
    renderInspector();
    return;
  }
  if (e.target.closest("[data-text-background]")) {
    const next = clip.text.background ? null : { r: 16, g: 14, b: 34, a: 150 };
    queueClipPatch({ text: { ...clip.text, background: next } }, "Fond du texte");
    renderInspector();
    return;
  }
  const remove = e.target.closest("[data-remove-effect]");
  if (remove) {
    const index = Number(remove.dataset.removeEffect);
    queueClipPatch(
      { effects: clip.effects.filter((_, i) => i !== index) },
      "Effet retiré"
    );
    renderInspector();
  }
}

/* --------------------------------------------------------------------- boot */

async function boot() {
  document.documentElement.dataset.theme = S.theme;
  stage = new LivePreview($("stage-canvas"), {
    onTime: (micros) => {
      S.playhead = micros;
      // The engine stops itself at the end of the timeline.
      S.playing = stage.playing;
      updateTransport();
    },
    segmentSeconds: 15,
    onNeedProxy: (assetId, index) => {
      if (!S.proxyNoticed) {
        S.proxyNoticed = true;
        toast(
          "Ce média n'est pas lisible par le navigateur : Velocis prépare un aperçu par segments. L'export utilisera le fichier d'origine.",
          "info"
        );
      }
      return ensureProxy(assetId, index);
    },
    onBlocked: () => {
      S.playing = false;
      updateTransport();
      toast("Le navigateur a bloqué la lecture : cliquez à nouveau sur Lecture", "error", {
        label: "Relancer",
        run: () => {
          stage.blocked = false;
          S.playing = stage.toggle();
          updateTransport();
        },
      });
    },
  });
  const savedHeight = Number(localStorage.getItem("velocis.timelineH"));
  if (savedHeight > 0) {
    document.documentElement.style.setProperty("--timeline-h", `${Math.round(savedHeight)}px`);
  }
  applyPanelState();
  manipulator = new StageManipulator({
    canvas: $("stage-canvas"),
    overlay: $("stage-overlay"),
    box: $("sel-box"),
    getProject: project,
    getSelection: selectedClip,
    onLive: () => stage?.requestDraw(),
    onCommit: (transform) => {
      const selection = selectedClip();
      if (!selection) return;
      mutate("/api/clip/update", {
        clip_id: selection.clip.id,
        transform,
        label: "Ajuster la transformation",
      });
    },
  });
  bindEvents();
  try {
    S.caps = await api.get("/api/capabilities");
  } catch {
    /* capabilities are informational only */
  }
  if (S.caps?.proxy_segment_seconds && stage) stage.segmentSeconds = S.caps.proxy_segment_seconds;
  await refresh();
  if (S.caps) {
    toast(
      `Encodage ${S.caps.hardware_accelerated ? "accéléré par le GPU" : "sur le processeur"} · ${S.caps.heights.join(", ")} disponibles`,
      "info"
    );
  }
}

boot();
