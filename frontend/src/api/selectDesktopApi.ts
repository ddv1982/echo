export function selectDesktopApi<T>(hasTauriInternals: boolean, tauri: T, preview: T): T {
  return hasTauriInternals ? tauri : preview
}
