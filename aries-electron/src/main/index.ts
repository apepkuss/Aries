import { app, BrowserWindow, shell, dialog, ipcMain, Notification } from 'electron'
import path from 'path'
import fs from 'fs'
import { BackendManager } from './backend'

// Map file extension to MIME type
function getMimeType(ext: string): string {
  const mimeMap: Record<string, string> = {
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.gif': 'image/gif',
    '.webp': 'image/webp',
    '.txt': 'text/plain',
    '.md': 'text/markdown',
    '.csv': 'text/csv',
    '.pdf': 'application/pdf',
    '.json': 'application/json',
    '.xml': 'application/xml',
    '.js': 'text/javascript',
    '.ts': 'text/typescript',
    '.py': 'text/x-python',
    '.rs': 'text/x-rust',
    '.go': 'text/x-go',
    '.java': 'text/x-java',
    '.c': 'text/x-c',
    '.cpp': 'text/x-c++',
    '.h': 'text/x-c',
    '.hpp': 'text/x-c++',
    '.css': 'text/css',
    '.html': 'text/html',
    '.yaml': 'text/yaml',
    '.yml': 'text/yaml',
    '.toml': 'text/toml',
    '.sh': 'text/x-shellscript',
    '.sql': 'text/x-sql',
  }
  return mimeMap[ext] || 'application/octet-stream'
}

// Handle native notification requests from renderer
ipcMain.on('show-notification', (_event, title: string, body: string) => {
  if (Notification.isSupported()) {
    const notification = new Notification({ title, body })
    notification.on('click', () => {
      mainWindow?.show()
      mainWindow?.focus()
    })
    notification.show()
  }
})

// Handle file selection dialog
ipcMain.handle('select-files', async () => {
  if (!mainWindow) return []

  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openFile', 'multiSelections'],
    filters: [
      {
        name: 'Supported Files',
        extensions: [
          'png', 'jpg', 'jpeg', 'gif', 'webp',
          'txt', 'md', 'csv',
          'js', 'ts', 'py', 'rs', 'go', 'java', 'c', 'cpp', 'h', 'hpp',
          'css', 'html', 'json', 'yaml', 'yml', 'toml', 'xml', 'sh', 'sql',
          'pdf',
        ],
      },
      { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] },
      { name: 'Text', extensions: ['txt', 'md', 'csv'] },
      {
        name: 'Code',
        extensions: [
          'js', 'ts', 'py', 'rs', 'go', 'java', 'c', 'cpp', 'h', 'hpp',
          'css', 'html', 'json', 'yaml', 'yml', 'toml', 'xml', 'sh', 'sql',
        ],
      },
      { name: 'PDF', extensions: ['pdf'] },
      { name: 'All Files', extensions: ['*'] },
    ],
  })

  if (result.canceled || result.filePaths.length === 0) return []

  return result.filePaths.map((filePath) => {
    const stats = fs.statSync(filePath)
    const ext = path.extname(filePath).toLowerCase()
    return {
      path: filePath,
      name: path.basename(filePath),
      size: stats.size,
      extension: ext,
      mimeType: getMimeType(ext),
    }
  })
})

// Check whether given file paths still exist on disk
ipcMain.handle('check-files-exist', async (_event, paths: string[]) => {
  return paths.map((p) => fs.existsSync(p))
})

let mainWindow: BrowserWindow | null = null
const backend = new BackendManager()

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    title: 'Aries',
    webPreferences: {
      preload: path.join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false
    },
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    trafficLightPosition: { x: 16, y: 16 },
    show: false
  })

  // Show window when content is ready to avoid white flash
  mainWindow.on('ready-to-show', () => {
    mainWindow?.show()
  })

  // Open external links in system browser
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  mainWindow.on('closed', () => {
    mainWindow = null
  })

  // Load the splash/loading page first
  const loadingPage = path.join(__dirname, '../renderer/index.html')
  mainWindow.loadFile(loadingPage)
}

app.whenReady().then(async () => {
  // 1. Show window with loading screen immediately
  createWindow()

  // 2. Start backend in the background
  try {
    console.log('Starting Aries backend...')
    const port = await backend.start()
    console.log(`Backend started on port ${port}`)

    // 3. Navigate to the backend-served frontend
    mainWindow?.loadURL(`http://127.0.0.1:${port}`)
  } catch (err) {
    console.error('Failed to start backend:', err)
    dialog.showErrorBox(
      'Failed to start Aries backend',
      err instanceof Error ? err.message : String(err)
    )
    app.quit()
  }
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit()
  }
})

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0 && backend.port) {
    createWindow()
    mainWindow?.loadURL(`http://127.0.0.1:${backend.port}`)
  }
})

app.on('before-quit', async () => {
  await backend.stop()
})

// Ensure backend is cleaned up on unexpected exit
process.on('exit', () => {
  // Synchronous cleanup is limited, but tree-kill in stop() handles it
})
