#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

rg -q 'features = \["pipewire", "pulseaudio"\]' crates/echo/Cargo.toml
rg -q 'libpipewire-0.3-dev' .github/workflows/check.yml .github/workflows/release.yml
rg -q 'libpulse-dev' .github/workflows/check.yml .github/workflows/release.yml
rg -q 'Jabra Elite 8 Active' frontend/src/tauri.ts

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

printf '%s\n' 'verify-settings-ux: ok'
