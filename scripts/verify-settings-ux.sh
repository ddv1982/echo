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
rg -q 'Jabra Elite 8 Active' frontend/src/tauri.ts
rg -q 'Advanced audio endpoints' frontend/src/settings/MicrophoneChooser.tsx
rg -q 'Installed components' frontend/src/settings/SpeechSetupSection.tsx
rg -q 'Advanced speech options' frontend/src/settings/SpeechSetupSection.tsx
rg -q -- '--radius-md:' frontend/src/styles/tokens.css
rg -Fq '@media (max-width: 960px)' frontend/src/styles/views.css
if rg -Fq '@media (max-width: 760px)' frontend/src/styles/views.css \
  || sed -n '/@media (max-width: 520px)/,/^}/p' frontend/src/styles/views.css | rg -q 'setting-row'; then
  printf '%s\n' 'stale Settings-specific narrow breakpoint remains' >&2
  exit 1
fi

generated_components=$(sed -n 's/^export type ComponentId = //p' frontend/src/generated/ipc.ts \
  | rg -o '"[^"]+"' \
  | tr -d '"' \
  | sort)
preview_components=$(sed -n '/const sources:/,/return {/p' frontend/src/tauri.ts \
  | rg -o "id: '[^']+'" \
  | cut -d "'" -f 2 \
  | sort)
if ! diff -u \
  <(printf '%s\n' "$generated_components") \
  <(printf '%s\n' "$preview_components"); then
  printf '%s\n' 'preview readiness component IDs differ from the generated IPC contract' >&2
  exit 1
fi
test "$(printf '%s\n' "$generated_components" | wc -l)" -eq 8

plan_count=$(sed -n '/plans: \[/,/microphoneReady:/p' frontend/src/tauri.ts \
  | rg -c "id: '(recommended|parakeet|whisper-base|whisper-small|whisper-large-v3-turbo)'" )
test "$plan_count" -eq 5

advanced_count=$(sed -n '/const advancedDevices/,/return \[/p' frontend/src/tauri.ts | rg -c "\['alsa:")
test "$advanced_count" -ge 8

cargo test -p echo microphone::tests
npm run typecheck --prefix frontend
npm run test --prefix frontend -- --run setup.test.ts
npm run test:responsive --prefix frontend

printf '%s\n' 'verify-settings-ux: ok'
