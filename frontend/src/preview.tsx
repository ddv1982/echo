import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { createPreviewDesktopApi } from './api/previewDesktopApi'
import { selectDesktopApi } from './api/selectDesktopApi'
import { tauriDesktopApi } from './api/tauriDesktopApi'
import './styles/index.css'
import { configureDesktopApi } from './tauri'

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown
  }
}

const previewDesktopApi = createPreviewDesktopApi()
const desktopApi = selectDesktopApi(
  Boolean(window.__TAURI_INTERNALS__),
  tauriDesktopApi,
  previewDesktopApi,
)
configureDesktopApi(desktopApi)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
