import { contextBridge, ipcRenderer } from 'electron'

contextBridge.exposeInMainWorld('electronAPI', {
  platform: process.platform,
  isElectron: true,
  showNotification: (title: string, body: string) => {
    ipcRenderer.send('show-notification', title, body)
  }
})
