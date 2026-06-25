import init, {
  init_renderer,
  init_texture_renderer,
  render_frame,
  render_texture_frame,
} from './wasm/velocis_wasm'

let initialized = false

export async function initWasm() {
  if (initialized) return
  await init()
  initialized = true
}

export function initWasmRenderer(gl: WebGL2RenderingContext) {
  init_renderer(gl)
  init_texture_renderer(gl)
}

export function renderWasmFrame(gl: WebGL2RenderingContext, width: number, height: number, time: number) {
  render_frame(gl, width, height, time)
}

export function renderWasmTextureFrame(gl: WebGL2RenderingContext, width: number, height: number) {
  render_texture_frame(gl, width, height)
}
