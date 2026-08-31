#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/.." && pwd)
commit=$(git -C "$repo_dir" rev-parse HEAD)
target_dir=${CARGO_TARGET_DIR:-"$repo_dir/target"}
fixture=empty
if [ "${1:-}" = --existing ]; then
  fixture=existing
  shift
fi
output=${1:-"$target_dir/status-perf/$commit-$fixture.json"}
perf_root=$(mktemp -d /tmp/echo-status-perf.XXXXXX)

cleanup() {
  case "$perf_root" in
    /tmp/echo-status-perf.*) rm -rf -- "$perf_root" ;;
    *) printf 'refusing to remove unexpected temporary path: %s\n' "$perf_root" >&2 ;;
  esac
}
trap cleanup EXIT

mkdir -p "$(dirname "$output")" "$perf_root/data" "$perf_root/config" "$perf_root/models"
printf '{"rows":[]}\n' > "$perf_root/data/history.json"

(
  cd "$repo_dir"
  probe_dist="$perf_root/frontend-dist"
  VITE_STATUS_PERF_PROBE=1 npm run build --prefix frontend -- \
    --outDir "$probe_dist" --emptyOutDir
  tauri_config="{\"build\":{\"frontendDist\":\"$probe_dist\"}}"
  ECHO_BUILD_SHA="$commit" TAURI_CONFIG="$tauri_config" cargo build --release -p echo-desktop \
    --features status-perf-probe
)

run_log="$perf_root/run.log"
set +e
runtime_env=(GDK_BACKEND=x11)
if [ "$fixture" = empty ]; then
  runtime_env+=(
    ECHO_DATA_DIR="$perf_root/data"
    ECHO_CONFIG_DIR="$perf_root/config"
    ECHO_MODEL_DIR="$perf_root/models"
    ECHO_ENGINE=fake
  )
fi
timeout 180s xvfb-run -a env "${runtime_env[@]}" \
  "$target_dir/release/echo-desktop" >"$run_log" 2>&1
run_status=$?
set -e
if [ "$run_status" -ne 0 ]; then
  cat "$run_log" >&2
  exit "$run_status"
fi

report=$(sed -n 's/^STATUS_PERF_JSON //p' "$run_log" | tail -n 1)
if [ -z "$report" ]; then
  cat "$run_log" >&2
  printf 'status performance report was not emitted\n' >&2
  exit 1
fi
printf '%s\n' "$report" > "$output"

python3 - "$output" "$commit" "$fixture" <<'PY'
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
commit = sys.argv[2]
fixture = sys.argv[3]
document = json.loads(path.read_text())
if document.get("commit") != commit:
    raise SystemExit("status performance commit does not match")
lanes = document.get("report", {}).get("lanes")
stages = document.get("statusStages")
cold = document.get("coldStatusStage")
if not isinstance(lanes, list) or [lane.get("name") for lane in lanes] != [
    "noop",
    "fixed-status",
    "current-status",
]:
    raise SystemExit("status performance lanes are invalid")
if not isinstance(stages, list) or len(stages) != 40:
    raise SystemExit("status performance stage sample count is invalid")
if not isinstance(cold, dict) or not cold:
    raise SystemExit("cold status performance stage sample is invalid")
document["fixture"] = fixture
path.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n")
for lane in lanes:
    summary = lane["summary"]
    print(
        f"{lane['name']}: count={summary['count']} "
        f"p50={summary['p50Ms']:.3f}ms p95={summary['p95Ms']:.3f}ms"
    )
print(f"status-stage-samples: {len(stages)}")
print(f"cold-status-total: {cold['totalUs'] / 1000:.3f}ms")
for name in stages[0]:
    values = sorted(sample[name] for sample in stages)
    rank = (len(values) - 1) * 0.95
    lower = int(rank)
    upper = min(lower + 1, len(values) - 1)
    p95 = values[lower] + (values[upper] - values[lower]) * (rank - lower)
    print(f"{name}: p50={values[len(values) // 2]:.1f}us p95={p95:.1f}us")
print(f"report: {path}")
PY
