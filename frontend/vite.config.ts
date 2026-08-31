import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { workspaceVersionFromManifest } from './src/workspaceVersion.ts'

const statusPerfProbe = process.env.VITE_STATUS_PERF_PROBE === '1'

// The workspace Cargo.toml is the single version source. The frontend reads
// it at build time; nothing in the frontend may hardcode a version.
function workspaceVersion(): string {
  const manifest = readFileSync(fileURLToPath(new URL('../Cargo.toml', import.meta.url)), 'utf8')
  return workspaceVersionFromManifest(manifest)
}

export default defineConfig({
  plugins: [
    react(),
    {
      name: 'echo-preview-entry',
      apply: 'serve',
      transformIndexHtml: (html) => html.replace('/src/main.tsx', '/src/preview.tsx'),
    },
    {
      name: 'echo-production-boundary',
      apply: 'build',
      generateBundle: (options, bundle) => {
        void options
        if (statusPerfProbe) return
        const previewModules = [
          '/src/preview.tsx',
          '/src/api/previewDesktopApi.ts',
          '/src/perf/statusPerf.ts',
        ]
        const previewStrings = ['Jabra Elite 8 Active', 'This is a test. This is a test.', 'echo-preview']
        for (const output of Object.values(bundle)) {
          if (output.type !== 'chunk') continue
          if (previewModules.some((module) =>
            Object.keys(output.modules).some((path) => path.endsWith(module)))) {
            throw new Error(`production chunk ${output.fileName} imports preview code`)
          }
          if (previewStrings.some((fixture) => output.code.includes(fixture))) {
            throw new Error(`production chunk ${output.fileName} contains preview fixtures`)
          }
        }
      },
    },
  ],
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
