import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { tauriDesktopApi } from './api/tauriDesktopApi'
import './styles/index.css'
import { configureDesktopApi } from './tauri'

configureDesktopApi(tauriDesktopApi)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
