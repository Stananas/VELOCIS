import { useCallback, useState } from 'react'
import { useProjectStore } from '../store/project'
import { useTimelineStore } from '../store/timeline'
import { Home, Plus, Download, ChevronDown } from 'lucide-react'

interface Props {
  onHome: () => void
  videoEl: HTMLVideoElement | null
}

export default function AppBar({ onHome, videoEl }: Props) {
  const currentId = useProjectStore(s => s.currentId)
  const projects = useProjectStore(s => s.projects)
  const createProject = useProjectStore(s => s.createProject)
  const current = currentId ? projects.find(p => p.id === currentId) : null
  const [exporting, setExporting] = useState(false)

  const handleExport = useCallback(async () => {
    if (exporting || !videoEl) return
    setExporting(true)

    try {
      const canvas = document.querySelector('canvas')
      if (!canvas) return

      const stream = canvas.captureStream(30)
      const totalDur = useTimelineStore.getState().totalDuration
      const mediaRecorder = new MediaRecorder(stream, { mimeType: 'video/webm' })
      const chunks: Blob[] = []

      mediaRecorder.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.push(e.data)
      }

      mediaRecorder.onstop = () => {
        const blob = new Blob(chunks, { type: 'video/webm' })
        const url = URL.createObjectURL(blob)
        const a = document.createElement('a')
        a.href = url
        a.download = (current?.name || 'export') + '.webm'
        a.click()
        URL.revokeObjectURL(url)
        setExporting(false)
      }

      mediaRecorder.start()
      videoEl.currentTime = 0
      videoEl.play()

      setTimeout(() => {
        videoEl.pause()
        mediaRecorder.stop()
        stream.getTracks().forEach(t => t.stop())
      }, (totalDur || 10) * 1000 + 500)
    } catch (e) {
      console.error('Export failed:', e)
      setExporting(false)
    }
  }, [exporting, videoEl, current])

  return (
    <header className="flex items-center gap-3 px-4 h-12 border-b border-border bg-surface shrink-0 select-none">
      <button
        onClick={onHome}
        className="font-bold text-sm text-accent cursor-pointer bg-transparent border-0 tracking-tight hover:opacity-80 transition-opacity flex items-center gap-1"
        title="Accueil"
      >
        VELOCIS
      </button>

      {current && (
        <button className="flex items-center gap-1 text-sm text-text bg-surface-raised hover:bg-surface-hover border border-border rounded-full px-3 py-1 cursor-pointer transition-colors">
          <span className="truncate max-w-[160px]">{current.name}</span>
          <ChevronDown className="w-3.5 h-3.5 text-text-muted" />
        </button>
      )}

      <div className="flex-1" />

      <button
        onClick={onHome}
        className="flex items-center gap-1.5 text-[12px] text-text-muted cursor-pointer bg-transparent border border-border hover:bg-surface-raised hover:text-text rounded-full px-3 py-1.5 transition-colors"
      >
        <Home className="w-3.5 h-3.5" /> Accueil
      </button>

      <button
        onClick={() => createProject('Nouveau projet')}
        className="flex items-center gap-1.5 text-[12px] text-text-muted cursor-pointer bg-transparent border border-border hover:bg-surface-raised hover:text-text rounded-full px-3 py-1.5 transition-colors"
      >
        <Plus className="w-3.5 h-3.5" /> Nouveau
      </button>

      {videoEl && (
        <button
          onClick={handleExport}
          disabled={exporting}
          className="bg-accent text-white text-[12px] px-4 py-1.5 rounded-full font-semibold cursor-pointer border-0 hover:brightness-110 transition-all disabled:opacity-50 flex items-center gap-1.5 shadow-sm shadow-accent/20"
        >
          <Download className="w-3.5 h-3.5" />
          {exporting ? 'Export...' : 'Exporter'}
        </button>
      )}
    </header>
  )
}
