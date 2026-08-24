#!/usr/bin/env bash
set -euo pipefail

python3 scripts/probe-whisper-acceleration.py --self-test
python3 scripts/collect-whisper-host-evidence.py --self-test
python3 scripts/run-whisper-cache-cycle.py --self-test
python3 scripts/run-whisper-cache-cycle.py \
    --validate-cycle .audit/whisper-phase4-cache-cycle

if [[ -n "${WHISPER_CPP_V192_SOURCE:-}" ]]; then
    scripts/build-whisper-vulkan-receipt.sh --check-source "${WHISPER_CPP_V192_SOURCE}"
fi
