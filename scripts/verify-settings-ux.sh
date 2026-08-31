#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

rg -q 'features = \["pipewire", "pulseaudio"\]' crates/echo/Cargo.toml
rg -q 'libpipewire-0.3-dev' .github/workflows/check.yml .github/workflows/release.yml
rg -q 'libpulse-dev' .github/workflows/check.yml .github/workflows/release.yml
rg -q '"depends": \["libpipewire-0.3-0", "libpulse0"\]' src-tauri/tauri.conf.json
rg -q '"depends": \["pipewire-libs", "pulseaudio-libs"\]' src-tauri/tauri.conf.json
rg -q 'rpm Requires is missing' .github/workflows/release.yml
rg -q 'deb Depends is missing' .github/workflows/release.yml
rg -Fq 'libayatana-appindicator3-1' .github/workflows/release.yml
rg -Fq 'libwebkit2gtk-4.1-0' .github/workflows/release.yml
rg -Fq 'libgtk-3-0' .github/workflows/release.yml
rg -Fq "'libayatana-appindicator3.so.1()(64bit)'" .github/workflows/release.yml
rg -Fq "'libwebkit2gtk-4.1.so.0()(64bit)'" .github/workflows/release.yml
rg -Fq "'libgtk-3.so.0()(64bit)'" .github/workflows/release.yml
rg -q 'Advanced audio endpoints' frontend/src/settings/MicrophoneChooser.tsx
rg -q 'Installed components' frontend/src/settings/SpeechSetupSection.tsx
rg -q 'Advanced speech options' frontend/src/settings/SpeechSetupSection.tsx
rg -q 'aria-label="Transcription"' frontend/src/settings/SettingsView.tsx
rg -q 'GPU preference saved for Whisper' src-tauri/src/speech.rs
rg -q 'NextSpeechRun' crates/echo-ipc/src/lib.rs
if rg -q '<summary>Advanced</summary>' frontend/src/settings/SettingsView.tsx; then
  printf '%s\n' 'top-level Advanced Settings drawer returned' >&2
  exit 1
fi
rg -q -- '--radius-md:' frontend/src/styles/tokens.css
rg -Fq '@media (max-width: 960px)' frontend/src/styles/views.css
if rg -Fq '@media (max-width: 760px)' frontend/src/styles/views.css \
  || sed -n '/@media (max-width: 520px)/,/^}/p' frontend/src/styles/views.css | rg -q 'setting-row'; then
  printf '%s\n' 'stale Settings-specific narrow breakpoint remains' >&2
  exit 1
fi

cargo test -p echo microphone::tests
npm run typecheck --prefix frontend
npm run test --prefix frontend -- --run src/api/previewDesktopApi.test.ts
npm run test --prefix frontend -- --run setup.test.ts
npm run test:responsive --prefix frontend

printf '%s\n' 'verify-settings-ux: ok'
