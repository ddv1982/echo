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
rg -q 'Jabra Elite 8 Active' frontend/src/tauri.ts
rg -q 'Advanced audio endpoints' frontend/src/App.tsx
rg -q 'Installed components' frontend/src/App.tsx
rg -q 'Advanced speech options' frontend/src/App.tsx
rg -q -- '--radius-md:' frontend/src/styles/tokens.css
rg -Fq '@media (max-width: 960px)' frontend/src/styles/views.css
if rg -Fq '@media (max-width: 760px)' frontend/src/styles/views.css \
  || sed -n '/@media (max-width: 520px)/,/^}/p' frontend/src/styles/views.css | rg -q 'setting-row'; then
  printf '%s\n' 'stale Settings-specific narrow breakpoint remains' >&2
  exit 1
fi

component_count=$(sed -n '/const sources:/,/return {/p' frontend/src/tauri.ts \
  | rg -c "id: '(whisper-runtime|whisper-base-q5-1|whisper-small|whisper-large-v3-turbo-q5-0|silero-vad|sherpa-runtime|parakeet-tdt-06b-v3-int8)'" )
test "$component_count" -eq 7

plan_count=$(sed -n '/plans: \[/,/microphoneReady:/p' frontend/src/tauri.ts \
  | rg -c "id: '(recommended|parakeet|whisper-base|whisper-small|whisper-large-v3-turbo)'" )
test "$plan_count" -eq 5

advanced_count=$(sed -n '/const advancedDevices/,/return \[/p' frontend/src/tauri.ts | rg -c "\['alsa:")
test "$advanced_count" -ge 8

cargo test -p echo microphone::tests
npm run typecheck --prefix frontend
npm run test --prefix frontend -- --run setup.test.ts

printf '%s\n' 'verify-settings-ux: ok'
