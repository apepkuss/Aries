import { app, BrowserWindow, shell, dialog, ipcMain, Notification } from 'electron'
import path from 'path'
import { BackendManager } from './backend'

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
