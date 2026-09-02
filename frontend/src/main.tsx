import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { tauriDesktopApi } from './api/tauriDesktopApi'
import { startStatusPerfProbe } from './perf/startStatusPerfProbe'
import './styles/index.css'
import { configureDesktopApi } from './tauri'

if (import.meta.env.VITE_STATUS_PERF_PROBE === '1') {
  startStatusPerfProbe()
} else {
  configureDesktopApi(tauriDesktopApi)
  const rootElement = document.getElementById('root')
  if (!rootElement) {
    throw new Error('Cannot start Echo: root element #root was not found.')
  }
  createRoot(rootElement).render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}
