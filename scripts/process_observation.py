"""Run one command while collecting bounded Linux process and host evidence."""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal


@dataclass(frozen=True)
class ProcessTreeSample:
    monotonic_ns: int
    rss_bytes: int | None
    vm_swap_bytes: int | None
    mem_available_bytes: int | None
    host_swap_used_bytes: int | None


@dataclass(frozen=True)
class ProcessObservation:
    schema_version: int
    sampling: Literal["complete", "partial", "unavailable"]
    process_tree_peak_rss_bytes: int | None
    process_tree_peak_swap_bytes: int | None
    root_vm_hwm_bytes: int | None
    host_min_mem_available_bytes: int | None
    host_swap_used_before_bytes: int | None
    host_swap_used_peak_bytes: int | None
    host_swap_used_after_settle_bytes: int | None
    sample_count: int
    interval_ms: int
    settle_window_ms: int
    exit: Literal["success", "nonzero", "signaled", "timeout", "spawn-failed"]
    return_code: int | None
    sampling_issues: tuple[str, ...]
    samples: tuple[ProcessTreeSample, ...]

    def json(self) -> dict[str, object]:
        value = asdict(self)
        rename = {
            "schema_version": "schemaVersion",
            "process_tree_peak_rss_bytes": "processTreePeakRssBytes",
            "process_tree_peak_swap_bytes": "processTreePeakSwapBytes",
            "root_vm_hwm_bytes": "rootVmHwmBytes",
            "host_min_mem_available_bytes": "hostMinMemAvailableBytes",
            "host_swap_used_before_bytes": "hostSwapUsedBeforeBytes",
            "host_swap_used_peak_bytes": "hostSwapUsedPeakBytes",
            "host_swap_used_after_settle_bytes": "hostSwapUsedAfterSettleBytes",
            "sample_count": "sampleCount",
            "interval_ms": "intervalMs",
            "settle_window_ms": "settleWindowMs",
            "return_code": "returnCode",
            "sampling_issues": "samplingIssues",
        }
        for old, new in rename.items():
            value[new] = value.pop(old)
        value["samples"] = [
            {
                "monotonicNs": sample["monotonic_ns"],
                "rssBytes": sample["rss_bytes"],
                "vmSwapBytes": sample["vm_swap_bytes"],
                "memAvailableBytes": sample["mem_available_bytes"],
                "hostSwapUsedBytes": sample["host_swap_used_bytes"],
            }
            for sample in value["samples"]
        ]
        return value


def _read_status(root: Path, pid: int) -> dict[str, int] | None:
    try:
        result = {}
        for line in (root / str(pid) / "status").read_text().splitlines():
            key, _, raw = line.partition(":")
            if key in {"VmRSS", "VmSwap", "VmHWM"}:
                result[key] = int(raw.split()[0]) * 1024
        return result
    except (FileNotFoundError, PermissionError, ValueError, OSError):
        return None


def _read_children(root: Path, pid: int) -> list[int] | None:
    try:
        tasks = list((root / str(pid) / "task").iterdir())
    except (FileNotFoundError, PermissionError, OSError):
        return None
    children: set[int] = set()
    for task in tasks:
        if not task.name.isdigit():
            continue
        try:
            children.update(int(value) for value in (task / "children").read_text().split())
        except (ValueError, OSError):
            return None
    return sorted(children)


def _parent_snapshot(root: Path) -> tuple[dict[int, list[int]], set[int]] | None:
    try:
        entries = list(root.iterdir())
    except (FileNotFoundError, PermissionError, OSError):
        return None
    children: dict[int, list[int]] = {}
    known: set[int] = set()
    for entry in entries:
        if not entry.name.isdigit():
            continue
        try:
            pid = int(entry.name)
            lines = (entry / "status").read_text().splitlines()
            parent = next(
                int(line.partition(":")[2].strip())
                for line in lines
                if line.startswith("PPid:")
            )
        except FileNotFoundError:
            continue
        except (PermissionError, OSError, StopIteration, ValueError):
            return None
        known.add(pid)
        children.setdefault(parent, []).append(pid)
    for values in children.values():
        values.sort()
    return children, known


def _gone_or_zombie(root: Path, pid: int) -> bool:
    process = root / str(pid)
    if not process.exists():
        return True
    try:
        return any(
            line.startswith("State:") and line.partition(":")[2].strip().startswith("Z")
            for line in (process / "status").read_text().splitlines()
        )
    except (FileNotFoundError, PermissionError, OSError):
        return not process.exists()


def _tree(root: Path, pid: int) -> tuple[list[int], bool]:
    pending, result, partial = [pid], [], False
    fallback: tuple[dict[int, list[int]], set[int]] | None = None
    while pending:
        current = pending.pop()
        if current in result:
            continue
        if fallback is None:
            children = _read_children(root, current)
            if children is None:
                fallback = _parent_snapshot(root)
                if fallback is not None:
                    parent_map, known = fallback
                    children = parent_map.get(current, []) if current in known else None
        else:
            parent_map, known = fallback
            children = parent_map.get(current, []) if current in known else None
        if children is None and _gone_or_zombie(root, current):
            continue
        result.append(current)
        if children is None:
            partial = True
        else:
            pending.extend(children)
    return result, partial


def _host_values(root: Path) -> tuple[int | None, int | None]:
    try:
        values = {}
        for line in (root / "meminfo").read_text().splitlines():
            key, _, raw = line.partition(":")
            if key in {"MemAvailable", "SwapTotal", "SwapFree"}:
                values[key] = int(raw.split()[0]) * 1024
        return values.get("MemAvailable"), values["SwapTotal"] - values["SwapFree"]
    except (FileNotFoundError, PermissionError, KeyError, ValueError, OSError):
        return None, None


def observe(
    command: list[str],
    environment: dict[str, str],
    *,
    timeout_seconds: float,
    interval_ms: int,
    settle_window_ms: int,
    proc_root: Path = Path("/proc"),
) -> tuple[subprocess.CompletedProcess[str] | None, ProcessObservation, OSError | None]:
    if timeout_seconds <= 0 or interval_ms < 1 or settle_window_ms < 0:
        raise ValueError("invalid resource observation limits")
    samples: list[ProcessTreeSample] = []
    sampling_issues: set[str] = set()
    process_sample_failures = 0
    partial = False
    stop = threading.Event()
    process = None
    root_hwm = None
    baseline_available, baseline_swap = _host_values(proc_root)
    samples.append(
        ProcessTreeSample(
            time.monotonic_ns(), None, None, baseline_available, baseline_swap
        )
    )
    if baseline_available is None or baseline_swap is None:
        partial = True
        sampling_issues.add("host-memory-baseline-unavailable")

    def sample(settle: bool = False) -> None:
        nonlocal partial, process_sample_failures, root_hwm
        assert process is not None
        available, host_swap = _host_values(proc_root)
        if settle:
            if available is None or host_swap is None:
                partial = True
                sampling_issues.add("settle-host-memory-unavailable")
            samples.append(
                ProcessTreeSample(time.monotonic_ns(), None, None, available, host_swap)
            )
            return
        pids, unreadable = _tree(proc_root, process.pid)
        if unreadable:
            partial = True
            sampling_issues.add("process-children-unavailable")
        rss = vm_swap = 0
        root_gone = False
        for pid in pids:
            status = _read_status(proc_root, pid)
            if status is None:
                if pid == process.pid and (
                    process.poll() is not None or _gone_or_zombie(proc_root, pid)
                ):
                    root_gone = True
                    break
                if pid != process.pid and _gone_or_zombie(proc_root, pid):
                    continue
                unreadable = True
                sampling_issues.add("process-status-unavailable")
                continue
            if "VmRSS" not in status or "VmSwap" not in status:
                if pid == process.pid:
                    return
                if pid != process.pid and _gone_or_zombie(proc_root, pid):
                    continue
                unreadable = True
                sampling_issues.add("process-memory-fields-missing")
                continue
            rss += status["VmRSS"]
            vm_swap += status["VmSwap"]
            if pid == process.pid and "VmHWM" in status:
                root_hwm = max(root_hwm or 0, status["VmHWM"])
        if root_gone:
            return
        if unreadable:
            partial = True
            process_sample_failures += 1
        if available is None or host_swap is None:
            partial = True
        if available is None or host_swap is None:
            sampling_issues.add("host-memory-unavailable")
        samples.append(
            ProcessTreeSample(
                time.monotonic_ns(),
                None if unreadable else rss,
                None if unreadable else vm_swap,
                available,
                host_swap,
            )
        )

    def sampler() -> None:
        nonlocal partial
        try:
            while not stop.is_set():
                sample()
                stop.wait(interval_ms / 1000)
        except Exception:
            partial = True
            sampling_issues.add("sampler-failed")
            stop.set()

    try:
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=environment,
            start_new_session=True,
        )
    except OSError as error:
        host_swaps = [
            sample.host_swap_used_bytes
            for sample in samples
            if sample.host_swap_used_bytes is not None
        ]
        return (
            None,
            ProcessObservation(
                1,
                "unavailable",
                None,
                None,
                None,
                baseline_available,
                baseline_swap,
                max(host_swaps) if host_swaps else None,
                baseline_swap,
                len(samples),
                interval_ms,
                settle_window_ms,
                "spawn-failed",
                None,
                tuple(sorted(sampling_issues | {"spawn-failed"})),
                tuple(samples),
            ),
            error,
        )
    try:
        sample()
    except Exception:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
        raise
    thread = threading.Thread(target=sampler, daemon=True)
    thread.start()
    timed_out = False
    group_outlived_root = False
    try:
        try:
            stdout, stderr = process.communicate(timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            timed_out = True
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            stdout, stderr = process.communicate()
    finally:
        stop.set()
        thread.join()
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
    if not timed_out:
        try:
            os.killpg(process.pid, 0)
        except ProcessLookupError:
            pass
        else:
            group_outlived_root = True
            partial = True
            sampling_issues.add("process-group-outlived-root")
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    if settle_window_ms:
        time.sleep(settle_window_ms / 1000)
        sample(True)
    state = (
        "signaled"
        if group_outlived_root
        else "timeout"
        if timed_out
        else "success"
        if process.returncode == 0
        else "signaled"
        if process.returncode and process.returncode < 0
        else "nonzero"
    )
    process_samples = [item for item in samples if item.rss_bytes is not None]
    complete = (
        len(process_samples) >= 2
        and len(process_samples) > process_sample_failures
        and not partial
        and all(
            item.mem_available_bytes is not None
            and item.host_swap_used_bytes is not None
            for item in samples
        )
    )
    sampling = "complete" if complete else "partial" if samples else "unavailable"

    def values(name: str) -> list[int]:
        return [
            getattr(item, name) for item in samples if getattr(item, name) is not None
        ]

    rss, swaps, available, host_swaps = (
        values(name)
        for name in (
            "rss_bytes",
            "vm_swap_bytes",
            "mem_available_bytes",
            "host_swap_used_bytes",
        )
    )
    observation = ProcessObservation(
        1,
        sampling,
        max(rss) if rss else None,
        max(swaps) if swaps else None,
        root_hwm,
        min(available) if available else None,
        host_swaps[0] if host_swaps else None,
        max(host_swaps) if host_swaps else None,
        host_swaps[-1] if host_swaps else None,
        len(samples),
        interval_ms,
        settle_window_ms,
        state,
        -signal.SIGKILL if group_outlived_root else process.returncode,
        tuple(sorted(sampling_issues)),
        tuple(samples),
    )
    return (
        subprocess.CompletedProcess(command, process.returncode or 0, stdout, stderr),
        observation,
        None,
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        (root / "meminfo").write_text(
            "MemAvailable: 100 kB\nSwapTotal: 50 kB\nSwapFree: 20 kB\n"
        )
        for pid, status, children in (
            (1, "VmRSS: 2 kB\nVmSwap: 3 kB\nVmHWM: 4 kB\n", "2"),
            (2, "VmRSS: 5 kB\nVmSwap: 7 kB\n", ""),
        ):
            base = root / str(pid)
            (base / "task" / str(pid)).mkdir(parents=True)
            (base / "status").write_text(status)
            (base / "task" / str(pid) / "children").write_text(children)
        assert _host_values(root) == (102400, 30720) and _tree(root, 1) == (
            [1, 2],
            False,
        )
        assert (
            sum(_read_status(root, pid)["VmRSS"] for pid in _tree(root, 1)[0]) == 7168
        )
        for pid, parent in ((1, 0), (2, 1)):
            status = root / str(pid) / "status"
            status.write_text(f"PPid:\t{parent}\n" + status.read_text())
            (root / str(pid) / "task" / str(pid) / "children").unlink()
        assert _tree(root, 1) == ([1, 2], False)
        (root / "1" / "task" / "1" / "children").write_text("2")
        (root / "2" / "task" / "2" / "children").write_text("")
        (root / "2" / "status").unlink()
        assert _read_status(root, 2) is None
        (root / "2" / "status").write_text("State:\tZ (zombie)\n")
        assert _gone_or_zombie(root, 2)
        (root / "2").rename(root / "gone")
        assert _tree(root, 1) == ([1], False)
        child_directory = root / "2"
        child_directory.mkdir()
        assert _tree(root, 1) == ([1, 2], True)
    completed, observation, error = observe(
        [sys.executable, "-c", "import time; time.sleep(.2)"],
        dict(os.environ),
        timeout_seconds=2,
        interval_ms=5,
        settle_window_ms=5,
    )
    assert (
        error is None
        and completed is not None
        and observation.sampling == "complete"
        and observation.host_swap_used_after_settle_bytes is not None
    )
    _, observation, _ = observe(
        [sys.executable, "-c", "import time; time.sleep(2)"],
        dict(os.environ),
        timeout_seconds=0.05,
        interval_ms=5,
        settle_window_ms=0,
    )
    assert observation.exit == "timeout"
    with tempfile.TemporaryDirectory() as temporary:
        child_pid_path = Path(temporary) / "child.pid"
        child_code = (
            "import pathlib, subprocess, sys, time; "
            "child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(10)']); "
            f"pathlib.Path({str(child_pid_path)!r}).write_text(str(child.pid)); "
            "time.sleep(10)"
        )
        _, observation, _ = observe(
            [sys.executable, "-c", child_code],
            dict(os.environ),
            timeout_seconds=0.2,
            interval_ms=5,
            settle_window_ms=0,
        )
        assert observation.exit == "timeout"
        child_pid = int(child_pid_path.read_text())
        deadline = time.monotonic() + 1
        while (Path("/proc") / str(child_pid)).exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        assert not (Path("/proc") / str(child_pid)).exists()
    with tempfile.TemporaryDirectory() as temporary:
        child_pid_path = Path(temporary) / "outliving-child.pid"
        child_code = (
            "import pathlib, subprocess, sys; "
            "child = subprocess.Popen([sys.executable, '-c', "
            "'import time; time.sleep(10)'], stdout=subprocess.DEVNULL, "
            "stderr=subprocess.DEVNULL); "
            f"pathlib.Path({str(child_pid_path)!r}).write_text(str(child.pid))"
        )
        _, observation, _ = observe(
            [sys.executable, "-c", child_code],
            dict(os.environ),
            timeout_seconds=2,
            interval_ms=5,
            settle_window_ms=0,
        )
        assert observation.exit == "signaled"
        assert "process-group-outlived-root" in observation.sampling_issues
    _, observation, _ = observe(
        [sys.executable, "-c", "print('ok')"],
        dict(os.environ),
        timeout_seconds=2,
        interval_ms=5,
        settle_window_ms=0,
        proc_root=Path("/missing-proc"),
    )
    assert observation.sampling == "partial"
    _, observation, error = observe(
        ["/missing-command"],
        dict(os.environ),
        timeout_seconds=2,
        interval_ms=5,
        settle_window_ms=0,
    )
    assert error is not None and observation.exit == "spawn-failed"


if __name__ == "__main__":
    if sys.argv[1:] != ["--self-test"]:
        raise SystemExit("usage: process_observation.py --self-test")
    self_test()
    print("process-observation: self-test ok")
