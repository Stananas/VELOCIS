import { useEffect, useRef } from 'react'
import {
  initWasm,
  initWasmRenderer,
  renderWasmTextureFrame,
} from '../lib/wasm-bridge'

export function usePreviewRenderer(
  canvas: HTMLCanvasElement | null,
  video: HTMLVideoElement | null,
) {
  const ready = useRef(false)
  const texRef = useRef<WebGLTexture | null>(null)
  const videoRef = useRef(video)
  videoRef.current = video

  useEffect(() => {
    if (!canvas) return

    const gl = canvas.getContext('webgl2')
    if (!gl) return

    let animId = 0
    let cancelled = false

    initWasm().then(() => {
      if (cancelled) return

      initWasmRenderer(gl)
      ready.current = true

      const tex = gl.createTexture()
      texRef.current = tex

      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, tex)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)

      const render = () => {
        if (cancelled) return

        const currentVideo = videoRef.current
        if (currentVideo && currentVideo.readyState >= 2) {
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, currentVideo)
          renderWasmTextureFrame(gl, canvas.width, canvas.height)
        } else {
          gl.viewport(0, 0, canvas.width, canvas.height)
          gl.clearColor(0.02, 0.03, 0.04, 1.0)
          gl.clear(gl.COLOR_BUFFER_BIT)
        }

        animId = requestAnimationFrame(render)
      }
      animId = requestAnimationFrame(render)
    })

    return () => {
      cancelled = true
      cancelAnimationFrame(animId)
      if (texRef.current) {
        gl.deleteTexture(texRef.current)
        texRef.current = null
      }
      ready.current = false
    }
  }, [canvas, video])
}
