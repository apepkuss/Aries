import { app, BrowserWindow, shell, dialog, ipcMain, Notification } from 'electron'
import path from 'path'
import fs from 'fs'
import { BackendManager } from './backend'
import { initAutoUpdater, stopAutoUpdater } from './updater'

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
    title: 'Moss',
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
    console.log('Starting Moss backend...')
    const port = await backend.start()
    console.log(`Backend started on port ${port}`)

    // 3. Navigate to the backend-served frontend
    mainWindow?.loadURL(`http://127.0.0.1:${port}`)

    // 4. Start auto-updater (packaged builds only)
    initAutoUpdater()
  } catch (err) {
    console.error('Failed to start backend:', err)
    const msg = err instanceof Error ? err.message : String(err)

    let title = 'Failed to start Moss'
    let detail = msg

    if (msg.includes('binary not found')) {
      title = 'Backend binary not found'
      detail =
        'The Moss backend binary could not be located.\n\n' +
        'If you are running in development mode, please run:\n' +
        '  cargo build --release\n\n' +
        msg
    } else if (msg.includes('already in use')) {
      title = 'Port conflict'
      detail =
        'Another process is using the configured port.\n\n' +
        `You can change the port in:\n${backend.getConfigFilePath()}\n\n` +
        'Or stop the other process and try again.'
    } else if (msg.includes('No config.toml found')) {
      title = 'Configuration file missing'
      detail =
        'Moss could not find a config.toml file.\n\n' +
        `Expected location: ${backend.getConfigFilePath()}\n\n` +
        'Please ensure the configuration file exists.'
    } else if (msg.includes('did not become ready')) {
      title = 'Backend startup timeout'
      detail =
        'The backend process started but did not respond in time.\n\n' +
        'Check the log file for details:\n' +
        `${app.getPath('logs')}/moss-backend.log`
    } else if (msg.includes('exited unexpectedly')) {
      title = 'Backend crashed'
      detail =
        'The backend process exited unexpectedly.\n\n' +
        'Check the log file for details:\n' +
        `${app.getPath('logs')}/moss-backend.log`
    }

    dialog.showErrorBox(title, detail)
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

let isQuitting = false

app.on('before-quit', (e) => {
  if (isQuitting) return // Already handling quit

  e.preventDefault()
  isQuitting = true

  stopAutoUpdater()
  backend.stop().finally(() => {
    app.exit(0)
  })
})
