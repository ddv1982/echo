#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verify_root=$(mktemp -d "${TMPDIR:-/tmp}/echo-stt-benchmark.XXXXXX")
trap 'rm -rf "$verify_root"' EXIT

python3 "$repo_root/scripts/process_observation.py" --self-test
python3 "$repo_root/scripts/test_process_observation.py"
python3 "$repo_root/scripts/benchmark-stt.py" --self-test
python3 "$repo_root/scripts/probe-whisper-resident.py" --self-test
if [[ ${ECHO_STT_BENCHMARK_SKIP_BUILD:-0} == 1 ]]; then
  test -x "$repo_root/target/debug/echo-desktop"
else
  cargo build --manifest-path "$repo_root/Cargo.toml" -p echo-desktop >/dev/null
fi

python3 - "$repo_root" "$verify_root" <<'PY'
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

repo = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
benchmark = repo / "scripts" / "benchmark-stt.py"
binary = repo / "target" / "debug" / "echo-desktop"
fixtures = repo / "benchmarks" / "stt" / "fixtures.json"


def command(
    output: pathlib.Path,
    executable: pathlib.Path = binary,
    manifest: pathlib.Path = fixtures,
) -> list[str]:
    return [
        sys.executable,
        str(benchmark),
        "--binary",
        str(executable),
        "--manifest",
        str(manifest),
        "--candidate",
        "fake",
        "--repeats",
        "2",
        "--warmups",
        "1",
        "--seed",
        "42",
        "--output-dir",
        str(output),
    ]


def run(output: pathlib.Path, **kwargs: object) -> subprocess.CompletedProcess[str]:
    environment = dict(os.environ)
    environment["ECHO_STT_BENCHMARK_VERIFY_SECRET"] = "not-for-artifacts"
    environment["ECHO_MODEL_DIR"] = str(root / "model-cache")
    return subprocess.run(
        command(output, **kwargs),
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )


def read_status(output: pathlib.Path) -> dict[str, object]:
    return json.loads((output / "status.json").read_text(encoding="utf-8"))


def assert_artifacts(output: pathlib.Path, artifact: dict[str, object]) -> None:
    for name in ("command", "environment", "stdout", "stderr", "result", "timing"):
        reference = artifact[name]
        assert isinstance(reference, dict)
        relative = reference["path"]
        assert isinstance(relative, str) and not pathlib.PurePosixPath(relative).is_absolute()
        path = output / relative
        assert path.is_file()
        contents = path.read_bytes()
        assert reference["bytes"] == len(contents)
        assert reference["sha256"] == hashlib.sha256(contents).hexdigest()
    resource = artifact["processObservation"]
    resource_path = output / resource["path"]
    assert resource_path.name == "process-observation.json"
    assert resource["sha256"] == hashlib.sha256(resource_path.read_bytes()).hexdigest()
    observation = json.loads(resource_path.read_text(encoding="utf-8"))
    assert observation["sampling"] in {"complete", "partial", "unavailable"}
    assert observation["exit"] in {"success", "nonzero", "signaled", "timeout", "spawn-failed"}


report = root / "report"
completed = run(report)
assert completed.returncode == 0, completed.stderr
status = read_status(report)
bundle = json.loads((report / "run-manifest.json").read_text(encoding="utf-8"))
rows = [json.loads(line) for line in (report / "runs.jsonl").read_text().splitlines()]
assert len(rows) == 4
assert all(row["schemaVersion"] == 2 for row in rows)
assert all(row["seed"] == 42 and row["warmups"] == 1 for row in rows)
assert all(row["outerMs"] >= row["inferMs"] for row in rows)
assert all(len(row["echoBinary"]["sha256"]) == 64 for row in rows)
assert status["state"] == "complete" and status["runId"] == bundle["runId"]
assert bundle["seed"] == 42 and bundle["repeats"] == 2 and bundle["warmups"] == 1
assert bundle["binary"] == rows[0]["echoBinary"]
snapshot = bundle["corpus"]["snapshot"]
snapshot_path = report / snapshot["path"]
assert snapshot_path.is_file()
assert hashlib.sha256(snapshot_path.read_bytes()).hexdigest() == snapshot["sha256"]
assert len(bundle["artifactIndex"]) == 6
for row in rows:
    artifact = row["observationArtifact"]
    assert artifact["rowId"] == row["rowId"]
    assert artifact in bundle["artifactIndex"]
    assert_artifacts(report, artifact)
    result = json.loads((report / artifact["result"]["path"]).read_text())
    environment = json.loads((report / artifact["environment"]["path"]).read_text())
    assert result["productJson"]["text"] == row["text"]
    assert "ECHO_STT_BENCHMARK_VERIFY_SECRET" not in environment
    assert environment["ECHO_MODEL_DIR"] == str(root / "model-cache")
speech = [row for row in rows if row["utterance"] == "claude-code-en"]
silence = [row for row in rows if row["utterance"] == "silence"]
assert all(row["wordErrors"] == 0 and row["wer"] == 0 for row in speech)
assert all(row["text"] == "" and not row["hallucinatedSilence"] for row in silence)
summary = (report / "summary.md").read_text()
assert "| fake | en | 0.00%" in summary
assert "| fake | auto | n/a" in summary

stale = run(report)
assert stale.returncode == 1 and "must be empty" in stale.stderr
assert read_status(report)["state"] == "complete"


def write_wrapper(name: str, body: str) -> pathlib.Path:
    wrapper = root / name
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import os\n"
        "import pathlib\n"
        "import subprocess\n"
        "import sys\n"
        "import time\n"
        f"real = {str(binary)!r}\n"
        + body,
        encoding="utf-8",
    )
    wrapper.chmod(0o755)
    return wrapper


failure_wrapper = write_wrapper(
    "fail-one.py",
    """if sys.argv[1:] == ["--version"]:
    os.execv(real, [real, *sys.argv[1:]])
if sys.argv[1] == "transcribe":
    print("forced child failure", file=sys.stderr)
    raise SystemExit(23)
os.execv(real, [real, *sys.argv[1:]])
""",
)
failed_output = root / "failed"
failed = run(failed_output, executable=failure_wrapper)
assert failed.returncode == 1 and "forced child failure" in failed.stderr
failed_status = read_status(failed_output)
failed_bundle = json.loads((failed_output / "run-manifest.json").read_text())
assert failed_status["state"] == "failed" and failed_status["failure"]["type"] == "RuntimeError"
assert len(failed_bundle["artifactIndex"]) == 1
assert not (failed_output / "runs.jsonl").exists() and not (failed_output / "summary.md").exists()
failed_artifact = failed_bundle["artifactIndex"][0]
assert_artifacts(failed_output, failed_artifact)
assert (
    json.loads((failed_output / failed_artifact["result"]["path"]).read_text())["productJson"]
    is None
)

input_root = root / "mutable-input"
input_root.mkdir()
mutable_audio = input_root / "sample.wav"
source_fixture = json.loads(fixtures.read_text(encoding="utf-8"))["utterances"][0]
shutil.copyfile(fixtures.parent / source_fixture["file"], mutable_audio)
mutable_manifest = input_root / "fixtures.json"
mutable_manifest.write_text(
    json.dumps(
        {
            "schemaVersion": 1,
            "utterances": [
                {"id": "sample", "file": mutable_audio.name, "language": "en", "reference": "test"}
            ],
        }
    ),
    encoding="utf-8",
)
mutation_wrapper = write_wrapper(
    "mutate-after-child.py",
    """if sys.argv[1:] == ["--version"]:
    os.execv(real, [real, *sys.argv[1:]])
completed = subprocess.run([real, *sys.argv[1:]], capture_output=True, text=True)
path = pathlib.Path(sys.argv[2])
path.write_bytes(path.read_bytes() + b"!")
sys.stdout.write(completed.stdout)
sys.stderr.write(completed.stderr)
raise SystemExit(completed.returncode)
""",
)
mutated_output = root / "mutated"
mutated = run(mutated_output, executable=mutation_wrapper, manifest=mutable_manifest)
assert mutated.returncode == 1 and "audio fixture changed during benchmark" in mutated.stderr
mutated_status = read_status(mutated_output)
mutated_bundle = json.loads((mutated_output / "run-manifest.json").read_text())
assert mutated_status["state"] == "failed" and len(mutated_bundle["artifactIndex"]) == 1
assert not (mutated_output / "runs.jsonl").exists()

marker = root / "interrupted-child-started"
interruption_wrapper = write_wrapper(
    "interruptible.py",
    f"""if sys.argv[1:] == ["--version"]:
    os.execv(real, [real, *sys.argv[1:]])
pathlib.Path({str(marker)!r}).write_text("started", encoding="utf-8")
time.sleep(3)
os.execv(real, [real, *sys.argv[1:]])
""",
)
interrupted_output = root / "interrupted"
environment = dict(os.environ)
interrupted = subprocess.Popen(
    command(interrupted_output, executable=interruption_wrapper),
    cwd=repo,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    env=environment,
)
for _ in range(50):
    if marker.exists():
        break
    time.sleep(0.1)
else:
    interrupted.kill()
    raise AssertionError("interruption test did not start a child process")
assert read_status(interrupted_output)["state"] == "running"
interrupted.terminate()
stdout, stderr = interrupted.communicate(timeout=10)
assert interrupted.returncode == 1, (stdout, stderr)
interrupted_status = read_status(interrupted_output)
assert interrupted_status["state"] == "failed"
assert interrupted_status["failure"]["type"] == "BenchmarkInterrupted"
assert not (interrupted_output / "runs.jsonl").exists()
PY

printf '%s\n' 'verify-stt-benchmark: ok'
