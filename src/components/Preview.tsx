import { useRef, useState, useEffect, useCallback, memo } from 'react'
import { usePreviewRenderer } from '../hooks/usePreviewRenderer'
import { useTimelineStore } from '../store/timeline'
import { Play, Pause, Volume2, Maximize, Crop, Monitor, Ratio } from 'lucide-react'

interface Props {
  videoEl: HTMLVideoElement | null
}

function fmt(sec: number): string {
  const h = Math.floor(sec / 3600)
  const m = Math.floor((sec % 3600) / 60)
  const s = Math.floor(sec % 60)
  if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
  return `${m}:${s.toString().padStart(2, '0')}`
}

const Preview = memo(function Preview({ videoEl }: Props) {
  const [canvas, setCanvas] = useState<HTMLCanvasElement | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef(videoEl)
  videoRef.current = videoEl
  usePreviewRenderer(canvas, videoEl)

  useEffect(() => {
    const c = canvas
    if (!c || !containerRef.current) return

    const ro = new ResizeObserver(entries => {
      const entry = entries[0]
      if (!entry) return
      const { inlineSize: w, blockSize: h } = entry.contentBoxSize[0]
      if (w <= 0 || h <= 0) return

      const v = videoRef.current
      let ar = 16 / 9
      if (v && v.videoWidth > 0 && v.videoHeight > 0) {
        ar = v.videoWidth / v.videoHeight
      }

      let cw: number, ch: number
      if (w / h > ar) {
        ch = h
        cw = h * ar
      } else {
        cw = w
        ch = w / ar
      }

      const dpr = window.devicePixelRatio || 1
      cw = Math.min(Math.round(cw * dpr), 1920)
      ch = Math.min(Math.round(ch * dpr), 1080)

      if (c.width !== cw || c.height !== ch) {
        c.width = cw
        c.height = ch
      }
    })
    ro.observe(containerRef.current)
    return () => ro.disconnect()
  }, [canvas])

  const playing = useTimelineStore(s => s.playing)
  const setPlaying = useTimelineStore(s => s.setPlaying)
  const tlPlayhead = useTimelineStore(s => s.playhead)
  const tlTracks = useTimelineStore(s => s.tracks)

  const hasClip = tlTracks.flatMap(t => t.clips).length === 0
    ? false
    : useTimelineStore.getState().findClipByPlayhead(tlPlayhead) !== null
  const [volume, setVolume] = useState(1)
  const timeRef = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    if (!videoEl) {
      setPlaying(false)
      if (timeRef.current) timeRef.current.textContent = '0:00 / 0:00'
      return
    }

    const onPlay = () => setPlaying(true)
    const onPause = () => setPlaying(false)
    const onEnd = () => setPlaying(false)
    videoEl.addEventListener('play', onPlay)
    videoEl.addEventListener('pause', onPause)
    videoEl.addEventListener('ended', onEnd)

    let id = 0
    const tick = () => {
      const t = videoEl.currentTime
      const d = videoEl.duration || 0

      if (timeRef.current) {
        timeRef.current.textContent = `${fmt(t)} / ${fmt(d)}`
      }
      id = requestAnimationFrame(tick)
    }
    id = requestAnimationFrame(tick)

    return () => {
      cancelAnimationFrame(id)
      videoEl.removeEventListener('play', onPlay)
      videoEl.removeEventListener('pause', onPause)
      videoEl.removeEventListener('ended', onEnd)
    }
  }, [videoEl, setPlaying])

  const togglePlay = useCallback(() => {
    if (!videoEl) return
    if (videoEl.paused) videoEl.play()
    else videoEl.pause()
  }, [videoEl])

  const handleVolume = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const v = parseFloat(e.target.value)
    setVolume(v)
    if (videoEl) videoEl.volume = v
  }, [videoEl])

  return (
    <div className="flex-1 flex flex-col m-3 rounded-2xl overflow-hidden bg-surface-raised border border-border min-h-0">
      {/* Top preview toolbar */}
      <div className="flex items-center justify-between px-3 h-10 border-b border-border bg-surface-raised shrink-0">
        <div className="flex items-center gap-1">
          <button className="flex items-center gap-1.5 text-[11px] text-text-muted hover:text-text bg-transparent border-0 rounded-lg px-2 py-1 cursor-pointer hover:bg-surface-hover transition-colors">
            <Crop className="w-3.5 h-3.5" /> Recadrer
          </button>
          <button className="flex items-center gap-1.5 text-[11px] text-text-muted hover:text-text bg-transparent border-0 rounded-lg px-2 py-1 cursor-pointer hover:bg-surface-hover transition-colors">
            <Ratio className="w-3.5 h-3.5" /> 16:9
          </button>
        </div>
        <div className="flex items-center gap-1">
          <button className="flex items-center gap-1.5 text-[11px] text-text-muted hover:text-text bg-transparent border-0 rounded-lg px-2 py-1 cursor-pointer hover:bg-surface-hover transition-colors">
            <Monitor className="w-3.5 h-3.5" />
          </button>
          <button className="flex items-center gap-1.5 text-[11px] text-text-muted hover:text-text bg-transparent border-0 rounded-lg px-2 py-1 cursor-pointer hover:bg-surface-hover transition-colors">
            <Maximize className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Canvas area with corner handles */}
      <div ref={containerRef} className="flex-1 flex items-center justify-center relative overflow-hidden p-4">
        <div className="relative">
          <canvas
            ref={el => setCanvas(el)}
            width={640}
            height={360}
            className="rounded-xl max-w-full max-h-full object-contain shadow-2xl"
          />
          {!hasClip && (
            <div className="absolute inset-0 bg-surface/80 rounded-xl flex items-center justify-center">
              <span className="text-sm text-text-muted">Apercu video</span>
            </div>
          )}

          {/* Corner resize handles */}
          <div className="absolute -top-1 -left-1 w-3 h-3 bg-white border border-border rounded-sm shadow z-10" />
          <div className="absolute -top-1 -right-1 w-3 h-3 bg-white border border-border rounded-sm shadow z-10" />
          <div className="absolute -bottom-1 -left-1 w-3 h-3 bg-white border border-border rounded-sm shadow z-10" />
          <div className="absolute -bottom-1 -right-1 w-3 h-3 bg-white border border-border rounded-sm shadow z-10" />
          {/* Center bottom rotate handle */}
          <div className="absolute -bottom-3 left-1/2 -translate-x-1/2 w-2 h-2 bg-white rounded-full border border-border shadow z-10" />
        </div>
      </div>

      {/* Bottom controls */}
      <div className="flex items-center gap-3 px-4 h-11 border-t border-border bg-surface-raised shrink-0">
        <button
          onClick={togglePlay}
          className="w-8 h-8 flex items-center justify-center rounded-full bg-white text-surface font-bold cursor-pointer border-0 hover:scale-105 transition-transform"
        >
          {playing ? <Pause className="w-4 h-4 fill-current" /> : <Play className="w-4 h-4 fill-current ml-0.5" />}
        </button>

        <span
          ref={timeRef}
          className="tabular-nums whitespace-nowrap font-mono text-[11px] text-text-muted"
        >
          0:00 / 0:00
        </span>

        <div className="flex-1" />

        <div className="flex items-center gap-2">
          <Volume2 className="w-3.5 h-3.5 text-text-muted" />
          <input
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={volume}
            onChange={handleVolume}
            className="w-20"
          />
        </div>
      </div>
    </div>
  )
})

export default Preview
