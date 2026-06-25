const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('electronAPI', {
  maximize: () => ipcRenderer.send('maximize'),
  readFile: (path) => ipcRenderer.invoke('read-file', path),
})
