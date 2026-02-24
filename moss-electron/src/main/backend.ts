import { spawn, execFileSync, ChildProcess } from 'child_process'
import { app } from 'electron'
import path from 'path'
import fs from 'fs'
import net from 'net'
import http from 'http'
import kill from 'tree-kill'

export class BackendManager {
  private process: ChildProcess | null = null
  private _port: number = 0
  private _appHomeDir: string | null = null

  get port(): number {
    return this._port
  }

  /**
   * Query the backend binary for a value via a subcommand.
   * Returns trimmed stdout, or null on failure.
   */
  private queryBackend(subcommand: string): string | null {
    try {
      const output = execFileSync(this.getBinaryPath(), [subcommand], {
        timeout: 5000,
        encoding: 'utf-8'
      })
      return output.trim()
    } catch (err) {
      console.error(`Failed to query backend '${subcommand}': ${err}`)
      return null
    }
  }

  /**
   * Get the application home directory from the backend.
   * Caches the result after first call.
   */
  private getAppHomeDir(): string {
    if (this._appHomeDir) return this._appHomeDir

    const result = this.queryBackend('home-dir')
    if (result) {
      this._appHomeDir = result
      return result
    }

    // Fallback: derive from home directory (should rarely happen)
    console.warn('Failed to get home-dir from backend, using fallback')
    this._appHomeDir = path.join(app.getPath('home'), '.moss')
    return this._appHomeDir
  }

  /**
   * Read the port from the config file.
   * The config file is the single source of truth for port configuration.
   */
  private readPortFromConfig(configPath: string): number {
    try {
      const content = fs.readFileSync(configPath, 'utf-8')
      const match = content.match(/^port\s*=\s*(\d+)/m)
      if (match) {
        return parseInt(match[1], 10)
      }
    } catch {
      // Ignore read errors
    }
    // Fallback: must match [server] port in config.toml
    return 3389
  }

  /**
   * Check if a port is available.
   */
  private checkPortAvailable(port: number): Promise<boolean> {
    return new Promise((resolve) => {
      const server = net.createServer()
      server.listen(port, '127.0.0.1', () => {
        server.close(() => resolve(true))
      })
      server.on('error', () => {
        resolve(false)
      })
    })
  }

  /**
   * Get the path to the Rust backend binary.
   */
  private getBinaryPath(): string {
    if (app.isPackaged) {
      const binaryName = process.platform === 'win32' ? 'moss.exe' : 'moss'
      return path.join(process.resourcesPath, 'backend', binaryName)
    }
    // Development: use cargo build output
    const binaryName = process.platform === 'win32' ? 'moss.exe' : 'moss'
    return path.join(__dirname, '..', '..', '..', 'target', 'release', binaryName)
  }

  /**
   * Get the path to the frontend static files.
   */
  private getWebUiPath(): string {
    if (app.isPackaged) {
      return path.join(process.resourcesPath, 'web-ui')
    }
    return path.join(__dirname, '..', '..', '..', 'moss-web', 'dist')
  }

  /**
   * Get the config file path. Copies default config on first run.
   */
  private getConfigPath(): string {
    const appDir = this.getAppHomeDir()
    const userConfigPath = path.join(appDir, 'config.toml')

    // Ensure app home directory structure exists
    for (const sub of ['artifacts', 'data', 'sessions', 'skills']) {
      fs.mkdirSync(path.join(appDir, sub), { recursive: true })
    }

    if (fs.existsSync(userConfigPath)) {
      return userConfigPath
    }

    // Copy default config to app home directory
    const defaultConfigPaths = app.isPackaged
      ? [path.join(process.resourcesPath, 'config.toml.example')]
      : [
          path.join(__dirname, '..', '..', '..', 'config.toml'),
          path.join(__dirname, '..', '..', '..', 'config.toml.example')
        ]

    for (const src of defaultConfigPaths) {
      if (fs.existsSync(src)) {
        fs.mkdirSync(appDir, { recursive: true })
        fs.copyFileSync(src, userConfigPath)
        console.log(`Copied default config from ${src} to ${userConfigPath}`)
        return userConfigPath
      }
    }

    // Fallback: use project root config (dev only)
    const devConfig = path.join(__dirname, '..', '..', '..', 'config.toml')
    if (fs.existsSync(devConfig)) {
      return devConfig
    }

    throw new Error('No config.toml found')
  }

  /**
   * Get the config file path for display in error messages.
   */
  getConfigFilePath(): string {
    return path.join(this.getAppHomeDir(), 'config.toml')
  }

  /**
   * Start the backend process.
   */
  async start(): Promise<number> {
    const binaryPath = this.getBinaryPath()
    if (!fs.existsSync(binaryPath)) {
      throw new Error(`Backend binary not found at: ${binaryPath}`)
    }

    const webUiPath = this.getWebUiPath()
    const configPath = this.getConfigPath()
    this._port = this.readPortFromConfig(configPath)

    // Check if the port is available
    const available = await this.checkPortAvailable(this._port)
    if (!available) {
      throw new Error(
        `Port ${this._port} is already in use.\n\n` +
        `Please change the port in config file:\n${configPath}`
      )
    }

    const logDir = app.getPath('logs')
    fs.mkdirSync(logDir, { recursive: true })
    const logFile = path.join(logDir, 'moss-backend.log')

    const args = [
      '--config', configPath,
      '--web-ui', webUiPath,
      '--log-destination', 'file',
      '--log-file', logFile
    ]

    console.log(`Starting backend: ${binaryPath}`)
    console.log(`  Config: ${configPath}`)
    console.log(`  Web UI: ${webUiPath}`)
    console.log(`  Port: ${this._port}`)
    console.log(`  Log: ${logFile}`)

    // Ensure binary is executable (Unix)
    if (process.platform !== 'win32') {
      try {
        fs.chmodSync(binaryPath, 0o755)
      } catch {
        // Ignore permission errors
      }
    }

    this.process = spawn(binaryPath, args, {
      cwd: app.getPath('home'),
      env: { ...process.env },
      stdio: ['ignore', 'pipe', 'pipe']
    })

    this.process.stdout?.on('data', (data: Buffer) => {
      console.log(`[backend] ${data.toString().trim()}`)
    })

    this.process.stderr?.on('data', (data: Buffer) => {
      console.error(`[backend] ${data.toString().trim()}`)
    })

    this.process.on('exit', (code, signal) => {
      console.log(`Backend process exited (code=${code}, signal=${signal})`)
      this.process = null
    })

    this.process.on('error', (err) => {
      console.error(`Backend process error: ${err.message}`)
      this.process = null
    })

    await this.waitForReady()
    return this._port
  }

  /**
   * Poll the health endpoint until the backend is ready.
   */
  private waitForReady(timeout: number = 30000): Promise<void> {
    const startTime = Date.now()
    const interval = 500

    return new Promise((resolve, reject) => {
      let done = false

      const check = (): void => {
        if (done) return

        if (Date.now() - startTime > timeout) {
          done = true
          reject(new Error(`Backend did not become ready within ${timeout}ms`))
          return
        }

        // Check if process died
        if (this.process === null) {
          done = true
          reject(new Error('Backend process exited unexpectedly'))
          return
        }

        const req = http.get(`http://127.0.0.1:${this._port}/health`, (res) => {
          if (done) return
          if (res.statusCode === 200) {
            done = true
            console.log('Backend is ready')
            resolve()
          } else {
            setTimeout(check, interval)
          }
        })

        req.on('error', () => {
          if (!done) setTimeout(check, interval)
        })

        req.setTimeout(2000, () => {
          req.destroy()
          if (!done) setTimeout(check, interval)
        })
      }

      check()
    })
  }

  /**
   * Stop the backend process.
   */
  async stop(): Promise<void> {
    if (!this.process || !this.process.pid) {
      return
    }

    console.log(`Stopping backend process (pid=${this.process.pid})`)

    return new Promise<void>((resolve) => {
      const pid = this.process!.pid!

      // Set a timeout to force kill if graceful shutdown takes too long
      const forceKillTimer = setTimeout(() => {
        console.warn('Force killing backend process')
        try {
          kill(pid, 'SIGKILL')
        } catch {
          // Process may already be dead
        }
        resolve()
      }, 5000)

      this.process!.on('exit', () => {
        clearTimeout(forceKillTimer)
        this.process = null
        resolve()
      })

      // Graceful shutdown
      kill(pid, 'SIGTERM', (err) => {
        if (err) {
          console.error(`Failed to kill backend: ${err.message}`)
          clearTimeout(forceKillTimer)
          this.process = null
          resolve()
        }
      })
    })
  }
}
