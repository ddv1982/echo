#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERIFY_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/echo-transcribe-cli.XXXXXX")
trap 'rm -rf "$VERIFY_ROOT"' EXIT

cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p echo-desktop >/dev/null

BIN="$REPO_ROOT/target/debug/echo-desktop"
WAV="$REPO_ROOT/crates/echo/tests/fixtures/claude_code.wav"
CONFIG_DIR="$VERIFY_ROOT/config"
DATA_DIR="$VERIFY_ROOT/data"
MODEL_DIR="$VERIFY_ROOT/models"
mkdir -p "$CONFIG_DIR" "$DATA_DIR" "$MODEL_DIR" "$VERIFY_ROOT/bin"
printf '%s' 'corrupt config sentinel' > "$CONFIG_DIR/config.json"
printf '%s' 'corrupt dictionary sentinel' > "$DATA_DIR/dictionary.json"

run_fake() {
  ECHO_ENGINE=fake \
  ECHO_CONFIG_DIR="$CONFIG_DIR" \
  ECHO_DATA_DIR="$DATA_DIR" \
  ECHO_MODEL_DIR="$MODEL_DIR" \
  "$BIN" "$@"
}

printf 'Claude code.\n' > "$VERIFY_ROOT/expected-clean"
run_fake transcribe "$WAV" > "$VERIFY_ROOT/clean"
cmp "$VERIFY_ROOT/expected-clean" "$VERIFY_ROOT/clean"

printf 'claude code\n' > "$VERIFY_ROOT/expected-raw"
run_fake transcribe "$WAV" --raw > "$VERIFY_ROOT/raw"
cmp "$VERIFY_ROOT/expected-raw" "$VERIFY_ROOT/raw"

run_fake transcribe "$WAV" --format json > "$VERIFY_ROOT/fake.json"
python3 - "$VERIFY_ROOT/fake.json" <<'PY'
import json
import pathlib
import sys

raw = pathlib.Path(sys.argv[1]).read_bytes()
assert raw.endswith(b"\n") and not raw.endswith(b"\n\n")
value = json.loads(raw)
assert value["schemaVersion"] == 1
assert value["text"] == "Claude code."
assert value["raw"] == "claude code"
assert value["engine"]["id"] == "fake"
assert value["engine"]["model"] == "fake"
assert value["language"] == {"requested": "en", "observed": None, "probability": None}
assert value["hintCount"] == 0
assert "confidence" not in value
PY

run_fake transcribe "$WAV" --output "$VERIFY_ROOT/exact.output"
cmp "$VERIFY_ROOT/expected-clean" "$VERIFY_ROOT/exact.output"
test ! -e "$VERIFY_ROOT/exact.output.txt"
(
  cd "$VERIFY_ROOT"
  run_fake transcribe "$WAV" --output relative.output
)
cmp "$VERIFY_ROOT/expected-clean" "$VERIFY_ROOT/relative.output"
test "$(cat "$CONFIG_DIR/config.json")" = 'corrupt config sentinel'
test "$(cat "$DATA_DIR/dictionary.json")" = 'corrupt dictionary sentinel'
test ! -e "$CONFIG_DIR/config.json.corrupt"
for name in history.json status recording.lock recording.stop dictionary.json.corrupt; do
  test ! -e "$DATA_DIR/$name"
done

set +e
run_fake transcribe "$WAV" --raw --format json > "$VERIFY_ROOT/invalid.out" 2> "$VERIFY_ROOT/invalid.err"
INVALID_CODE=$?
run_fake transcribe "$VERIFY_ROOT/missing.wav" > "$VERIFY_ROOT/missing.out" 2> "$VERIFY_ROOT/missing.err"
MISSING_CODE=$?
set -e
test "$INVALID_CODE" -eq 2
test "$MISSING_CODE" -eq 1
test ! -s "$VERIFY_ROOT/invalid.out"
test ! -s "$VERIFY_ROOT/missing.out"
test -s "$VERIFY_ROOT/invalid.err"
test -s "$VERIFY_ROOT/missing.err"

printf '%s' '' > "$MODEL_DIR/ggml-small.bin"
printf '%s' '' > "$MODEL_DIR/ggml-silero-v6.2.0.bin"
printf '%s' '{"entries":[{"spoken":"clawed code","written":"Claude Code","created_at":1}]}' > "$DATA_DIR/dictionary.json"
cat > "$VERIFY_ROOT/bin/whisper-cli" <<'SH'
#!/bin/sh
{
  printf 'BEGIN\n'
  for arg in "$@"; do printf '%s\n' "$arg"; done
  printf 'END\n'
} >> "$ECHO_ARGV_LOG"
if [ ! -f "$ECHO_ATTEMPT_FILE" ]; then
  : > "$ECHO_ATTEMPT_FILE"
  printf 'vad failed\n' >&2
  exit 1
fi
printf '%s\n' '{"model":{"type":"small","multilingual":true},"result":{"language":"de"},"transcription":[{"text":" claude code"}]}'
printf '%s\n' 'whisper_full: auto-detected language: de (p = 0.958162)' >&2
SH
chmod +x "$VERIFY_ROOT/bin/whisper-cli"

PATH="$VERIFY_ROOT/bin:$PATH" \
ECHO_ARGV_LOG="$VERIFY_ROOT/argv.log" \
ECHO_ATTEMPT_FILE="$VERIFY_ROOT/attempt" \
ECHO_CONFIG_DIR="$CONFIG_DIR" \
ECHO_DATA_DIR="$DATA_DIR" \
ECHO_MODEL_DIR="$MODEL_DIR" \
"$BIN" transcribe "$WAV" --engine whisper --model small --language de --format json > "$VERIFY_ROOT/whisper.json"

test "$(grep -c '^BEGIN$' "$VERIFY_ROOT/argv.log")" -eq 2
test "$(grep -c '^--prompt$' "$VERIFY_ROOT/argv.log")" -eq 2
test "$(grep -c '^Claude Code$' "$VERIFY_ROOT/argv.log")" -eq 2
test "$(grep -c '^-l$' "$VERIFY_ROOT/argv.log")" -eq 2
test "$(grep -c '^de$' "$VERIFY_ROOT/argv.log")" -eq 2
test "$(grep -c '^--vad$' "$VERIFY_ROOT/argv.log")" -eq 1
test "$(grep -c 'ggml-small.bin$' "$VERIFY_ROOT/argv.log")" -eq 2
test "$(grep -c '^clawed code$' "$VERIFY_ROOT/argv.log" || true)" -eq 0
python3 - "$VERIFY_ROOT/whisper.json" <<'PY'
import json
import pathlib
import sys

value = json.loads(pathlib.Path(sys.argv[1]).read_bytes())
assert value["engine"]["id"] == "whisper"
assert value["engine"]["model"] == "small"
assert value["engine"]["vad"] is False
assert value["language"] == {"requested": "de", "observed": "de", "probability": 0.958162}
assert value["hintCount"] == 1
PY

ECHO_CONFIG_DIR="$CONFIG_DIR" ECHO_DATA_DIR="$DATA_DIR" ECHO_MODEL_DIR="$MODEL_DIR" \
"$BIN" languages --engine whisper --format json > "$VERIFY_ROOT/languages-whisper.json"
ECHO_CONFIG_DIR="$CONFIG_DIR" ECHO_DATA_DIR="$DATA_DIR" ECHO_MODEL_DIR="$MODEL_DIR" \
"$BIN" languages --engine parakeet --format json > "$VERIFY_ROOT/languages-parakeet.json"
python3 - "$VERIFY_ROOT/languages-whisper.json" "$VERIFY_ROOT/languages-parakeet.json" <<'PY'
import json
import pathlib
import sys

whisper = json.loads(pathlib.Path(sys.argv[1]).read_bytes())
parakeet = json.loads(pathlib.Path(sys.argv[2]).read_bytes())
assert whisper["schemaVersion"] == 1
assert whisper["engine"] == "whisper"
assert len(whisper["languages"]) == 100
assert parakeet["selection"] == "automatic-only"
assert len(parakeet["languages"]) == 25
PY

printf '%s\n' 'verify-transcribe-cli: ok'
