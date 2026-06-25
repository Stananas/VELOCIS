import { useCallback, useRef, useState } from 'react'
import { useProjectStore, type Project } from '../store/project'
import { Folder, Clapperboard, Pencil, Trash2 } from 'lucide-react'

function fmtDate(ts: number): string {
  const d = new Date(ts)
  const now = new Date()
  const opts: Intl.DateTimeFormatOptions = {
    day: 'numeric', month: 'short',
    hour: '2-digit', minute: '2-digit',
  }
  if (d.getFullYear() !== now.getFullYear()) opts.year = 'numeric'
  return d.toLocaleDateString('fr-FR', opts)
}

interface Props {
  onOpenProject: (id: string) => void
}

export default function Home({ onOpenProject }: Props) {
  const projects = useProjectStore(s => s.projects)
  const createProject = useProjectStore(s => s.createProject)
  const renameProject = useProjectStore(s => s.renameProject)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editName, setEditName] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  const handleNew = useCallback(() => {
    const id = createProject('Nouveau projet')
    onOpenProject(id)
  }, [createProject, onOpenProject])

  const handleOpen = useCallback((id: string) => {
    onOpenProject(id)
  }, [onOpenProject])

  const handleDelete = useCallback((e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    useProjectStore.setState(s => ({
      projects: s.projects.filter(p => p.id !== id),
    }))
  }, [])

  const startRename = useCallback((e: React.MouseEvent, p: Project) => {
    e.stopPropagation()
    setEditingId(p.id)
    setEditName(p.name)
    setTimeout(() => inputRef.current?.select(), 50)
  }, [])

  const commitRename = useCallback(() => {
    if (editingId && editName.trim()) {
      renameProject(editingId, editName.trim())
    }
    setEditingId(null)
  }, [editingId, editName, renameProject])

  return (
    <div className="flex flex-col h-screen bg-surface text-text font-sans">
      <header className="flex items-center gap-3 px-6 py-4 border-b border-border">
        <span className="font-bold text-xl text-accent tracking-tight">VELOCIS</span>
        <span className="text-xs text-text-muted">editeur video</span>
        <div className="flex-1" />
        <button
          onClick={handleNew}
          className="bg-accent text-white text-sm px-4 py-1.5 rounded font-medium cursor-pointer border-0 hover:opacity-90 transition-opacity"
        >
          + Nouveau projet
        </button>
      </header>

      <div className="flex-1 overflow-auto p-8">
        {projects.length === 0 ? (
          <div className="text-center pt-20 text-text-muted">
            <Clapperboard className="w-12 h-12 mx-auto mb-4 text-text-muted" />
            <p className="text-lg">Aucun projet</p>
            <p className="text-sm mt-1">Clique sur "Nouveau projet" pour commencer</p>
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4 max-w-4xl mx-auto">
            {projects.map(p => (
              <div
                key={p.id}
                onClick={() => handleOpen(p.id)}
                className="bg-surface-raised border border-border rounded-lg p-4 cursor-pointer hover:border-accent/50 transition-colors group relative"
              >
                <Folder className="w-8 h-8 mb-2 text-accent" />
                {editingId === p.id ? (
                  <input
                    ref={inputRef}
                    className="bg-surface text-text text-sm font-medium w-full rounded px-1 py-0.5 border border-accent outline-none"
                    value={editName}
                    onChange={e => setEditName(e.target.value)}
                    onBlur={commitRename}
                    onKeyDown={e => { if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') setEditingId(null) }}
                    autoFocus
                  />
                ) : (
                  <div className="text-sm font-medium truncate">{p.name}</div>
                )}
                <div className="text-[11px] text-text-muted mt-1">{fmtDate(p.createdAt)}</div>

                <div className="absolute top-2 right-2 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={e => startRename(e, p)}
                    className="bg-surface-hover text-text-muted text-[11px] px-1.5 py-0.5 rounded cursor-pointer border-0 hover:text-text"
                    title="Renommer"
                  >
                    <Pencil className="w-3 h-3" />
                  </button>
                  <button
                    onClick={e => handleDelete(e, p.id)}
                    className="bg-surface-hover text-red-400 text-[11px] px-1.5 py-0.5 rounded cursor-pointer border-0 hover:text-red-300"
                    title="Supprimer"
                  >
                    <Trash2 className="w-3 h-3" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
