import { useEffect, useRef, useState } from 'react'
import Preview from './Preview'
import Timeline from './Timeline'
import MediaPanel from './MediaPanel'
import AppBar from './AppBar'
import { useTimelineStore } from '../store/timeline'
import { useMediaStore } from '../store/media'
import {
  FolderOpen, Video, Library, LayoutGrid, Type, Sparkles,
  Subtitles, Volume2, Blend, SlidersHorizontal, Wand2, Palette, Gauge,
} from 'lucide-react'

interface Props {
  onHome: () => void
}

const leftTabs = [
  { id: 'media', label: 'Medias', icon: FolderOpen },
  { id: 'record', label: 'Enregistrer', icon: Video },
  { id: 'library', label: 'Bibliotheque', icon: Library },
  { id: 'templates', label: 'Modeles', icon: LayoutGrid },
  { id: 'text', label: 'Texte', icon: Type },
  { id: 'transitions', label: 'Transitions', icon: Sparkles },
]

const rightTabs = [
  { id: 'captions', label: 'Legende', icon: Subtitles },
  { id: 'audio', label: 'Audio', icon: Volume2 },
  { id: 'fade', label: 'Fondu', icon: Blend },
  { id: 'filters', label: 'Filtres', icon: SlidersHorizontal },
  { id: 'effects', label: 'Effets', icon: Wand2 },
  { id: 'colors', label: 'Couleurs', icon: Palette },
  { id: 'speed', label: 'Vitesse', icon: Gauge },
]

export default function Editor({ onHome }: Props) {
  const [videoEl, setVideoEl] = useState<HTMLVideoElement | null>(null)
  const [isReady, setIsReady] = useState(false)
  const videoElRef = useRef<HTMLVideoElement | null>(null)
  videoElRef.current = videoEl

  const [leftTab, setLeftTab] = useState<string | null>('media')
  const [rightTab, setRightTab] = useState<string | null>(null)

  useEffect(() => {
    ;(window as any).electronAPI?.maximize()
    useMediaStore.getState().restoreFiles()
  }, [])

  useEffect(() => {
    if (videoEl) { setIsReady(true); return }

    const state = useTimelineStore.getState()
    const firstClip = state.tracks.flatMap(t => t.clips)[0]
    if (!firstClip || !firstClip.sourceMediaId) { setIsReady(true); return }

    const media = useMediaStore.getState().items.find(i => i.id === firstClip.sourceMediaId)
    if (!media) { setIsReady(true); return }

    const url = useMediaStore.getState().getUrl(media.id)
    if (!url) { setIsReady(true); return }
    const vid = document.createElement('video')
    vid.src = url
    vid.autoplay = true
    vid.loop = true
    vid.muted = true
    vid.playsInline = true
    Object.assign(vid.style, {
      position: 'absolute', width: '1px', height: '1px', opacity: '0',
      pointerEvents: 'none', overflow: 'hidden',
    })
    document.body.appendChild(vid)
    setVideoEl(vid)
    setIsReady(true)
  }, [videoEl])

  const tracks = useTimelineStore(s => s.tracks)
  const splitClip = useTimelineStore(s => s.splitClip)
  const removeClip = useTimelineStore(s => s.removeClip)

  useEffect(() => {
    if (!videoEl) return
    const state = useTimelineStore.getState()
    const ph = state.playhead
    const found = state.findClipByPlayhead(ph)
    if (!found) {
      const allClips = state.tracks.flatMap(t => t.clips)
      if (allClips.length > 0) {
        const nearest = allClips.reduce((a, b) => Math.abs(a.start - ph) < Math.abs(b.start - ph) ? a : b)
        const newPh = nearest.start
        useTimelineStore.getState().setPlayhead(newPh)
        videoEl.currentTime = nearest.sourceStart ?? newPh
      } else {
        videoEl.pause()
      }
    } else {
      videoEl.currentTime = (found.clip.sourceStart ?? found.clip.start) + (ph - found.clip.start)
    }
  }, [tracks, videoEl])

  const cutAtPlayhead = () => {
    const state = useTimelineStore.getState()
    const found = state.findClipByPlayhead(state.playhead)
    if (found && state.playhead > found.clip.start && state.playhead < found.clip.start + found.clip.duration) {
      splitClip(found.trackId, found.clip.id, state.playhead)
    }
  }

  const deleteAtPlayhead = () => {
    const state = useTimelineStore.getState()
    const selId = state.selectedClipId
    if (selId) {
      for (const track of state.tracks) {
        const clip = track.clips.find(c => c.id === selId)
        if (clip) { removeClip(track.id, clip.id); return }
      }
    }
    const found = state.findClipByPlayhead(state.playhead)
    if (found) removeClip(found.trackId, found.clip.id)
  }

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      if (e.code === 'Space') {
        e.preventDefault()
        const v = videoElRef.current
        if (v) { v.paused ? v.play() : v.pause() }
      }
      if (e.code === 'KeyS' && !e.ctrlKey && !e.metaKey) { e.preventDefault(); cutAtPlayhead() }
      if (e.code === 'Delete' || e.code === 'Backspace') { deleteAtPlayhead() }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  if (!isReady) {
    return (
      <div className="flex h-screen bg-surface text-text items-center justify-center">
        <div className="text-center">
          <div className="font-bold text-lg text-accent mb-2">VELOCIS</div>
          <div className="text-sm text-text-muted">Chargement...</div>
        </div>
      </div>
    )
  }

  const RightContent = () => {
    if (!rightTab) return null
    const tab = rightTabs.find(t => t.id === rightTab)
    return (
      <div className="w-56 bg-surface-raised border-l border-border flex flex-col">
        <div className="flex items-center justify-between px-3 py-2 border-b border-border">
          <span className="text-[11px] font-semibold text-text uppercase tracking-wide">{tab?.label}</span>
          <button onClick={() => setRightTab(null)} className="text-text-muted hover:text-text cursor-pointer bg-transparent border-0 p-0.5 text-[11px]">✕</button>
        </div>
        <div className="flex-1 p-3 text-[11px] text-text-muted">Aucune selection</div>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-screen bg-surface text-text font-sans">
      <AppBar onHome={onHome} videoEl={videoEl} />
      <div className="flex flex-1 overflow-hidden">
        {/* Left icon rail */}
        <div className="w-16 bg-surface-raised border-r border-border flex flex-col items-center py-2 gap-1 shrink-0 z-20">
          {leftTabs.map(tab => {
            const Icon = tab.icon
            const active = leftTab === tab.id
            return (
              <button
                key={tab.id}
                onClick={() => setLeftTab(active ? null : tab.id)}
                className={`w-14 flex flex-col items-center gap-1 rounded-xl px-1 py-2 cursor-pointer border-0 transition-colors ${
                  active ? 'bg-surface-active text-accent' : 'text-text-muted hover:bg-surface-hover hover:text-text'
                }`}
              >
                <Icon className="w-5 h-5" />
                <span className="text-[9px] font-medium leading-tight text-center">{tab.label}</span>
              </button>
            )
          })}
        </div>

        {/* Left panel content */}
        {leftTab && (
          <div className="w-56 bg-surface-raised border-r border-border flex flex-col shrink-0">
            {leftTab === 'media' ? (
              <MediaPanel videoEl={videoEl} setVideoEl={setVideoEl} />
            ) : (
              <>
                <div className="flex items-center justify-between px-3 py-2 border-b border-border">
                  <span className="text-[11px] font-semibold text-text uppercase tracking-wide">
                    {leftTabs.find(t => t.id === leftTab)?.label}
                  </span>
                  <button onClick={() => setLeftTab(null)} className="text-text-muted hover:text-text cursor-pointer bg-transparent border-0 p-0.5 text-[11px]">✕</button>
                </div>
                <div className="flex-1 p-3 text-[11px] text-text-muted">Bientot disponible</div>
              </>
            )}
          </div>
        )}

        {/* Main area */}
        <main className="flex flex-1 flex-col min-w-0">
          <Preview videoEl={videoEl} />
        </main>

        {/* Right panel content */}
        <RightContent />

        {/* Right icon rail */}
        <div className="w-16 bg-surface-raised border-l border-border flex flex-col items-center py-2 gap-1 shrink-0 z-20">
          {rightTabs.map(tab => {
            const Icon = tab.icon
            const active = rightTab === tab.id
            return (
              <button
                key={tab.id}
                onClick={() => setRightTab(active ? null : tab.id)}
                className={`w-14 flex flex-col items-center gap-1 rounded-xl px-1 py-2 cursor-pointer border-0 transition-colors ${
                  active ? 'bg-surface-active text-accent' : 'text-text-muted hover:bg-surface-hover hover:text-text'
                }`}
              >
                <Icon className="w-5 h-5" />
                <span className="text-[9px] font-medium leading-tight text-center">{tab.label}</span>
              </button>
            )
          })}
        </div>
      </div>
      <Timeline videoEl={videoEl} />
    </div>
  )
}
