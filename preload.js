const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('api', {
  onTrack: (callback) => ipcRenderer.on('track', (_event, track) => callback(track))
})
