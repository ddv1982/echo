#!/usr/bin/env bash
set -euo pipefail

python3 scripts/fetch-stt-corpus.py --self-test
python3 scripts/fetch-stt-corpus.py \
  --check-manifest \
  --manifest benchmarks/stt/corpus-fleurs.json
python3 scripts/analyze-stt-host-matrix.py --self-test
python3 - <<'PY'
import json
from pathlib import Path

manifest = json.loads(Path("benchmarks/stt/corpus-fleurs.json").read_text())
assert manifest["schemaVersion"] == 1
assert manifest["source"]["license"] == "CC-BY-4.0"
assert len(manifest["utterances"]) == 20
languages = [item["language"] for item in manifest["utterances"]]
assert {language: languages.count(language) for language in set(languages)} == {
    "de": 4,
    "en": 4,
    "es": 4,
    "fr": 4,
    "nl": 4,
}
assert all(item["class"] == "clean-read" for item in manifest["utterances"])
print("verify-stt-corpus: ok")
PY
