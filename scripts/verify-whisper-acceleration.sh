#!/usr/bin/env bash
set -euo pipefail

python3 scripts/probe-whisper-acceleration.py --self-test
python3 scripts/collect-whisper-host-evidence.py --self-test
python3 scripts/run-whisper-cache-cycle.py --self-test
python3 scripts/sweep-whisper-admission.py --self-test
python3 scripts/whisper_release_common.py
python3 scripts/whisper_identity_v3.py --self-test
python3 scripts/verify-whisper-behavior-contract.py --self-test
python3 scripts/verify-whisper-invalidation.py --self-test
python3 scripts/verify-pr16-2-evidence.py
python3 scripts/promote-whisper-admission.py --self-test
python3 scripts/compose-whisper-admission-set.py --self-test
python3 scripts/patch-tauri-bundle-type.py --self-test
python3 scripts/stage-qualified-whisper-release.py --self-test
python3 scripts/prepare-whisper-local-selection.py --self-test
python3 scripts/verify-whisper-local-selection.py --self-test
python3 scripts/run-whisper-cache-cycle.py \
    --validate-cycle .audit/whisper-phase4-cache-cycle

if [[ -n "${WHISPER_CPP_V192_SOURCE:-}" ]]; then
    scripts/build-whisper-vulkan-receipt.sh --check-source "${WHISPER_CPP_V192_SOURCE}"
fi

if [[ -n "${WHISPER_CPP_V193_SOURCE:-}" ]]; then
    scripts/build-whisper-vulkan-receipt.sh --revision v1.9.3 \
        --check-source "${WHISPER_CPP_V193_SOURCE}"
fi
