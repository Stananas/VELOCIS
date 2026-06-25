import { useRef, useCallback, useState } from 'react'
import { useMediaStore } from '../store/media'
import { useTimelineStore } from '../store/timeline'
import { Film, Music, Image as ImageIcon, Plus, Trash2 } from 'lucide-react'

const clipColors = ['#2F5FEE', '#FF5C00', '#22C55E', '#8B5CF6', '#F59E0B', '#06B6D4']

function fmtDur(sec: number): string {
  if (!sec) return '--:--'
  const h = Math.floor(sec / 3600)
  const m = Math.floor((sec % 3600) / 60)
  const s = Math.floor(sec % 60)
  if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
  return `${m}:${s.toString().padStart(2, '0')}`
}

const typeIcon: Record<string, React.ReactNode> = {
  video: <Film className="w-5 h-5" />,
  audio: <Music className="w-5 h-5" />,
  image: <ImageIcon className="w-5 h-5" />,
}

interface Props {
  videoEl: HTMLVideoElement | null
  setVideoEl: (el: HTMLVideoElement | null) => void
}

export default function MediaPanel({ videoEl, setVideoEl }: Props) {
  const inputRef = useRef<HTMLInputElement>(null)
  const items = useMediaStore(s => s.items)
  const importFile = useMediaStore(s => s.importFile)
  const addClip = useTimelineStore(s => s.addClip)
  const colorIndex = useRef(0)
  const [dragOver, setDragOver] = useState(false)

  const handleImport = useCallback(() => { inputRef.current?.click() }, [])

  const handleFiles = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files) return
    for (let i = 0; i < files.length; i++) importFile(files[i])
    e.target.value = ''
  }, [importFile])

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDragOver(true)
  }, [])

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDragOver(false)
  }, [])

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setDragOver(false)
    const files = e.dataTransfer?.files
    if (!files) return
    for (let i = 0; i < files.length; i++) importFile(files[i])
  }, [importFile])

  const addToTimeline = useCallback(async (mediaId: string) => {
    const media = useMediaStore.getState().items.find(i => i.id === mediaId)
    if (!media || media.type !== 'video') return

    const old = videoEl
    if (old) { old.pause(); old.remove() }

    const url = useMediaStore.getState().getUrl(mediaId)
    if (!url) return

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

    vid.onloadedmetadata = () => {
      const color = clipColors[colorIndex.current % clipColors.length]
      colorIndex.current++
      addClip('v1', media.name, vid.duration || 10, color, mediaId)
    }
  }, [videoEl, setVideoEl, addClip])

  const handleRemoveMedia = useCallback((e: React.MouseEvent, mediaId: string) => {
    e.stopPropagation()
    useMediaStore.getState().removeMedia(mediaId)
  }, [])

  return (
    <aside className="flex flex-col h-full bg-surface-raised">
      <div className="p-3 border-b border-border space-y-2">
        <button
          onClick={handleImport}
          className="w-full text-left p-2.5 rounded-full cursor-pointer bg-accent text-white font-semibold text-[13px] border-0 hover:brightness-110 transition-all shadow-sm shadow-accent/20 flex items-center justify-center gap-2"
        >
          <Plus className="w-4 h-4" /> Importer
        </button>
        <p className="text-[10px] text-text-muted text-center">ou glisser-deposer</p>
        <input ref={inputRef} type="file" multiple accept="video/*,audio/*,image/*" className="hidden" onChange={handleFiles} />
      </div>

      <div
        className={`flex-1 overflow-auto p-2 space-y-2 transition-colors ${dragOver ? 'bg-accent/10' : ''}`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        {items.length === 0 && (
          <div className="text-[11px] text-text-muted text-center pt-6">Aucun media importe</div>
        )}

        {items.map(item => (
          <div
            key={item.id}
            className="rounded-xl overflow-hidden transition-colors group relative hover:ring-1 hover:ring-border bg-surface border border-border-subtle"
          >
            <div className="relative h-20 flex items-center justify-center">
              {item.thumbnail ? (
                <img src={item.thumbnail} alt="" className="w-full h-full object-cover" />
              ) : (
                <div className="text-text-muted">{typeIcon[item.type]}</div>
              )}
              <span className="absolute bottom-1.5 left-1.5 bg-black/70 text-white text-[10px] px-1.5 py-0.5 rounded-md font-mono tabular-nums">
                {fmtDur(item.duration)}
              </span>

              <button
                onClick={e => handleRemoveMedia(e, item.id)}
                className="absolute top-1.5 right-1.5 bg-red-500/90 text-white rounded-full w-6 h-6 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer border-0 hover:bg-red-500"
                title="Supprimer"
              >
                <Trash2 className="w-3 h-3" />
              </button>
            </div>
            <div className="px-2 py-2 flex items-center gap-1">
              <div className="text-[12px] truncate flex-1" title={item.name}>{item.name}</div>
              {item.type === 'video' && (
                <button
                  onClick={() => addToTimeline(item.id)}
                  className="text-accent hover:text-accent/80 transition-colors cursor-pointer bg-transparent border-0 p-0.5"
                  title="Ajouter a la timeline"
                >
                  <Plus className="w-4 h-4" />
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
    </aside>
  )
}
