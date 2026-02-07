import { app, dialog, BrowserWindow } from 'electron'
import electronUpdater, { type AppUpdater } from 'electron-updater'

// ESM compatibility workaround for electron-updater
// See https://github.com/electron-userland/electron-builder/issues/7976
function getAutoUpdater(): AppUpdater {
  const { autoUpdater } = electronUpdater
  return autoUpdater
}

const autoUpdater = getAutoUpdater()

// Check interval: 4 hours
const CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000

let pollingTimer: ReturnType<typeof setInterval> | null = null

/**
 * Initialise the auto-updater.
 *
 * Call once after the main window has loaded.
 * - Disables auto-download so the user can decide.
 * - Installs the downloaded update when the app quits.
 * - Shows a native dialog when a new version is found.
 * - Polls for updates at a fixed interval.
 *
 * For private GitHub repos, set the `GH_TOKEN` environment variable
 * (with `repo` scope) on the user's machine.
 */
export function initAutoUpdater(): void {
  // Skip in dev mode — updates only work on packaged apps
  if (!app.isPackaged) {
    console.log('[updater] Skipping auto-updater in development mode')
    return
  }

  autoUpdater.autoDownload = false
  autoUpdater.autoInstallOnAppQuit = true

  // ── Event handlers ──────────────────────────────────────────────

  autoUpdater.on('checking-for-update', () => {
    console.log('[updater] Checking for updates…')
  })

  autoUpdater.on('update-available', (info) => {
    console.log(`[updater] Update available: v${info.version}`)
    promptUserForUpdate(info.version)
  })

  autoUpdater.on('update-not-available', () => {
    console.log('[updater] Already up to date')
  })

  autoUpdater.on('download-progress', (progress) => {
    console.log(`[updater] Download: ${progress.percent.toFixed(1)}%`)
  })

  autoUpdater.on('update-downloaded', (info) => {
    console.log(`[updater] Update downloaded: v${info.version}`)
    promptUserToRestart(info.version)
  })

  autoUpdater.on('error', (err) => {
    console.error('[updater] Error:', err.message)
  })

  // ── Initial check (delayed 5 s to let the UI settle) ───────────

  setTimeout(() => {
    autoUpdater.checkForUpdates().catch((err) => {
      console.error('[updater] Initial check failed:', err.message)
    })
  }, 5000)

  // ── Periodic polling ────────────────────────────────────────────

  pollingTimer = setInterval(() => {
    autoUpdater.checkForUpdates().catch((err) => {
      console.error('[updater] Periodic check failed:', err.message)
    })
  }, CHECK_INTERVAL_MS)
}

/**
 * Stop the polling timer. Call when the app is quitting.
 */
export function stopAutoUpdater(): void {
  if (pollingTimer) {
    clearInterval(pollingTimer)
    pollingTimer = null
  }
}

// ── User-facing dialogs ─────────────────────────────────────────────

async function promptUserForUpdate(version: string): Promise<void> {
  const focusedWindow = BrowserWindow.getFocusedWindow()

  const { response } = await dialog.showMessageBox(
    focusedWindow ?? ({} as BrowserWindow),
    {
      type: 'info',
      title: 'Update Available',
      message: `A new version (v${version}) is available.`,
      detail: 'Would you like to download it now?',
      buttons: ['Download', 'Later'],
      defaultId: 0,
      cancelId: 1,
    }
  )

  if (response === 0) {
    autoUpdater.downloadUpdate().catch((err) => {
      console.error('[updater] Download failed:', err.message)
      dialog.showErrorBox(
        'Update download failed',
        `Could not download the update:\n${err.message}`
      )
    })
  }
}

async function promptUserToRestart(version: string): Promise<void> {
  const focusedWindow = BrowserWindow.getFocusedWindow()

  const { response } = await dialog.showMessageBox(
    focusedWindow ?? ({} as BrowserWindow),
    {
      type: 'info',
      title: 'Update Ready',
      message: `Version v${version} has been downloaded.`,
      detail: 'Restart now to apply the update?',
      buttons: ['Restart', 'Later'],
      defaultId: 0,
      cancelId: 1,
    }
  )

  if (response === 0) {
    autoUpdater.quitAndInstall(false, true)
  }
}
