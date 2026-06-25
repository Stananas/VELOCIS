import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export interface Project {
  id: string
  name: string
  createdAt: number
}

interface ProjectState {
  projects: Project[]
  currentId: string | null
  createProject: (name: string) => string
  selectProject: (id: string | null) => void
  renameProject: (id: string, name: string) => void
}

function nanoid(): string {
  return Math.random().toString(36).slice(2, 9)
}

export const useProjectStore = create<ProjectState>()(
  persist(
    (set) => ({
      projects: [],
      currentId: null,

      createProject: (name: string) => {
        const id = nanoid()
        const project: Project = { id, name, createdAt: Date.now() }
        set(s => ({ projects: [...s.projects, project], currentId: id }))
        return id
      },

      selectProject: (id: string | null) => {
        set({ currentId: id })
      },

      renameProject: (id: string, name: string) => {
        set(s => ({
          projects: s.projects.map(p => p.id === id ? { ...p, name } : p),
        }))
      },
    }),
    { name: 'velocis-projects' },
  ),
)
