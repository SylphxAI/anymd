"""Exercise the CLI with a real PTY without adding terminal dependencies to anymd."""
import concurrent.futures
import errno
import json
import os
from pathlib import Path
import pty
import select
import shutil
import subprocess
import sys
import tempfile
import wave

BINARY, ROOT = sys.argv[1:]
ROOT = Path(ROOT).resolve()
PDF = ROOT / "apps/web/public/samples/sample.pdf"
SINGLE = ROOT / "test/fixtures/sample.pdf"
VIDEO = ROOT / "apps/web/public/demo.mp4"
PRO = b"anymd Pro:"
PDF_SURFACE = b"entry_surface=cli_pdf"
VIDEO_SURFACE = b"entry_surface=cli_video"
BASE_ENV = os.environ.copy()
for name in ("CI", "ANYMD_OCR", "NO_PRO_HINT", "ANYMD_NO_PRO_HINT", "ANYMD_PRO_TOKEN", "MCP_TRANSPORT",
             "PDF_READER_MCP_TRANSPORT", "ANYMD_ALLOWED_DIRS", "ANYMD_ALLOW_DIRS"):
    BASE_ENV.pop(name, None)
BASE_ENV.update(ANYMD_NO_STAR_HINT="1")


def options(state, overrides):
    return {"cwd": ROOT, "env": {**BASE_ENV, "XDG_CONFIG_HOME": str(state),
                                "ANYMD_CACHE_DIR": str(state.parent / "cache"), **overrides}}


def marker(state):
    return state / "anymd/.pro-hint-shown"


def read_terminal(fd):
    chunks = []
    while True:
        try:
            chunk = os.read(fd, 4096)
        except OSError as error:
            if error.errno != errno.EIO:  # Linux PTY EOF
                raise
            break
        if not chunk:
            break
        chunks.append(chunk)
    return b"".join(chunks)


def cli(state, *args, terminal=True, **overrides):
    if not terminal:
        result = subprocess.run([BINARY, *args], capture_output=True, stdin=subprocess.DEVNULL,
                                timeout=20, **options(state, overrides))
        return result.returncode, result.stdout, result.stderr
    master, slave = pty.openpty()
    try:
        with subprocess.Popen([BINARY, *args], stdout=subprocess.PIPE, stderr=slave,
                              stdin=subprocess.DEVNULL, **options(state, overrides)) as child:
            os.close(slave)
            slave = None
            try:
                stdout, _ = child.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                child.kill()
                raise
            return child.returncode, stdout, read_terminal(master)
    finally:
        os.close(master)
        if slave is not None:
            os.close(slave)


def mcp(state):
    master, slave = pty.openpty()
    # Both directions are pipes for MCP. Keeping its stderr a TTY ensures the
    # absence of a hint comes from the CLI-only call site, not the terminal guard.
    try:
        with subprocess.Popen([BINARY, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=slave, **options(state, {})) as child:
            def send(method, params, message_id=None):
                message = {"jsonrpc": "2.0", "method": method, "params": params}
                if message_id is not None:
                    message["id"] = message_id
                child.stdin.write(json.dumps(message).encode() + b"\n")
                child.stdin.flush()
                if message_id is not None:
                    assert select.select([child.stdout], [], [], 15)[0], method
                    line = child.stdout.readline()
                    response = json.loads(line)
                    assert response["id"] == message_id and "result" in response, response
                    return response["result"]

            try:
                send("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                   "clientInfo": {"name": "pro-hint-test", "version": "1"}}, 1)
                send("notifications/initialized", {})
                result = send("tools/call", {"name": "read", "arguments": {"source": str(PDF)}}, 2)
                assert not result.get("isError") and "Quarterly Field Report" in str(result), result
                assert PRO.decode() not in str(result), result
                child.stdin.close()
                assert child.wait(timeout=15) == 0, child.returncode
                assert child.stdout.read() == b""
            finally:
                if child.poll() is None:
                    child.kill()
        os.close(slave)
        slave = None
        stderr = read_terminal(master)
        assert PRO not in stderr, stderr
        assert not marker(state).exists()
    finally:
        os.close(master)
        if slave is not None:
            os.close(slave)


with tempfile.TemporaryDirectory() as temp:
    tmp = Path(temp)
    state = tmp / "first"
    read_args = (str(PDF), "--no-ocr", "--images", "none")
    expected = cli(state, *read_args, terminal=False)
    assert expected[0] == 0 and b"Quarterly Field Report" in expected[1], expected
    assert expected[2] == b"" and not marker(state).exists(), expected
    for env in ({"ANYMD_NO_PRO_HINT": "1"}, {"NO_PRO_HINT": ""}, {"CI": "true"}):
        result = cli(state, *read_args, **env)
        assert result == expected and not marker(state).exists(), result
    code, stdout, stderr = cli(state, *read_args)
    assert (code, stdout) == expected[:2] and PRO not in stdout, (code, stdout)
    assert stderr.count(PRO) == 1 and PDF_SURFACE in stderr and b"anymd pro buy" in stderr, stderr
    assert marker(state).is_file() and cli(state, *read_args) == expected

    state = tmp / "ineligible"
    for args in (("--help",), ("--version",), (str(SINGLE),), (*read_args, "-p", "1"),
                 ("outline", str(SINGLE)), ("search", "content", str(SINGLE))):
        result = cli(state, *args)
        assert result[0] == 0 and PRO not in result[1] + result[2], result
        assert not marker(state).exists(), args
    for args in (("missing.pdf",), (*read_args, "missing.pdf"),
                 (*read_args, "--output", str(tmp / "missing" / "report.md"))):
        result = cli(state, *args)
        assert result[0] != 0 and PRO not in result[1] + result[2], result
        assert not marker(state).exists(), args
    mcp(state)

    state = tmp / "unwritable"
    state.touch()  # Create a regular file so this cannot be written even as root.
    assert cli(state, *read_args) == expected
    for mode, args in (("outline", ("outline", str(PDF), "--json")),
                       ("search", ("search", "Quarterly Field Report", str(PDF)))):
        state = tmp / mode
        plain = cli(state, *args, terminal=False)
        result = cli(state, *args)
        assert result[0] == 0 and result[:2] == plain[:2] and PDF_SURFACE in result[2], result
        if mode == "outline":
            assert isinstance(json.loads(result[1]), dict)

    state = tmp / "concurrent"
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(lambda _: cli(state, *read_args), range(4)))
    assert all(result[:2] == expected[:2] for result in results), results
    assert sum(result[2].count(PRO) for result in results) == 1 and marker(state).is_file(), results

    if shutil.which("ffprobe"):
        state = tmp / "media"
        audio = tmp / "audio.wav"
        with wave.open(str(audio), "wb") as pcm:
            pcm.setparams((1, 2, 8000, 8000, "NONE", "not compressed"))
            pcm.writeframes(bytes(16000))
        code, stdout, stderr = cli(state, str(audio))
        assert code == 0 and PRO not in stdout + stderr and not marker(state).exists(), stderr
        code, stdout, stderr = cli(state, str(VIDEO))
        assert code == 0 and PRO not in stdout and VIDEO_SURFACE in stderr, stderr
        assert marker(state).is_file()
