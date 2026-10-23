/* ============================================================================
   Velocis — composition temps réel
   ----------------------------------------------------------------------------
   The preview is composed in the browser instead of being rendered by the
   service: one <video> per source, one <canvas> for the result, and a clock that
   draws the timeline at display refresh rate. Scrubbing and playback therefore
   feel immediate, while the export keeps using the native engine for accuracy.

   Deliberate limits, stated rather than hidden:
     * effects are mapped onto canvas filters, so blur/brightness/contrast/
       saturation/hue/grayscale/sepia/invert are faithful, gamma and temperature
       are approximations, and sharpen/chroma-key have no canvas equivalent;
     * one video element per asset, so the same asset twice on screen at the same
       instant would share a decoder (the timeline forbids it on one track);
     * audio comes from the video elements, so it follows their clock.
   ========================================================================= */

const US = 1e6;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

export class LivePreview {
  constructor(canvas, { onTime, onBlocked, onNeedProxy, segmentSeconds } = {}) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d", { alpha: false });
    this.onTime = onTime ?? (() => {});
    this.onBlocked = onBlocked ?? null;
    this.blocked = false;
    this.onNeedProxy = onNeedProxy ?? null;
    this.segmentSeconds = segmentSeconds ?? 15;
    this.proxy = new Map();      // assetId -> { index, url, loading }
    this.proxyUrls = new Map();  // "assetId:index" -> url
    this.project = null;
    this.videos = new Map();
    this.images = new Map();
    this.time = 0;
    this.playing = false;
    this.raf = 0;
    this.drawPending = false;
    this.wallStart = 0;
    this.timeStart = 0;
    this.lastSync = 0;
    this.observer = new ResizeObserver(() => this.layout());
    if (canvas.parentElement) this.observer.observe(canvas.parentElement);
  }

  /* ---------------------------------------------------------------- project */

  setProject(project) {
    this.project = project;
    this.syncMedia();
    this.layout();
    this.draw();
  }

  duration() {
    let end = 0;
    for (const track of this.project?.tracks ?? []) {
      for (const clip of track.clips) {
        end = Math.max(end, clip.timeline.start + clip.timeline.duration);
      }
    }
    return end;
  }

  /** Create or drop media elements so they match the project's assets. */
  syncMedia() {
    const assets = new Map((this.project?.assets ?? []).map((a) => [a.id, a]));

    for (const [id, el] of this.videos) {
      if (!assets.has(id)) {
        el.pause();
        el.removeAttribute("src");
        el.load();
        el.remove();
        this.videos.delete(id);
      }
    }
    for (const [id, img] of this.images) {
      if (!assets.has(id)) this.images.delete(id);
    }

    for (const [id, asset] of assets) {
      if (asset.kind === "image") {
        if (!this.images.has(id)) {
          const img = new Image();
          img.decoding = "async";
          img.addEventListener("load", () => this.requestDraw());
          img.src = `/api/media/${id}`;
          this.images.set(id, img);
        }
        continue;
      }
      if (this.videos.has(id)) continue;

      const el = document.createElement("video");
      el.preload = "auto";
      el.playsInline = true;
      el.muted = false;
      el.style.cssText = "position:absolute;width:1px;height:1px;opacity:0;pointer-events:none";
      document.body.appendChild(el);
      // A seek or a freshly decoded frame must repaint even when paused.
      el.addEventListener("seeked", () => this.requestDraw());
      // Metadata arriving is also the moment a pending `currentTime` becomes
      // writable: setting it before the element knows its duration is ignored,
      // which silently shows the first frame of a proxy segment instead of the
      // frame the playhead is on.
      const resync = () => {
        this.syncPlayback(this.time);
        // Paint on the next animation frame: drawing straight from `seeked` can
        // run before the new frame is presented, which paints the old one again
        // and actually makes scrubbing feel slower.
        this.requestDraw();
      };
      el.addEventListener("loadedmetadata", resync);
      el.addEventListener("loadeddata", resync);
      this.videos.set(id, el);
      this.applySource(id, asset, el);
    }

    // A prepared proxy may have appeared since the element was created.
    for (const [id, asset] of assets) {
      const el = this.videos.get(id);
      if (el) this.applySource(id, asset, el);
    }
  }

  /// Choose what this asset should be decoded from:
  ///  * a prepared preview proxy (seek-friendly) when one exists;
  ///  * short segments when the browser cannot decode the file at all;
  ///  * the original otherwise.
  applySource(assetId, asset, el) {
    const variant = asset.preview_path
      ? "preview"
      : asset.browser_playable === false
      ? "segments"
      : "original";
    if (el.dataset.variant === variant) return;
    el.dataset.variant = variant;
    this.proxy.delete(assetId);
    if (variant === "segments") {
      el.removeAttribute("src");
      return;
    }
    el.src =
      variant === "preview"
        ? `/api/media/${assetId}?variant=preview`
        : `/api/media/${assetId}`;
  }

  /* ----------------------------------------------------------------- canvas */

  /** Fit the canvas into the stage, keeping the project's aspect ratio. */
  layout() {
    if (!this.project) return;
    const host = this.canvas.parentElement;
    if (!host) return;
    const { width, height } = this.project.canvas;
    const box = host.getBoundingClientRect();
    const pad = 0;
    const availableW = Math.max(64, box.width - pad);
    const availableH = Math.max(36, box.height - pad);
    const scale = Math.min(availableW / width, availableH / height);
    const displayW = Math.max(32, Math.floor(width * scale));
    const displayH = Math.max(18, Math.floor(height * scale));

    this.canvas.style.width = `${displayW}px`;
    this.canvas.style.height = `${displayH}px`;

    // Backing store: crisp text, bounded cost.
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const maxWidth = 1600;
    const backingScale = Math.min((displayW * dpr) / width, maxWidth / width);
    const bw = Math.max(2, Math.round(width * backingScale));
    const bh = Math.max(2, Math.round(height * backingScale));
    if (this.canvas.width !== bw || this.canvas.height !== bh) {
      this.canvas.width = bw;
      this.canvas.height = bh;
    }
    this.draw();
  }

  requestDraw() {
    if (this.drawPending) return;
    this.drawPending = true;
    requestAnimationFrame(() => {
      this.drawPending = false;
      this.draw();
    });
  }

  /* ------------------------------------------------------------------ clock */

  play() {
    if (this.playing || !this.project) return;
    const end = this.duration();
    if (end <= 0) return;
    if (this.time >= end - 1000) this.seek(0);
    this.playing = true;
    this.wallStart = performance.now();
    this.timeStart = this.time;
    // Start the media from inside the caller's gesture. Browsers refuse unmuted
    // playback without user activation; if that is ignored the clock keeps
    // running while every picture stays frozen.
    this.syncPlayback(this.time);
    this.raf = requestAnimationFrame(this.tick);
  }

  pause() {
    this.playing = false;
    cancelAnimationFrame(this.raf);
    for (const el of this.videos.values()) el.pause();
    this.requestDraw();
  }

  toggle() {
    this.playing ? this.pause() : this.play();
    return this.playing;
  }

  tick = () => {
    if (!this.playing) return;
    const end = this.duration();
    const t = this.timeStart + (performance.now() - this.wallStart) * 1000;
    if (t >= end) {
      this.time = end;
      this.pause();
      this.onTime(end);
      this.draw();
      return;
    }
    this.time = t;
    this.syncPlayback(t);
    this.draw();
    this.onTime(t);
    this.raf = requestAnimationFrame(this.tick);
  };

  seek(micros) {
    this.time = clamp(micros, 0, Math.max(0, this.duration()));
    if (this.playing) {
      // Re-anchor the clock so playback continues from the requested instant.
      this.wallStart = performance.now();
      this.timeStart = this.time;
    }
    // A pointer can move faster than the display refreshes. Seeking on every
    // event floods the decoder with targets it will never show, so the request
    // is coalesced to one per frame.
    if (this.seekScheduled) return;
    this.seekScheduled = true;
    requestAnimationFrame(() => {
      this.seekScheduled = false;
      this.syncPlayback(this.time);
      this.requestDraw();
    });
  }

  /* ------------------------------------------------------------- media sync */

  /** Returns the clips visible at `t`, bottom track first. */
  visibleClips(t) {
    const out = [];
    if (!this.project) return out;
    this.project.tracks.forEach((track, z) => {
      if (track.hidden) return;
      for (const clip of track.clips) {
        const start = clip.timeline.start;
        const end = start + clip.timeline.duration;
        if (t >= start && t < end) out.push({ clip, track, z });
      }
    });
    out.sort((a, b) => a.z - b.z || a.clip.timeline.start - b.clip.timeline.start);
    return out;
  }

  /** Where a clip's source should be at time `t` (seconds). */
  sourceTime(clip, t) {
    const into = (t - clip.timeline.start) / US;
    return clip.source.start / US + into * clip.speed;
  }

  syncPlayback(t) {
    const active = new Map();
    for (const { clip, track } of this.visibleClips(t)) {
      if (clip.text) continue;
      const asset = this.project.assets.find((a) => a.id === clip.asset_id);
      if (!asset || asset.kind === "image") continue;
      const el = this.videos.get(clip.asset_id);
      if (!el) continue;

      let offset = 0;
      if (asset.browser_playable === false && !asset.preview_path) {
        // The browser cannot decode this source: play the proxy segment that
        // covers the current instant, and treat its start as time zero.
        const source = this.sourceTime(clip, t);
        const bucket = Math.max(0, Math.floor(source / this.segmentSeconds));
        offset = bucket * this.segmentSeconds;
        const state = this.proxy.get(clip.asset_id);
        if (!state || state.index !== bucket) {
          this.requestSegment(asset, bucket);
          continue;
        }
        if (!state.url) continue; // still being prepared
      }
      active.set(clip.asset_id, { el, clip, track, offset });
    }

    for (const [id, el] of this.videos) {
      const entry = active.get(id);
      if (!entry) {
        if (!el.paused) el.pause();
        continue;
      }
      const { clip, track, offset } = entry;
      const expected = this.sourceTime(clip, t) - offset;
      el.volume = clamp(track.muted ? 0 : clip.volume, 0, 1);

      if (this.playing) {
        // Correct drift only when it is audible/visible, otherwise the element
        // would seek constantly and never play smoothly.
        if (Math.abs(el.currentTime - expected) > 0.3) el.currentTime = expected;
        if (this.sourceTime(clip, t) - offset > this.segmentSeconds - 3) {
          this.prefetchNext(clip.asset_id, this.sourceTime(clip, t));
        }
        if (el.paused) {
          const attempt = el.play();
          if (attempt?.catch) {
            attempt.catch(() => {
              // Report it once instead of failing silently forever.
              if (!this.blocked) {
                this.blocked = true;
                this.onBlocked?.();
              }
            });
          }
        }
      } else {
        // Half a frame at 30 fps: below that the element is already showing the
        // frame we would ask for.
        if (Math.abs(el.currentTime - expected) > 0.017) el.currentTime = expected;
        if (!el.paused) el.pause();
      }
    }
  }

  /** Ask for the proxy segment that covers `bucket`, once. */
  requestSegment(asset, bucket) {
    const key = `${asset.id}:${bucket}`;
    if (this.proxyUrls.has(key)) {
      const url = this.proxyUrls.get(key);
      this.proxy.set(asset.id, { index: bucket, url });
      const el = this.videos.get(asset.id);
      if (el && el.dataset.proxyKey !== key) {
        el.dataset.proxyKey = key;
        el.src = url;
      }
      return;
    }
    const state = this.proxy.get(asset.id);
    if (state?.loading) return;
    this.proxy.set(asset.id, { index: bucket, loading: true });
    Promise.resolve(this.onNeedProxy?.(asset.id, bucket))
      .then((url) => {
        if (!url) throw new Error("segment indisponible");
        this.proxyUrls.set(key, url);
        const el = this.videos.get(asset.id);
        if (el) {
          el.dataset.proxyKey = key;
          el.src = url;
        }
        this.proxy.set(asset.id, { index: bucket, url });
        this.requestDraw();
      })
      .catch(() => {
        this.proxy.set(asset.id, { index: bucket, error: true });
      });
  }

  /** Prefetch the next segment before playback reaches the edge. */
  prefetchNext(assetId, sourceSeconds) {
    const bucket = Math.floor(sourceSeconds / this.segmentSeconds) + 1;
    if (!this.onNeedProxy) return;
    const key = `${assetId}:${bucket}`;
    if (this.proxyUrls.has(key)) return;
    Promise.resolve(this.onNeedProxy(assetId, bucket))
      .then((url) => {
        if (url) this.proxyUrls.set(key, url);
      })
      .catch(() => {});
  }

  /* ------------------------------------------------------------------- draw */

  draw() {
    if (!this.project) return;
    const ctx = this.ctx;
    const { width, height } = this.project.canvas;
    const sx = this.canvas.width / width;
    const sy = this.canvas.height / height;
    const t = this.time;

    const visible = this.visibleClips(t);

    // If a video frame is still being decoded, keep the previous composite on
    // screen. Clearing first would flash the background for one or two frames,
    // which is exactly what makes scrubbing feel broken.
    for (const { clip } of visible) {
      if (clip.text) continue;
      const asset = this.project.assets.find((a) => a.id === clip.asset_id);
      if (!asset || asset.kind === "image") continue;
      const media = this.videos.get(asset.id);
      if (media && media.readyState < 2) return;
    }

    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = rgba(this.project.canvas.background);
    ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.restore();

    for (const { clip, track } of visible) {
      const alpha = this.clipAlpha(clip, t);
      if (alpha <= 0.001) continue;
      if (clip.text) this.drawText(clip, alpha, sx);
      else if (track.kind === "audio") continue;
      else this.drawClip(clip, alpha, sx, sy);
    }
  }

  clipAlpha(clip, t) {
    const duration = clip.timeline.duration;
    const into = t - clip.timeline.start;
    const fade = (value) => Math.min(value, duration / 2);
    let alpha = clamp(clip.opacity, 0, 1);
    const fin = fade(clip.fade_in);
    if (fin > 0) alpha *= clamp(into / fin, 0, 1);
    const fout = fade(clip.fade_out);
    if (fout > 0) alpha *= clamp((duration - into) / fout, 0, 1);
    return alpha;
  }

  drawClip(clip, alpha, sx, sy) {
    const ctx = this.ctx;
    const asset = this.project.assets.find((a) => a.id === clip.asset_id);
    if (!asset) return;
    const media = asset.kind === "image" ? this.images.get(asset.id) : this.videos.get(asset.id);
    if (!media) return;
    const mw = media.videoWidth || media.naturalWidth || 0;
    const mh = media.videoHeight || media.naturalHeight || 0;
    if (!mw || !mh || (media.readyState !== undefined && media.readyState < 2 && media.tagName === "VIDEO")) {
      return;
    }

    const cw = this.canvas.width;
    const ch = this.canvas.height;
    const tf = clip.transform;

    // Source rectangle: crop is expressed as fractions on each side.
    const cropX = mw * clamp(tf.crop_left, 0, 0.49);
    const cropY = mh * clamp(tf.crop_top, 0, 0.49);
    const cropW = mw * (1 - clamp(tf.crop_left, 0, 0.49) - clamp(tf.crop_right, 0, 0.49));
    const cropH = mh * (1 - clamp(tf.crop_top, 0, 0.49) - clamp(tf.crop_bottom, 0, 0.49));
    if (cropW <= 0 || cropH <= 0) return;

    // Contain inside the canvas, then apply the clip's scale — the same rule the
    // native renderer uses, so preview and export frame the shot identically.
    const fit = Math.min(cw / cropW, ch / cropH) * clamp(tf.scale, 0.01, 20);
    const dw = cropW * fit;
    const dh = cropH * fit;

    ctx.save();
    ctx.globalAlpha = alpha;
    const filter = canvasFilter(clip.effects);
    if (filter) ctx.filter = filter;
    ctx.translate(cw / 2 + tf.x * sx, ch / 2 + tf.y * sy);
    if (tf.rotation_deg) ctx.rotate((tf.rotation_deg * Math.PI) / 180);
    if (tf.mirror_x) ctx.scale(-1, 1);
    if (tf.mirror_y) ctx.scale(1, -1);
    try {
      ctx.drawImage(media, cropX, cropY, cropW, cropH, -dw / 2, -dh / 2, dw, dh);
    } catch {
      /* a frame not decoded yet */
    }
    ctx.restore();

    this.drawVignette(clip, dw, dh, cw / 2 + tf.x * sx, ch / 2 + tf.y * sy, alpha);
  }

  drawVignette(clip, dw, dh, cx, cy, alpha) {
    const amount = clip.effects
      .filter((e) => e.kind === "vignette")
      .reduce((acc, e) => Math.max(acc, clamp(e.amount, 0, Math.PI)), 0);
    if (!amount) return;
    const ctx = this.ctx;
    const radius = Math.max(dw, dh) * 0.62;
    const gradient = ctx.createRadialGradient(cx, cy, radius * 0.35, cx, cy, radius);
    gradient.addColorStop(0, "rgba(0,0,0,0)");
    gradient.addColorStop(1, `rgba(0,0,0,${Math.min(0.92, amount * 0.75)})`);
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.fillStyle = gradient;
    ctx.fillRect(cx - dw / 2, cy - dh / 2, dw, dh);
    ctx.restore();
  }

  drawText(clip, alpha, sx) {
    const text = clip.text;
    const ctx = this.ctx;
    const tf = clip.transform;
    const cw = this.canvas.width;
    const ch = this.canvas.height;
    const size = Math.max(4, text.font_size * sx);
    const lineHeight = size * 1.25;
    const weight = text.bold ? "600 " : "";
    const style = text.italic ? "italic " : "";
    const family = text.font_family?.trim() || "DejaVu Sans";
    ctx.save();
    ctx.font = `${style}${weight}${size}px "${family}", "DejaVu Sans", sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.globalAlpha = alpha;

    const lines = wrapLines(ctx, text.content ?? "", cw * 0.9);
    const blockHeight = lines.length * lineHeight;
    const x = cw / 2 + tf.x * sx;
    const y = ch / 2 + tf.y * sx;

    if (text.background) {
      let widest = 0;
      for (const line of lines) widest = Math.max(widest, ctx.measureText(line).width);
      const padX = size * 0.5;
      const padY = size * 0.28;
      ctx.fillStyle = rgba(text.background);
      roundRect(ctx, x - widest / 2 - padX, y - blockHeight / 2 - padY, widest + padX * 2, blockHeight + padY * 2, size * 0.18);
      ctx.fill();
    }

    ctx.fillStyle = rgba(text.color);
    lines.forEach((line, i) => {
      ctx.fillText(line, x, y - blockHeight / 2 + lineHeight * (i + 0.5));
    });
    ctx.restore();
  }

  destroy() {
    this.pause();
    this.observer.disconnect();
    for (const el of this.videos.values()) {
      el.removeAttribute("src");
      el.load();
      el.remove();
    }
    this.videos.clear();
    this.images.clear();
  }
}

/* ---------------------------------------------------------------- utilities */

function canvasFilter(effects) {
  let brightness = 1;
  let contrast = 1;
  let saturate = 1;
  let blur = 0;
  let hue = 0;
  let grayscale = 0;
  let sepia = 0;
  let invert = 0;
  for (const effect of effects ?? []) {
    switch (effect.kind) {
      case "brightness":
        brightness *= 1 + effect.amount;
        break;
      case "contrast":
        contrast *= 1 + effect.amount;
        break;
      case "saturation":
        saturate *= 1 + effect.amount;
        break;
      case "hue":
        hue += effect.degrees;
        break;
      case "blur":
        blur += effect.amount * 20;
        break;
      case "grayscale":
        grayscale = Math.max(grayscale, effect.amount);
        break;
      case "sepia":
        sepia = Math.max(sepia, effect.amount);
        break;
      case "invert":
        invert = Math.max(invert, effect.amount);
        break;
      case "gamma":
        // Approximated: canvas has no gamma filter.
        brightness *= 1 + (effect.amount - 1) * 0.3;
        break;
      case "temperature": {
        // Approximated with a warm/cool wash.
        hue += (effect.kelvin - 6500) / 60;
        saturate *= 1.05;
        break;
      }
      default:
        // `sharpen` and `chroma_key` have no canvas equivalent: the export
        // applies them, the preview does not pretend to.
        break;
    }
  }
  const parts = [];
  if (brightness !== 1) parts.push(`brightness(${brightness.toFixed(3)})`);
  if (contrast !== 1) parts.push(`contrast(${contrast.toFixed(3)})`);
  if (saturate !== 1) parts.push(`saturate(${saturate.toFixed(3)})`);
  if (hue) parts.push(`hue-rotate(${hue.toFixed(1)}deg)`);
  if (blur) parts.push(`blur(${blur.toFixed(2)}px)`);
  if (grayscale) parts.push(`grayscale(${grayscale.toFixed(3)})`);
  if (sepia) parts.push(`sepia(${sepia.toFixed(3)})`);
  if (invert) parts.push(`invert(${invert.toFixed(3)})`);
  return parts.length ? parts.join(" ") : "";
}

function wrapLines(ctx, content, maxWidth) {
  const out = [];
  for (const paragraph of String(content).split("\n")) {
    const words = paragraph.split(/\s+/).filter(Boolean);
    if (!words.length) {
      out.push("");
      continue;
    }
    let line = words[0];
    for (const word of words.slice(1)) {
      const candidate = `${line} ${word}`;
      if (ctx.measureText(candidate).width > maxWidth && line) {
        out.push(line);
        line = word;
      } else {
        line = candidate;
      }
    }
    out.push(line);
  }
  return out;
}

function rgba(color) {
  if (!color) return "rgba(0,0,0,1)";
  return `rgba(${color.r},${color.g},${color.b},${(color.a ?? 255) / 255})`;
}

function roundRect(ctx, x, y, w, h, r) {
  const radius = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}
