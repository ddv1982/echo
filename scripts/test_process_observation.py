"""Focused regressions for process-tree sampling."""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from process_observation import _read_children, _tree


class ProcessObservationTests(unittest.TestCase):
    def test_tree_collects_children_from_every_thread(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "meminfo").write_text("MemAvailable: 1 kB\nSwapTotal: 1 kB\nSwapFree: 1 kB\n")
            for pid, children in ((1, "2"), (2, ""), (3, "")):
                (root / str(pid) / "task" / str(pid)).mkdir(parents=True)
                (root / str(pid) / "status").write_text("State:\tS (sleeping)\n")
                (root / str(pid) / "task" / str(pid) / "children").write_text(children)
            (root / "1" / "task" / "10").mkdir()
            (root / "1" / "task" / "10" / "children").write_text("3 2")

            self.assertEqual(_read_children(root, 1), [2, 3])
            self.assertEqual(sorted(_tree(root, 1)[0]), [1, 2, 3])
            self.assertFalse(_tree(root, 1)[1])

    def test_unreadable_children_falls_back_or_marks_partial(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for pid, parent in ((1, 0), (2, 1)):
                (root / str(pid) / "task" / str(pid)).mkdir(parents=True)
                (root / str(pid) / "status").write_text(f"PPid:\t{parent}\n")

            self.assertEqual(_tree(root, 1), ([1, 2], False))

            (root / "2" / "status").write_text("PPid:\tnot-a-pid\n")
            self.assertEqual(_tree(root, 1), ([1], True))

    def test_tree_finds_child_spawned_by_worker_thread(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            child_pid_file = Path(temporary) / "child.pid"
            program = f"""import pathlib
import subprocess
import sys
import threading
import time

ready = threading.Event()
release = threading.Event()
def worker():
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(10)"])
    target = pathlib.Path({str(child_pid_file)!r})
    temporary = target.with_name(target.name + ".tmp")
    temporary.write_text(str(child.pid))
    temporary.replace(target)
    ready.set()
    release.wait()

threading.Thread(target=worker).start()
ready.wait()
time.sleep(10)
"""
            parent = subprocess.Popen([sys.executable, "-c", program], start_new_session=True)
            try:
                deadline = time.monotonic() + 2
                while not child_pid_file.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(child_pid_file.exists(), "worker did not spawn its child")
                child_pid = int(child_pid_file.read_text())
                self.assertIn(child_pid, _tree(Path("/proc"), parent.pid)[0])
            finally:
                try:
                    os.killpg(parent.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                parent.wait()


if __name__ == "__main__":
    unittest.main()
