import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// Vite root = webview-ui so index.html and src/ are shared without copying.
// Build output goes to tauri-app/dist/ (frontendDist in tauri.conf.json).
export default defineConfig({
  root: path.resolve(__dirname, '../webview-ui'),
  plugins: [react()],
  build: {
    outDir: path.resolve(__dirname, 'dist'),
    emptyOutDir: true,
  },
  base: './',
  server: {
    port: 5173,
    strictPort: true,
    // Prevent Vite from opening a browser tab
    open: false,
  },
})
