/* ============================================================================
   Velocis — édition directe dans l'aperçu
   ----------------------------------------------------------------------------
   The stage is a canvas, so direct manipulation is done with a DOM overlay
   positioned exactly over it: a selection box, four corner handles for scale, a
   rotation handle, and four edge handles in crop mode.

   During a gesture the transform is written into the local project and the
   canvas is redrawn, so the picture follows the pointer. Only the final value is
   sent to the service, which keeps the undo history to one entry per gesture
   instead of one per mouse move.
   ========================================================================= */

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

export class StageManipulator {
  constructor({ canvas, overlay, box, getProject, getSelection, onLive, onCommit }) {
    this.canvas = canvas;
    this.overlay = overlay;
    this.box = box;
    this.getProject = getProject;
    this.getSelection = getSelection;
    this.onLive = onLive ?? (() => {});
    this.onCommit = onCommit ?? (() => {});
    this.drag = null;
    this.cropMode = false;

    this.overlay.addEventListener("pointerdown", (event) => this.start(event));
    this.overlay.addEventListener("pointermove", (event) => this.move(event));
    this.overlay.addEventListener("pointerup", () => this.end());
    this.overlay.addEventListener("pointercancel", () => this.end());

    // The canvas is resized by layout changes (panels, window), so the box has
    // to follow it.
    this.observer = new ResizeObserver(() => this.refresh());
    this.observer.observe(canvas);
  }

  destroy() {
    this.observer.disconnect();
  }

  setCropMode(enabled) {
    this.cropMode = enabled;
    this.refresh();
  }

  /* --------------------------------------------------------------- geometry */

  /** Screen-space geometry of the selected clip, in overlay pixels. */
  geometry() {
    const selection = this.getSelection();
    const project = this.getProject();
    if (!selection || !project) return null;
    const { clip } = selection;
    if (clip.text) return null;

    const asset = project.assets.find((a) => a.id === clip.asset_id);
    if (!asset || !asset.width || !asset.height) return null;

    const dispW = this.canvas.clientWidth;
    const dispH = this.canvas.clientHeight;
    if (!dispW || !dispH) return null;

    const sx = dispW / project.canvas.width;
    const sy = dispH / project.canvas.height;
    const tf = clip.transform;
    // Same rule as the renderers: contain the full frame, then crop, then scale.
    const fit =
      Math.min(project.canvas.width / asset.width, project.canvas.height / asset.height) *
      clamp(tf.scale, 0.01, 20);

    return {
      clip,
      asset,
      sx,
      sy,
      dispW,
      dispH,
      width: asset.width * fit * sx,
      height: asset.height * fit * sy,
      cx: dispW / 2 + tf.x * sx,
      cy: dispH / 2 + tf.y * sy,
    };
  }

  /** Place the overlay and the box over the selected clip. */
  refresh() {
    const geometry = this.geometry();
    if (!geometry) {
      this.overlay.hidden = true;
      return;
    }
    const { canvas } = this;
    const tf = geometry.clip.transform;

    this.overlay.hidden = false;
    this.overlay.style.left = `${canvas.offsetLeft}px`;
    this.overlay.style.top = `${canvas.offsetTop}px`;
    this.overlay.style.width = `${canvas.clientWidth}px`;
    this.overlay.style.height = `${canvas.clientHeight}px`;
    this.overlay.dataset.crop = this.cropMode ? "true" : "false";

    this.box.style.width = `${geometry.width}px`;
    this.box.style.height = `${geometry.height}px`;
    this.box.style.left = `${geometry.cx - geometry.width / 2}px`;
    this.box.style.top = `${geometry.cy - geometry.height / 2}px`;
    this.box.style.transform = `rotate(${tf.rotation_deg}deg)`;
    this.box.style.setProperty("--crop-left", `${(tf.crop_left * 100).toFixed(2)}%`);
    this.box.style.setProperty("--crop-right", `${(tf.crop_right * 100).toFixed(2)}%`);
    this.box.style.setProperty("--crop-top", `${(tf.crop_top * 100).toFixed(2)}%`);
    this.box.style.setProperty("--crop-bottom", `${(tf.crop_bottom * 100).toFixed(2)}%`);
  }

  /* ------------------------------------------------------------------ drags */

  start(event) {
    const geometry = this.geometry();
    if (!geometry) return;
    const target = event.target;
    const handle = target.closest("[data-handle]");
    const edge = target.closest("[data-edge]");
    const rotate = target.closest("[data-rotate]");
    const insideBox = target.closest(".sel-box");
    if (!insideBox) return;

    let mode = "move";
    if (rotate) mode = "rotate";
    else if (handle) mode = "scale";
    else if (edge) {
      if (!this.cropMode) return;
      mode = "crop";
    }

    const rect = this.overlay.getBoundingClientRect();
    const px = event.clientX - rect.left;
    const py = event.clientY - rect.top;

    this.drag = {
      mode,
      edge: edge?.dataset.edge,
      startX: px,
      startY: py,
      transform: { ...geometry.clip.transform },
      cx: geometry.cx,
      cy: geometry.cy,
      width: geometry.width,
      height: geometry.height,
      distance: Math.max(1, Math.hypot(px - geometry.cx, py - geometry.cy)),
      angle: (Math.atan2(py - geometry.cy, px - geometry.cx) * 180) / Math.PI,
      sx: geometry.sx,
      sy: geometry.sy,
    };

    try {
      this.overlay.setPointerCapture(event.pointerId);
    } catch {
      /* synthetic or released pointer */
    }
    event.preventDefault();
    event.stopPropagation();
  }

  move(event) {
    const drag = this.drag;
    if (!drag) return;
    const rect = this.overlay.getBoundingClientRect();
    const px = event.clientX - rect.left;
    const py = event.clientY - rect.top;
    const tf = drag.transform;
    const clip = this.getSelection()?.clip;
    if (!clip) return;

    if (drag.mode === "move") {
      const x = tf.x + (px - drag.startX) / drag.sx;
      const y = tf.y + (py - drag.startY) / drag.sy;
      clip.transform = { ...clip.transform, x: Math.round(x), y: Math.round(y) };
    } else if (drag.mode === "scale") {
      const distance = Math.max(1, Math.hypot(px - drag.cx, py - drag.cy));
      const scale = clamp((tf.scale * distance) / drag.distance, 0.05, 4);
      clip.transform = { ...clip.transform, scale: Number(scale.toFixed(3)) };
    } else if (drag.mode === "rotate") {
      const angle = (Math.atan2(py - drag.cy, px - drag.cx) * 180) / Math.PI;
      let rotation = tf.rotation_deg + (angle - drag.angle);
      if (event.shiftKey) rotation = Math.round(rotation / 15) * 15;
      // Keep the value readable and inside the model's range.
      rotation = ((rotation + 180) % 360 + 360) % 360 - 180;
      clip.transform = { ...clip.transform, rotation_deg: Math.round(rotation * 10) / 10 };
    } else if (drag.mode === "crop") {
      const dx = (px - drag.startX) / drag.width;
      const dy = (py - drag.startY) / drag.height;
      const min = 0.05;
      const crop = { ...tf };
      if (drag.edge === "w") {
        crop.crop_left = clamp(tf.crop_left + dx, 0, 1 - tf.crop_right - min);
      } else if (drag.edge === "e") {
        crop.crop_right = clamp(tf.crop_right - dx, 0, 1 - tf.crop_left - min);
      } else if (drag.edge === "n") {
        crop.crop_top = clamp(tf.crop_top + dy, 0, 1 - tf.crop_bottom - min);
      } else if (drag.edge === "s") {
        crop.crop_bottom = clamp(tf.crop_bottom - dy, 0, 1 - tf.crop_top - min);
      }
      clip.transform = crop;
    }

    this.onLive();
    this.refresh();
  }

  end() {
    const drag = this.drag;
    this.drag = null;
    if (!drag) return;
    const clip = this.getSelection()?.clip;
    if (!clip) return;
    this.onCommit({ ...clip.transform });
  }
}
