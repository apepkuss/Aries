import { spawn, ChildProcess } from 'child_process'
import { app } from 'electron'
import path from 'path'
import fs from 'fs'
import net from 'net'
import http from 'http'
import kill from 'tree-kill'

const DEFAULT_PORT = 3389

export class BackendManager {
  private process: ChildProcess | null = null
  private _port: number = 0

  get port(): number {
    return this._port
  }

  /**
   * Find an available TCP port, starting from the preferred port.
   */
  async findAvailablePort(preferred: number = DEFAULT_PORT): Promise<number> {
    return new Promise((resolve, reject) => {
      const server = net.createServer()
      server.listen(preferred, '127.0.0.1', () => {
        server.close(() => resolve(preferred))
      })
      server.on('error', () => {
        // Preferred port is busy, let OS assign one
        const fallback = net.createServer()
        fallback.listen(0, '127.0.0.1', () => {
          const address = fallback.address()
          if (address && typeof address !== 'string') {
            fallback.close(() => resolve(address.port))
          } else {
            fallback.close(() => reject(new Error('Failed to find available port')))
          }
        })
        fallback.on('error', (err) => reject(err))
      })
    })
  }

  /**
   * Get the path to the Rust backend binary.
   */
  private getBinaryPath(): string {
    if (app.isPackaged) {
      const binaryName = process.platform === 'win32' ? 'aries.exe' : 'aries'
      return path.join(process.resourcesPath, 'backend', binaryName)
    }
    // Development: use cargo build output
    const binaryName = process.platform === 'win32' ? 'aries.exe' : 'aries'
    return path.join(__dirname, '..', '..', '..', 'target', 'release', binaryName)
  }

  /**
   * Get the path to the frontend static files.
   */
  private getWebUiPath(): string {
    if (app.isPackaged) {
      return path.join(process.resourcesPath, 'web-ui')
    }
    return path.join(__dirname, '..', '..', '..', 'aries-web', 'dist')
  }

  /**
   * Get the config file path. Copies default config on first run.
   */
  private getConfigPath(): string {
    const homeDir = app.getPath('home')
    const userConfigPath = path.join(homeDir, '.aries', 'config.toml')

    if (fs.existsSync(userConfigPath)) {
      return userConfigPath
    }

    // Copy default config to ~/.aries/
    const ariesDir = path.join(homeDir, '.aries')
    const defaultConfigPaths = app.isPackaged
      ? [path.join(process.resourcesPath, 'config.toml.example')]
      : [
          path.join(__dirname, '..', '..', '..', 'config.toml'),
          path.join(__dirname, '..', '..', '..', 'config.toml.example')
        ]

    for (const src of defaultConfigPaths) {
      if (fs.existsSync(src)) {
        fs.mkdirSync(ariesDir, { recursive: true })
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
   * Update the port in the config file for the given port.
   */
  private ensureConfigPort(configPath: string, port: number): string {
    if (port === DEFAULT_PORT) {
      return configPath
    }

    // Read config, update port, write to temp location
    const content = fs.readFileSync(configPath, 'utf-8')
    const updated = content.replace(
      /^port\s*=\s*\d+/m,
      `port = ${port}`
    )

    const homeDir = app.getPath('home')
    const runtimeConfigPath = path.join(homeDir, '.aries', 'config.runtime.toml')
    fs.writeFileSync(runtimeConfigPath, updated, 'utf-8')
    return runtimeConfigPath
  }

  /**
   * Start the backend process.
   */
  async start(): Promise<number> {
    this._port = await this.findAvailablePort()

    const binaryPath = this.getBinaryPath()
    if (!fs.existsSync(binaryPath)) {
      throw new Error(`Backend binary not found at: ${binaryPath}`)
    }

    const webUiPath = this.getWebUiPath()
    const configPath = this.getConfigPath()
    const runtimeConfigPath = this.ensureConfigPort(configPath, this._port)

    const logDir = app.getPath('logs')
    fs.mkdirSync(logDir, { recursive: true })
    const logFile = path.join(logDir, 'aries-backend.log')

    const args = [
      '--config', runtimeConfigPath,
      '--web-ui', webUiPath,
      '--log-destination', 'file',
      '--log-file', logFile
    ]

    console.log(`Starting backend: ${binaryPath}`)
    console.log(`  Config: ${runtimeConfigPath}`)
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
      const check = (): void => {
        if (Date.now() - startTime > timeout) {
          reject(new Error(`Backend did not become ready within ${timeout}ms`))
          return
        }

        // Check if process died
        if (this.process === null) {
          reject(new Error('Backend process exited unexpectedly'))
          return
        }

        const req = http.get(`http://127.0.0.1:${this._port}/health`, (res) => {
          if (res.statusCode === 200) {
            console.log('Backend is ready')
            resolve()
          } else {
            setTimeout(check, interval)
          }
        })

        req.on('error', () => {
          setTimeout(check, interval)
        })

        req.setTimeout(2000, () => {
          req.destroy()
          setTimeout(check, interval)
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
