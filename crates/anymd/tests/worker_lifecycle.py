"""Model-free lifecycle checks; only the isolated native CI harness invokes these."""
import ctypes
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

from anymd import convert, ConversionTimeoutError


def alive(pid):
    if os.name == "nt":
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.GetExitCodeProcess.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x1000, False, pid)
        if not handle:
            return False
        try:
            code = ctypes.c_ulong()
            return bool(kernel.GetExitCodeProcess(handle, ctypes.byref(code))) and code.value == 259
        finally:
            kernel.CloseHandle(handle)
    stat = Path("/proc/{}/stat".format(pid))
    if stat.exists() and stat.read_text().rsplit(")", 1)[1].split()[0] == "Z":
        return False  # An exited orphan awaiting init reaping is not running.
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


class WorkerLifecycleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if os.environ.get("CI") != "true":
            raise unittest.SkipTest("isolated CI only; never run on the desk")
        cls.binary = os.environ["ANYMD_LIFECYCLE_BIN"]

    def assert_stopped(self, pid):
        deadline = time.monotonic() + 5
        while alive(pid) and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertFalse(alive(pid), "process {} still running".format(pid))

    def test_python_timeout_kills_cli_and_separate_group_native_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.txt"
            source.write_text("fixture")
            started = time.monotonic()
            with self.assertRaises(ConversionTimeoutError):
                convert(source, binary=self.binary, timeout=2)
            self.assertLess(time.monotonic() - started, 7)
            parent = int(source.with_suffix(".parent").read_text())
            worker = int(source.with_suffix(".worker").read_text().splitlines()[0])
            self.assertNotEqual(parent, worker)
            self.assert_stopped(parent)
            self.assert_stopped(worker)

    def test_worker_eof_terminates_without_model_load(self):
        with tempfile.TemporaryDirectory() as directory:
            worker = subprocess.Popen(
                [self.binary, "--worker", str(Path(directory) / "heartbeat"), "--supervised=30000"],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            try:
                worker.stdin.close()
                self.assertEqual(worker.wait(timeout=5), 124)
            finally:
                if worker.poll() is None:
                    worker.kill()
                worker.wait()

    def test_worker_deadline_terminates_with_live_parent(self):
        with tempfile.TemporaryDirectory() as directory:
            worker = subprocess.Popen(
                [self.binary, "--worker", str(Path(directory) / "heartbeat"), "--supervised=100"],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            try:
                self.assertEqual(worker.wait(timeout=5), 124)
            finally:
                worker.stdin.close()
                if worker.poll() is None:
                    worker.kill()
                worker.wait()

    def test_direct_benchmark_worker_does_not_require_liveness_pipe(self):
        with tempfile.TemporaryDirectory() as directory:
            worker = subprocess.Popen(
                [self.binary, "--worker", str(Path(directory) / "heartbeat")],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            try:
                time.sleep(0.2)
                self.assertIsNone(worker.poll())
            finally:
                worker.kill()
                worker.wait()


if __name__ == "__main__":
    unittest.main()
