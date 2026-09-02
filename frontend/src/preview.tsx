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

const rootElement = document.getElementById('root')
if (!rootElement) {
  throw new Error('Cannot start Echo preview: root element #root was not found.')
}

createRoot(rootElement).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
