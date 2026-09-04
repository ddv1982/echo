import {
  BookOpenText,
  CircleAlert,
  History,
  Home,
  Power,
  Settings,
} from 'lucide-react'
import { BrandMark, StatusPill } from './app/chrome'
import { useAppController } from './app/useAppController'
import { DictionaryView } from './dictionary/DictionaryView'
import { HistoryView } from './history/HistoryView'
import { HomeView } from './home/HomeView'
import { SettingsView } from './settings/SettingsView'
import type { View } from './types'

const navigation: Array<{ id: View; label: string; icon: typeof Home }> = [
  { id: 'home', label: 'Home', icon: Home },
  { id: 'history', label: 'History', icon: History },
  { id: 'dictionary', label: 'Dictionary', icon: BookOpenText },
  { id: 'settings', label: 'Settings', icon: Settings },
]

function App() {
  const {
    view,
    setView,
    status,
    history,
    deleteHistoryItem,
    clearHistory,
    dictionary,
    theme,
    setTheme,
    error,
    setError,
    recordingSeconds,
    refreshStatus,
    toggleRecording,
    quitApp,
    addDictionaryEntry,
    addDictionaryEntriesBatch,
    removeDictionaryEntry,
  } = useAppController()

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-lockup">
          <BrandMark />
          <h1>Echo</h1>
        </div>
        <nav className="navigation" aria-label="Echo sections">
          <div className="nav-list">
            {navigation.map((item) => {
              const Icon = item.icon
              return (
                <button
                  type="button"
                  key={item.id}
                  className="nav-item"
                  data-active={view === item.id}
                  aria-current={view === item.id ? 'page' : undefined}
                  aria-label={item.label}
                  onClick={() => setView(item.id)}
                >
                  <Icon size={18} aria-hidden="true" />
                  <span>{item.label}</span>
                </button>
              )
            })}
          </div>
        </nav>
        <div className="topbar-actions">
          <StatusPill status={status} />
          <button
            type="button"
            className="icon-button"
            aria-label="Quit Echo"
            title="Quit Echo"
            onClick={() => void quitApp()}
          >
            <Power size={17} aria-hidden="true" />
          </button>
        </div>
      </header>

      <div className="workspace">
        <main className="main-content">
          {error ? (
            <div className="error-banner" role="alert">
              <CircleAlert size={17} aria-hidden="true" />
              <span>{error}</span>
              <button type="button" onClick={() => setError(null)} aria-label="Dismiss error">×</button>
            </div>
          ) : null}

          {view === 'home' ? (
            <HomeView
              status={status}
              history={history}
              recordingSeconds={recordingSeconds}
              onToggleRecording={toggleRecording}
              onOpenSettings={() => setView('settings')}
              onOpenHistory={() => setView('history')}
            />
          ) : null}
          {view === 'history' ? (
            <HistoryView
              items={history}
              onDelete={deleteHistoryItem}
              onClear={clearHistory}
              onError={setError}
            />
          ) : null}
          {view === 'dictionary' ? (
            <DictionaryView
              items={dictionary}
              onAdd={addDictionaryEntry}
              onAddBatch={addDictionaryEntriesBatch}
              onRemove={removeDictionaryEntry}
              onError={setError}
            />
          ) : null}
          {view === 'settings' ? (
            <SettingsView
              status={status}
              theme={theme}
              onThemeChange={setTheme}
              onStatusChange={refreshStatus}
              onError={setError}
            />
          ) : null}
        </main>
      </div>
    </div>
  )
}

export default App
