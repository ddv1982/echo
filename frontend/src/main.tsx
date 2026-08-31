import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { tauriDesktopApi } from './api/tauriDesktopApi'
import './styles/index.css'
import { configureDesktopApi } from './tauri'

if (import.meta.env.VITE_STATUS_PERF_PROBE === '1') {
  void import('./perf/statusPerf').then(({ startStatusPerf }) => startStatusPerf())
} else {
  configureDesktopApi(tauriDesktopApi)
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}
