#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verify_root=$(mktemp -d "${TMPDIR:-/tmp}/echo-stt-benchmark.XXXXXX")
trap 'rm -rf "$verify_root"' EXIT

python3 "$repo_root/scripts/benchmark-stt.py" --self-test
python3 "$repo_root/scripts/probe-whisper-resident.py" --self-test
cargo build --manifest-path "$repo_root/Cargo.toml" -p echo-desktop >/dev/null

python3 "$repo_root/scripts/benchmark-stt.py" \
  --binary "$repo_root/target/debug/echo-desktop" \
  --manifest "$repo_root/benchmarks/stt/fixtures.json" \
  --candidate fake \
  --repeats 2 \
  --warmups 1 \
  --seed 42 \
  --output-dir "$verify_root/report"

python3 - "$verify_root/report" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
rows = [json.loads(line) for line in (root / "runs.jsonl").read_text().splitlines()]
assert len(rows) == 4
assert all(row["schemaVersion"] == 2 for row in rows)
assert all(row["seed"] == 42 and row["warmups"] == 1 for row in rows)
assert all(row["outerMs"] >= row["inferMs"] for row in rows)
assert all(len(row["echoBinary"]["sha256"]) == 64 for row in rows)
speech = [row for row in rows if row["utterance"] == "claude-code-en"]
silence = [row for row in rows if row["utterance"] == "silence"]
assert all(row["wordErrors"] == 0 and row["wer"] == 0 for row in speech)
assert all(row["text"] == "" and not row["hallucinatedSilence"] for row in silence)
summary = (root / "summary.md").read_text()
assert "| fake | en | 0.00%" in summary
assert "| fake | auto | n/a" in summary
PY

printf '%s\n' 'verify-stt-benchmark: ok'
