import { contextBridge, ipcRenderer } from 'electron'

export interface FileAttachmentInfo {
  path: string
  name: string
  size: number
  extension: string
  mimeType: string
}

contextBridge.exposeInMainWorld('electronAPI', {
  platform: process.platform,
  isElectron: true,
  showNotification: (title: string, body: string) => {
    ipcRenderer.send('show-notification', title, body)
  },
  selectFiles: (): Promise<FileAttachmentInfo[]> => {
    return ipcRenderer.invoke('select-files')
  },
  checkFilesExist: (paths: string[]): Promise<boolean[]> => {
    return ipcRenderer.invoke('check-files-exist', paths)
  }
})
