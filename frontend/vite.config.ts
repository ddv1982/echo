import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The workspace Cargo.toml is the single version source. The frontend reads
// it at build time; nothing in the frontend may hardcode a version.
function workspaceVersion(): string {
  const manifest = readFileSync(fileURLToPath(new URL('../Cargo.toml', import.meta.url)), 'utf8')
  const section = manifest.match(/\[workspace\.package\]([^[]*)/)
  const version = section?.[1]?.match(/version\s*=\s*"([^"]+)"/)
  if (!version) throw new Error('workspace.package.version not found in Cargo.toml')
  return version[1]
}

export default defineConfig({
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(workspaceVersion()),
  },
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: './src/test/setup.ts',
  },
})
