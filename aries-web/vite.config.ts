import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    proxy: {
      '/v1': {
        target: 'http://localhost:3389',
        changeOrigin: true,
      },
      '/api': {
        target: 'http://localhost:3389',
        changeOrigin: true,
      },
    },
  },
})
