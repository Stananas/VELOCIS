import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export interface MediaItem {
  id: string
  name: string
  type: 'video' | 'audio' | 'image'
  duration: number
  thumbnail?: string
  filePath?: string
}

interface MediaState {
  items: MediaItem[]
  selectedId: string | null
  importFile: (file: File) => Promise<void>
  selectMedia: (id: string | null) => void
  getUrl: (id: string) => string | null
  removeMedia: (id: string) => void
  restoreFiles: () => Promise<void>
}

function nanoid(): string { return Math.random().toString(36).slice(2, 9) }

function detectType(mime: string): MediaItem['type'] {
  if (mime.startsWith('video/')) return 'video'
  if (mime.startsWith('audio/')) return 'audio'
  if (mime.startsWith('image/')) return 'image'
  return 'video'
}

function extractThumbnail(el: HTMLVideoElement | HTMLImageElement): string {
  const c = document.createElement('canvas')
  c.width = 160; c.height = 90
  const ctx = c.getContext('2d')
  if (ctx) { try { ctx.drawImage(el, 0, 0, 160, 90); return c.toDataURL('image/jpeg', 0.6) } catch { return '' } }
  return ''
}

const urlMap = new Map<string, string>()

export const useMediaStore = create<MediaState>()(
  persist(
    (set) => ({
      items: [],
      selectedId: null,

      importFile: async (file: File) => {
        const id = nanoid()
        const type = detectType(file.type)
        const url = URL.createObjectURL(file)
        urlMap.set(id, url)

        const item: MediaItem = {
          id,
          name: file.name,
          type,
          duration: 0,
          filePath: (file as any).path || undefined,
        }

        if (type === 'video') {
          const video = document.createElement('video')
          video.preload = 'auto'; video.muted = true; video.playsInline = true; video.src = url
          document.body.appendChild(video)
          video.onloadedmetadata = () => {
            set((s: any) => ({ items: s.items.map((i: MediaItem) => i.id === id ? { ...i, duration: video.duration } : i) }))
            video.currentTime = Math.min(1, video.duration / 2)
            video.onseeked = () => {
              const thumb = extractThumbnail(video)
              if (thumb) set((s: any) => ({ items: s.items.map((i: MediaItem) => i.id === id ? { ...i, thumbnail: thumb } : i) }))
              video.remove()
            }
          }
          video.onerror = () => video.remove()
        } else if (type === 'image') {
          const img = new Image()
          img.onload = () => {
            const thumb = extractThumbnail(img)
            if (thumb) set((s: any) => ({ items: s.items.map((i: MediaItem) => i.id === id ? { ...i, thumbnail: thumb } : i) }))
          }
          img.src = url
        }

        set((s: any) => ({ items: [...s.items, item] }))
      },

      selectMedia: (id: string | null) => set({ selectedId: id }),

      getUrl: (id: string) => urlMap.get(id) || null,

      removeMedia: (id: string) => {
        const url = urlMap.get(id)
        if (url) { URL.revokeObjectURL(url); urlMap.delete(id) }
        set((s: any) => ({ items: s.items.filter((i: MediaItem) => i.id !== id), selectedId: s.selectedId === id ? null : s.selectedId }))
      },

      restoreFiles: async () => {
        const items = useMediaStore.getState().items
        const ea = (window as any).electronAPI
        if (!ea?.readFile) return
        for (const item of items) {
          if (urlMap.has(item.id)) continue
          if (!item.filePath) continue
          try {
            const buf = await ea.readFile(item.filePath)
            if (!buf) continue
            const file = new File([buf], item.name)
            const url = URL.createObjectURL(file)
            urlMap.set(item.id, url)
          } catch { }
        }
      },
    }),
    {
      name: 'velocis-media',
      partialize: (state) => ({
        items: state.items.map(i => ({ id: i.id, name: i.name, type: i.type, duration: i.duration, thumbnail: i.thumbnail, filePath: i.filePath })),
        selectedId: state.selectedId,
      }),
    },
  ),
)
