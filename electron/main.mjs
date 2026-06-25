import { app, BrowserWindow, ipcMain } from 'electron'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { readFile } from 'node:fs/promises'

const __dirname = dirname(fileURLToPath(import.meta.url))
const isDev = !app.isPackaged

app.whenReady().then(() => {
  const win = new BrowserWindow({
    width: 1100,
    height: 720,
    minWidth: 900,
    minHeight: 600,
    center: true,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: join(__dirname, 'preload.cjs'),
    },
  })

  ipcMain.on('maximize', () => {
    win.maximize()
  })

  ipcMain.handle('read-file', async (_event, filePath) => {
    try {
      const buf = await readFile(filePath)
      return buf.buffer
    } catch { return null }
  })

  if (isDev) {
    win.loadURL(process.env.VITE_DEV_SERVER_URL || 'http://localhost:5173')
    win.webContents.openDevTools({ mode: 'detach' })
  } else {
    win.loadFile(join(__dirname, '../dist/index.html'))
  }
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
