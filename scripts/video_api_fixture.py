"""Real FFmpeg + installed native MCP routes. Disposable hosted CI only."""
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading


def main():
    assert os.environ.get("CI") == "true", "hosted CI only"
    binary = str(Path(os.environ["ANYMD_BIN"]).resolve())
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        source = root / "cuts.mkv"
        subprocess.run([
            "ffmpeg", "-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i",
            "color=c=black:s=64x48:r=10:d=1", "-f", "lavfi", "-i",
            "color=c=white:s=64x48:r=10:d=1", "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0,setpts=PTS+5/TB[v]", "-map", "[v]",
            "-c:v", "ffv1", "-copyts", str(source),
        ], check=True, timeout=30)
        source.with_suffix(".srt").write_text("1\n00:00:00,500 --> 00:00:01,500\ncross-cut\n")
        env = os.environ.copy()
        env["ANYMD_CACHE_DIR"] = str(root / "cache")
        for key in list(env):
            if key.startswith("MCP_PDF_REGION_ANALYSIS_") or key.startswith("MCP_PDF_OCR_"):
                env.pop(key)
        process = subprocess.Popen([binary, "mcp"], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   text=True, env=env)
        responses = queue.Queue()
        def reader():
            for line in process.stdout:
                responses.put(json.loads(line))
        threading.Thread(target=reader, daemon=True).start()
        sequence = 0
        def request(method, params):
            nonlocal sequence
            sequence += 1
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": sequence,
                                           "method": method, "params": params}) + "\n")
            process.stdin.flush()
            while True:
                response = responses.get(timeout=30)
                if response.get("id") == sequence:
                    assert "error" not in response, response
                    return response["result"]
        def call(name, arguments):
            result = request("tools/call", {"name": name, "arguments": arguments})
            assert not result.get("isError"), result
            return result
        try:
            request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                   "clientInfo": {"name": "video-fixture", "version": "1"}})
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
            process.stdin.flush()
            tools = request("tools/list", {})["tools"]
            assert {tool["name"] for tool in tools} == {"read", "search", "inspect", "outline"}
            selection = {"start_ms": 0, "end_ms": 2000, "caption": True}
            result = call("inspect", {"operation": "video_timeline", "sources": [{"path": str(source)}],
                                      "timeline": selection})
            body = result["structuredContent"]
            assert body["timeline"]["source_sha256"] == hashlib.sha256(source.read_bytes()).hexdigest()
            assert len(body["timeline"]["scenes"]) >= 2, body
            assert body["timeline"]["components"]["caption"]["status"] == "unavailable"
            assert not any(item["type"] == "image" for item in result["content"])
            assert any(cue["text"] == "cross-cut" and cue["end_ms"] == 1500 for cue in body["timeline"]["cues"])
            frame = call("inspect", {"operation": "render_frame", "sources": [{"path": str(source)}],
                                     "timestamps_ms": [100], "include_image": True,
                                     "expected_source_sha256": body["timeline"]["source_sha256"]})
            metadata = frame["structuredContent"]["frames"][0]["frame"]
            assert metadata["actual_ms"] >= metadata["requested_ms"]
            assert any(item["type"] == "image" for item in frame["content"])
            assert "image_path" not in frame["structuredContent"]["frames"][0]
            for name in ["outline", "read"]:
                projected = call(name, {"source": str(source), "timeline": selection})
                assert any("scene" in item.get("text", "") for item in projected["content"])
            process.stdin.close()
            process.wait(timeout=5)
            counter = root / "caption-count"
            adapter = root / "caption.py"
            adapter.write_text("import json,sys\nfrom pathlib import Path\np=Path(sys.argv[2]);p.write_text(str(int(p.read_text())+1) if p.exists() else '1')\nprint(json.dumps({'description':'fixture frame description','model_revision':'fixture-v1'}))\n")
            env["MCP_PDF_REGION_ANALYSIS_COMMAND"] = sys.executable
            env["MCP_PDF_REGION_ANALYSIS_ARGS_JSON"] = json.dumps([str(adapter), "{input}", str(counter)])
            process = subprocess.Popen([binary, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=subprocess.DEVNULL, text=True, env=env)
            threading.Thread(target=reader, daemon=True).start()
            request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                   "clientInfo": {"name": "caption-fixture", "version": "1"}})
            process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
            process.stdin.flush()
            arguments = {"operation": "video_timeline", "sources": [{"path": str(source)}], "timeline": selection}
            first = call("inspect", arguments)["structuredContent"]
            assert first["timeline"]["components"]["caption"]["status"] == "ok", first
            count = int(counter.read_text())
            assert count == len(first["descriptions"])
            call("inspect", arguments)
            call("outline", {"source": str(source), "timeline": selection})
            call("read", {"source": str(source), "timeline": selection})
            assert int(counter.read_text()) == count, "representatives were recaptioned"
            print("Real video MCP fixture passed: four tools, cues/frame/read/outline and local-caption cache plumbing")
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
