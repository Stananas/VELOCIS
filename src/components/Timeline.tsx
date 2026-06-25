import { useRef, useCallback, useEffect, useState, memo } from 'react'
import { useTimelineStore } from '../store/timeline'
import { useMediaStore } from '../store/media'
import { Maximize, X, Film, Music, Type, Scissors, Trash2 } from 'lucide-react'

const trackHeight = 52
const labelWidth = 44

interface Props { videoEl: HTMLVideoElement | null }

type CtxMenu =
  | { type: 'clip'; x: number; y: number; trackId: string; clipId: string }
  | { type: 'gap'; x: number; y: number; trackId: string; gapStart: number; gapEnd: number }
  | null

function findGaps(clips: { start: number; duration: number }[]): { start: number; end: number }[] {
  if (clips.length === 0) return []
  const sorted = [...clips].sort((a, b) => a.start - b.start)
  const gaps: { start: number; end: number }[] = []
  if (sorted[0].start > 0.1) gaps.push({ start: 0, end: sorted[0].start })
  for (let i = 1; i < sorted.length; i++) {
    const prevEnd = sorted[i - 1].start + sorted[i - 1].duration
    if (sorted[i].start > prevEnd + 0.1) gaps.push({ start: prevEnd, end: sorted[i].start })
  }
  return gaps
}

function fmtTime(sec: number): string {
  const m = Math.floor(sec / 60)
  const s = Math.floor(sec % 60)
  const ms = Math.floor((sec % 1) * 100)
  return `${m}:${s.toString().padStart(2, '0')}.${ms.toString().padStart(2, '0')}`
}

const trackIcons: Record<string, React.ElementType> = {
  video: Film,
  audio: Music,
  text: Type,
}

const Timeline = memo(function Timeline({ videoEl }: Props) {
  const tracks = useTimelineStore(s => s.tracks)
  const zoom = useTimelineStore(s => s.zoom)
  const totalDuration = useTimelineStore(s => s.totalDuration)
  const selectedClipId = useTimelineStore(s => s.selectedClipId)
  const playhead = useTimelineStore(s => s.playhead)
  const setPlayhead = useTimelineStore(s => s.setPlayhead)
  const setPlaying = useTimelineStore(s => s.setPlaying)
  const setZoom = useTimelineStore(s => s.setZoom)
  const moveClip = useTimelineStore(s => s.moveClip)
  const splitClip = useTimelineStore(s => s.splitClip)
  const removeClip = useTimelineStore(s => s.removeClip)
  const selectClip = useTimelineStore(s => s.selectClip)
  const closeGap = useTimelineStore(s => s.closeGap)
  const playheadRef = useRef<HTMLDivElement>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{ clipId: string; startX: number; origStart: number } | null>(null)
  const [ctxMenu, setCtxMenu] = useState<CtxMenu>(null)

  const mediaItems = useMediaStore(s => s.items)
  const getThumb = useCallback((clipName: string) => {
    const m = mediaItems.find(i => i.name === clipName)
    return m?.thumbnail || null
  }, [mediaItems])

  useEffect(() => {
    if (!videoEl) return
    let id = 0
    let lastTick = performance.now()
    const seekThreshold = 0.15

    const tick = (now: number) => {
      const dt = (now - lastTick) / 1000
      lastTick = now

      const s = useTimelineStore.getState()
      let tlTime = s.playhead

      if (s.playing) {
        tlTime += dt
        if (tlTime > s.totalDuration) {
          tlTime = s.totalDuration
          s.setPlaying(false)
          videoEl.pause()
        }
        s.setPlayhead(tlTime)
      }

      const found = s.findClipByPlayhead(tlTime)
      if (found && s.playing) {
        const targetSrc = (found.clip.sourceStart ?? found.clip.start) + (tlTime - found.clip.start)
        if (Math.abs(videoEl.currentTime - targetSrc) > seekThreshold) {
          videoEl.currentTime = targetSrc
        }
      } else if (!found && s.playing) {
        videoEl.pause()
        s.setPlaying(false)
      }

      if (playheadRef.current) playheadRef.current.style.left = labelWidth + tlTime * zoom + 'px'
      id = requestAnimationFrame(tick)
    }
    id = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(id)
  }, [videoEl, zoom])

  useEffect(() => {
    if (!ctxMenu) return
    const close = () => setCtxMenu(null)
    window.addEventListener('click', close)
    window.addEventListener('keydown', close)
    return () => { window.removeEventListener('click', close); window.removeEventListener('keydown', close) }
  }, [ctxMenu])

  const handleAutoFit = useCallback(() => {
    const dur = useTimelineStore.getState().totalDuration
    if (dur <= 0) return
    const container = scrollRef.current
    if (!container) return
    const w = container.offsetWidth - labelWidth - 16
    if (w <= 0) return
    setZoom(Math.max(1, Math.min(500, w / dur)))
  }, [setZoom])

  const totalWidth = totalDuration * zoom + labelWidth

  const seekTo = useCallback((tlTime: number) => {
    tlTime = Math.max(0, tlTime)
    setPlayhead(tlTime)
    if (playheadRef.current) playheadRef.current.style.left = labelWidth + tlTime * zoom + 'px'
    if (videoEl) {
      const src = useTimelineStore.getState().getSourceTime(tlTime)
      videoEl.currentTime = src ?? tlTime
    }
  }, [zoom, setPlayhead, videoEl])

  const handleRulerClick = useCallback((e: React.MouseEvent) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const time = Math.max(0, (e.clientX - rect.left - labelWidth) / zoom)
    if (videoEl) videoEl.pause()
    setPlaying(false)
    seekTo(time)
    selectClip(null)
  }, [zoom, seekTo, selectClip, videoEl, setPlaying])

  const handleWheel = useCallback((e: React.WheelEvent) => {
    if (e.ctrlKey || e.metaKey) { e.preventDefault(); setZoom(zoom + zoom * (-e.deltaY * 0.05)) }
  }, [zoom, setZoom])

  const handleClipClick = useCallback((e: React.MouseEvent, clipId: string) => {
    e.stopPropagation()
    selectClip(clipId)
    const cl = useTimelineStore.getState().tracks.flatMap(t => t.clips).find(c => c.id === clipId)
    if (cl) {
      if (videoEl) videoEl.pause()
      setPlaying(false)
      seekTo(cl.start)
    }
  }, [selectClip, seekTo, videoEl, setPlaying])

  const handleClipContext = useCallback((e: React.MouseEvent, trackId: string, clipId: string) => {
    e.preventDefault(); e.stopPropagation()
    selectClip(clipId)
    setCtxMenu({ type: 'clip', x: e.clientX, y: e.clientY, trackId, clipId })
  }, [selectClip])

  const handleGapContext = useCallback((e: React.MouseEvent, trackId: string, gapStart: number, gapEnd: number) => {
    e.preventDefault(); e.stopPropagation()
    setCtxMenu({ type: 'gap', x: e.clientX, y: e.clientY, trackId, gapStart, gapEnd })
  }, [])

  const handleContextDelete = useCallback(() => {
    if (!ctxMenu || ctxMenu.type !== 'clip') return
    removeClip(ctxMenu.trackId, ctxMenu.clipId); selectClip(null); setCtxMenu(null)
  }, [ctxMenu, removeClip, selectClip])

  const handleContextSplit = useCallback(() => {
    if (!ctxMenu || ctxMenu.type !== 'clip') return
    const t = useTimelineStore.getState().playhead
    splitClip(ctxMenu.trackId, ctxMenu.clipId, t); selectClip(null); setCtxMenu(null)
  }, [ctxMenu, splitClip, selectClip])

  const handleContextCloseGap = useCallback(() => {
    if (!ctxMenu || ctxMenu.type !== 'gap') return
    closeGap(ctxMenu.trackId, ctxMenu.gapStart, ctxMenu.gapEnd); setCtxMenu(null)
  }, [ctxMenu, closeGap])

  const handleClipMouseDown = useCallback((e: React.MouseEvent, clipId: string, origStart: number) => {
    if (e.button !== 0) return
    e.stopPropagation()
    selectClip(clipId)
    dragRef.current = { clipId, startX: e.clientX, origStart }
    const handleMove = (me: MouseEvent) => {
      if (!dragRef.current) return
      moveClip(dragRef.current.clipId, Math.max(0, dragRef.current.origStart + (me.clientX - dragRef.current.startX) / zoom))
    }
    const handleUp = () => { dragRef.current = null; document.removeEventListener('mousemove', handleMove); document.removeEventListener('mouseup', handleUp) }
    document.addEventListener('mousemove', handleMove)
    document.addEventListener('mouseup', handleUp)
  }, [zoom, moveClip, selectClip])

  const step = zoom < 2 ? 300 : zoom < 4 ? 60 : zoom < 10 ? 30 : zoom < 40 ? 10 : zoom < 100 ? 5 : 1
  const rulerMarkers: number[] = []
  for (let t = 0; t <= totalDuration; t += step) rulerMarkers.push(t)

  return (
    <div className="h-56 bg-surface-raised border-t border-border flex flex-col select-none relative">
      {ctxMenu && (
        <div className="fixed z-50 bg-surface-hover border border-border rounded-xl shadow-xl py-1 text-xs overflow-hidden" style={{ left: ctxMenu.x, top: ctxMenu.y }}>
          {ctxMenu.type === 'clip' && (
            <>
              <button onClick={handleContextSplit} className="flex items-center gap-2 w-full text-left px-3 py-1.5 cursor-pointer bg-transparent border-0 text-text hover:bg-surface-active"><Scissors className="w-3 h-3" /> Couper ici (S)</button>
              <button onClick={handleContextDelete} className="flex items-center gap-2 w-full text-left px-3 py-1.5 cursor-pointer bg-transparent border-0 text-red-400 hover:bg-surface-active"><Trash2 className="w-3 h-3" /> Supprimer (Del)</button>
            </>
          )}
          {ctxMenu.type === 'gap' && (
            <button onClick={handleContextCloseGap} className="flex items-center gap-2 w-full text-left px-3 py-1.5 cursor-pointer bg-transparent border-0 text-text hover:bg-surface-active"><X className="w-3 h-3" /> Fermer le trou</button>
          )}
        </div>
      )}

      <div ref={scrollRef} className="flex-1 overflow-auto" onWheel={handleWheel} onClick={() => selectClip(null)}>
        <div style={{ width: totalWidth, minWidth: '100%', position: 'relative' }}>
          {/* Ruler */}
          <div className="h-7 flex items-end border-b border-border text-[10px] text-text-muted sticky top-0 bg-surface-raised z-10 cursor-pointer" onClick={handleRulerClick}>
            <div className="w-11 shrink-0 text-center border-r border-border leading-7 bg-surface-raised" />
            <div className="flex-1 relative h-full">
              {rulerMarkers.map(t => (
                <div key={t} className="absolute bottom-0 flex flex-col items-start" style={{ left: t * zoom }}>
                  <div className="border-l border-border h-2" />
                  <span className="ml-1 -mt-0.5">{step >= 60 ? `${Math.floor(t / 60)}m${t % 60 > 0 ? t % 60 + 's' : ''}` : `${t}s`}</span>
                </div>
              ))}
            </div>
          </div>

          {tracks.map(track => {
            const gaps = findGaps(track.clips)
            const Icon = trackIcons[track.kind] || Film
            const isTextTrack = track.kind === 'text'
            const isAudioTrack = track.kind === 'audio'
            const hasClips = track.clips.length > 0

            return (
              <div key={track.id} className="flex border-b border-border/50 relative">
                <div className="w-11 shrink-0 flex flex-col items-center justify-center gap-0.5 text-[9px] font-semibold text-text-muted border-r border-border bg-surface-raised">
                  <Icon className="w-3.5 h-3.5" />
                  {track.name}
                </div>
                <div className="relative" style={{ height: trackHeight, flex: 1 }}>
                  {/* Empty track backgrounds */}
                  {!hasClips && isTextTrack && (
                    <div className="absolute inset-1 rounded-xl dotted-area opacity-40 flex items-center px-3">
                      <span className="text-[11px] text-text-muted">Ajouter du texte</span>
                    </div>
                  )}
                  {!hasClips && isAudioTrack && (
                    <div className="absolute inset-1 rounded-xl dashed-track opacity-30 flex items-center px-3">
                      <span className="text-[11px] text-text-muted">Ajouter du son</span>
                    </div>
                  )}

                  {track.clips.map(clip => {
                    const thumb = getThumb(clip.name)
                    const isSelected = clip.id === selectedClipId
                    return (
                      <div key={clip.id}
                        className={`absolute flex items-center px-2 text-xs font-semibold text-white rounded-xl overflow-hidden cursor-pointer transition-all border border-white/10 ${
                          isSelected ? 'ring-2 ring-accent ring-offset-1 ring-offset-transparent opacity-100' : 'opacity-90 hover:opacity-100 hover:ring-1 hover:ring-white/40'
                        }`}
                        style={{ left: clip.start * zoom, width: clip.duration * zoom, height: trackHeight - 8, top: 4, background: clip.color, minWidth: 16 }}
                        onClick={e => handleClipClick(e, clip.id)}
                        onContextMenu={e => handleClipContext(e, track.id, clip.id)}
                        onMouseDown={e => handleClipMouseDown(e, clip.id, clip.start)}
                      >
                        {thumb && clip.duration * zoom > 40 && <img src={thumb} alt="" className="h-full w-auto object-cover rounded-lg mr-1.5" />}
                        <span className="truncate drop-shadow-sm">{clip.name}</span>
                      </div>
                    )
                  })}

                  {gaps.map((gap, gi) => (
                    <div key={`gap-${gi}`} className="absolute top-1 bottom-1 flex items-center justify-center cursor-pointer group rounded-xl striped-gap border border-transparent hover:border-border" style={{ left: gap.start * zoom, width: (gap.end - gap.start) * zoom }}
                      onClick={e => { e.stopPropagation(); closeGap(track.id, gap.start, gap.end) }}
                      onContextMenu={e => handleGapContext(e, track.id, gap.start, gap.end)}
                    >
                      <div className="w-full h-px bg-border group-hover:bg-accent/50 transition-colors" />
                      <button className="absolute bg-surface-hover border border-border rounded-full w-5 h-5 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer hover:bg-surface-active" title="Fermer le trou"><X className="w-3 h-3 text-text-muted" /></button>
                    </div>
                  ))}
                </div>
              </div>
            )
          })}

          {/* Playhead */}
          <div ref={playheadRef} className="absolute top-0 bottom-0 z-20 pointer-events-none" style={{ left: labelWidth }}>
            <div className="relative h-full">
              <div className="absolute top-0 left-1/2 -translate-x-1/2 w-0 h-0 border-l-[5px] border-l-transparent border-r-[5px] border-r-transparent border-t-[6px] border-t-accent" />
              <div className="absolute top-1.5 bottom-0 left-1/2 -translate-x-1/2 w-px bg-accent" />
              <div className="absolute top-6 left-1/2 -translate-x-1/2 bg-accent text-white text-[9px] font-semibold px-1.5 py-0.5 rounded-md whitespace-nowrap">
                {fmtTime(playhead)}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Zoom controls */}
      <div className="absolute bottom-2 right-3 z-30 flex items-center gap-1 bg-surface border border-border rounded-full px-1.5 py-1 shadow-lg">
        <button onClick={() => setZoom(zoom * 0.8)} className="hover:bg-surface-hover text-text text-[13px] w-6 h-6 rounded-full cursor-pointer border-0 flex items-center justify-center">−</button>
        <span className="text-[10px] text-text-muted min-w-[4ch] text-center tabular-nums">{Math.round(zoom)}</span>
        <button onClick={() => setZoom(zoom * 1.25)} className="hover:bg-surface-hover text-text text-[13px] w-6 h-6 rounded-full cursor-pointer border-0 flex items-center justify-center">+</button>
        <button onClick={handleAutoFit} className="hover:bg-surface-hover text-text text-[10px] px-2 h-6 rounded-full cursor-pointer border-0 ml-0.5 flex items-center gap-1"><Maximize className="w-3 h-3" /> Fit</button>
      </div>
    </div>
  )
})

export default Timeline
