import { useEffect, useState, useCallback } from 'react'
import Home from './components/Home'
import Editor from './components/Editor'
import { useProjectStore } from './store/project'

function App() {
  const [view, setView] = useState<'home' | 'editor'>('home')
  const projects = useProjectStore(s => s.projects)
  const selectProject = useProjectStore(s => s.selectProject)
  const createProject = useProjectStore(s => s.createProject)

  useEffect(() => {
    if (projects.length === 0) {
      createProject('Sans titre')
    }
  }, [projects.length, createProject])

  const handleOpenProject = useCallback((id: string) => {
    selectProject(id)
    setView('editor')
  }, [selectProject])

  const handleHome = useCallback(() => {
    selectProject(null)
    setView('home')
  }, [selectProject])

  if (view === 'home') {
    return <Home onOpenProject={handleOpenProject} />
  }

  return <Editor onHome={handleHome} />
}

export default App
