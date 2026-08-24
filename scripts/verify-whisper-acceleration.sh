#!/usr/bin/env bash
set -euo pipefail

python3 scripts/probe-whisper-acceleration.py --self-test

if [[ -n "${WHISPER_CPP_V192_SOURCE:-}" ]]; then
    scripts/build-whisper-vulkan-receipt.sh --check-source "${WHISPER_CPP_V192_SOURCE}"
fi
