import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export interface ClipData {
  id: string
  name: string
  start: number
  duration: number
  color: string
  trackId: string
  sourceMediaId?: string
  sourceStart?: number
}

export interface TrackData {
  id: string
  name: string
  kind: 'video' | 'audio' | 'text'
  clips: ClipData[]
}

interface TimelineState {
  tracks: TrackData[]
  playhead: number
  zoom: number
  totalDuration: number
  selectedClipId: string | null
  playing: boolean

  setPlayhead: (time: number) => void
  setPlaying: (p: boolean) => void
  setZoom: (zoom: number) => void
  moveClip: (clipId: string, newStart: number) => void
  resizeClip: (clipId: string, newDuration: number) => void
  addClip: (trackId: string, name: string, duration: number, color: string, sourceMediaId?: string) => void
  splitClip: (trackId: string, clipId: string, splitTime: number) => void
  removeClip: (trackId: string, clipId: string) => void
  selectClip: (clipId: string | null) => void
  findClipByPlayhead: (time: number) => { trackId: string; clip: ClipData } | null
  findClipBySourceTime: (sourceTime: number) => { trackId: string; clip: ClipData } | null
  getSourceTime: (timelineTime: number) => number | null
  closeGap: (trackId: string, gapStart: number, gapEnd: number) => void
}

function nanoid(): string {
  return Math.random().toString(36).slice(2, 9)
}

function calcDuration(tracks: TrackData[]): number {
  return tracks.reduce(
    (max, t) => Math.max(max, t.clips.reduce((m, c) => Math.max(m, c.start + c.duration), 0)),
    0,
  )
}

const defaultTracks: TrackData[] = [
  { id: 'v1', name: 'V1', kind: 'video', clips: [] },
  { id: 'a1', name: 'A1', kind: 'audio', clips: [] },
  { id: 't1', name: 'T1', kind: 'text', clips: [] },
]

export const useTimelineStore = create<TimelineState>()(
  persist(
    (set, get) => ({
  tracks: defaultTracks,
  playhead: 0,
  zoom: 80,
  totalDuration: 0,
  selectedClipId: null,
  playing: false,

  setPlayhead: (time: number) => {
    const clamped = Math.max(0, Math.min(time, get().totalDuration || 1))
    set({ playhead: clamped })
  },

  setPlaying: (p: boolean) => {
    set({ playing: p })
  },
  setZoom: (zoom: number) => {
    set({ zoom: Math.max(1, Math.min(500, zoom)) })
  },

  moveClip: (clipId: string, newStart: number) => {
    set(state => {
      const tracks = state.tracks.map(track => ({
        ...track,
        clips: track.clips.map(c =>
          c.id === clipId ? { ...c, start: Math.max(0, newStart) } : c,
        ),
      }))
      return { tracks, totalDuration: calcDuration(tracks) }
    })
  },

  addClip: (trackId: string, name: string, duration: number, color: string, sourceMediaId?: string) => {
    set(state => {
      const tracks = state.tracks.map(track =>
        track.id === trackId
          ? {
              ...track,
              clips: [
                ...track.clips,
                {
                  id: nanoid(),
                  name,
                  start: track.clips.reduce((max, c) => Math.max(max, c.start + c.duration), 0),
                  duration,
                  color,
                  trackId,
                  sourceMediaId,
                  sourceStart: 0,
                },
              ],
            }
          : track,
      )
      return { tracks, totalDuration: calcDuration(tracks) }
    })
  },

  resizeClip: (clipId: string, newDuration: number) => {
    set(state => {
      const tracks = state.tracks.map(track => ({
        ...track,
        clips: track.clips.map(c =>
          c.id === clipId ? { ...c, duration: Math.max(0.5, newDuration) } : c,
        ),
      }))
      return { tracks, totalDuration: calcDuration(tracks) }
    })
  },

  splitClip: (trackId: string, clipId: string, splitTime: number) => {
    set(state => {
      const tracks = state.tracks.map(track => {
        if (track.id !== trackId) return track
        const clip = track.clips.find(c => c.id === clipId)
        if (!clip) return track
        if (splitTime <= clip.start || splitTime >= clip.start + clip.duration) return track

        const leftDur = splitTime - clip.start
        const rightStart = splitTime
        const rightDur = clip.duration - leftDur
        const baseSource = clip.sourceStart ?? 0

        return {
          ...track,
          clips: track.clips.flatMap(c =>
            c.id === clipId
              ? [
                  { ...c, id: nanoid(), duration: leftDur, sourceStart: baseSource },
                  { ...c, id: nanoid(), start: rightStart, duration: rightDur, sourceStart: baseSource + leftDur },
                ]
              : [c],
          ),
        }
      })
      return { tracks, totalDuration: calcDuration(tracks) }
    })
  },

  removeClip: (trackId: string, clipId: string) => {
    set(state => {
      const tracks = state.tracks.map(track =>
        track.id === trackId
          ? { ...track, clips: track.clips.filter(c => c.id !== clipId) }
          : track,
      )
      return { tracks, totalDuration: calcDuration(tracks) }
    })
  },
  selectClip: (clipId: string | null) => {
    set({ selectedClipId: clipId })
  },
  findClipByPlayhead(time: number) {
    const tks = get().tracks
    for (const track of tks) {
      const clip = track.clips.find(c => time >= c.start && time <= c.start + c.duration)
      if (clip) return { trackId: track.id, clip }
    }
    return null
  },
  findClipBySourceTime(sourceTime: number) {
    const tks = get().tracks
    for (const track of tks) {
      const clip = track.clips.find(c => {
        const ss = c.sourceStart ?? c.start
        return sourceTime >= ss && sourceTime < ss + c.duration
      })
      if (clip) return { trackId: track.id, clip }
    }
    return null
  },
  getSourceTime(timelineTime: number) {
    const found = get().findClipByPlayhead(timelineTime)
    if (!found) return null
    const c = found.clip
    return (c.sourceStart ?? 0) + (timelineTime - c.start)
  },
  closeGap(trackId: string, gapStart: number, gapEnd: number) {
    const gapSize = gapEnd - gapStart
    set(state => {
      const tracks = state.tracks.map(track =>
        track.id === trackId
          ? {
              ...track,
              clips: track.clips.map(c => ({
                ...c,
                start: c.start > gapStart ? c.start - gapSize : c.start,
              })),
            }
          : track,
      )
      return { tracks, totalDuration: calcDuration(tracks) }
    })
  },
}),
    {
      name: 'velocis-timeline',
      partialize: (state) => ({
        tracks: state.tracks,
        zoom: state.zoom,
        totalDuration: state.totalDuration,
        selectedClipId: state.selectedClipId,
      }),
    },
  ),
)
