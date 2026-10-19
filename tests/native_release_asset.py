"""Execute one exact release asset on its native OS/architecture.

Private HOME/XDG/cwd, one synthetic loopback response, memory-only, tools off.
No emulator, real provider, existing State, installer or PTY qualification.
"""
import argparse
import ctypes
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import tempfile
import threading
import tomllib


TARGETS = {
    "aw-linux-x86_64": ("Linux", "x86_64", "full", 62),
    "aw-linux-aarch64": ("Linux", "aarch64", "full", 183),
    "aw-macos-aarch64": ("Darwin", "aarch64", "full", 0x100000c),
    "aw-windows-x86_64.exe": ("Windows", "x86_64", "minimal", 0x8664),
    "aw-windows-aarch64.exe": ("Windows", "aarch64", "minimal", 0xaa64),
}
ANSWER = "native-asset-ok-界"
PROMPT = "synthetic-native-asset-界"


def check(condition, message):
    if not condition:
        raise RuntimeError(message)


def native_host():
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Windows":
        # OS machine, including when the Python launcher is running under WOW64.
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        current = kernel.GetCurrentProcess
        current.restype = ctypes.c_void_p
        query = kernel.IsWow64Process2
        query.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ushort),
                          ctypes.POINTER(ctypes.c_ushort)]
        query.restype = ctypes.c_int
        process_machine, os_machine = ctypes.c_ushort(), ctypes.c_ushort()
        check(query(current(), ctypes.byref(process_machine), ctypes.byref(os_machine)),
              "Cannot establish native Windows architecture")
        machine = {0x8664: "x86_64", 0xaa64: "aarch64"}.get(os_machine.value, "unsupported")
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    return system, machine


def admitted_image(bundle, asset, revision, version):
    system, machine, _, image_machine = TARGETS[asset]
    check(native_host() == (system, machine), "Asset requires a different native host")
    check(re.fullmatch(r"[0-9a-f]{40}", revision), "Expected an exact source revision")
    check((bundle / "source-revision").read_text(encoding="ascii") == revision + "\n",
          "Source revision mismatch")
    check((bundle / "VERSION").read_text(encoding="ascii") == version + "\n",
          "Release VERSION differs from checked-out Cargo.toml")
    data = (bundle / asset).read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    check((bundle / (asset + ".sha256")).read_text(encoding="ascii") ==
          f"{digest}  {asset}\n", "Asset checksum mismatch")
    if system == "Linux":
        check(data[:6] == b"\x7fELF\x02\x01" and
              struct.unpack_from("<H", data, 18)[0] == image_machine,
              "Expected matching ELF64 image")
    elif system == "Darwin":
        check(data[:4] == b"\xcf\xfa\xed\xfe" and
              struct.unpack_from("<I", data, 4)[0] == image_machine,
              "Expected matching Mach-O image")
    else:
        check(data[:2] == b"MZ", "Expected PE image")
        offset = struct.unpack_from("<I", data, 0x3c)[0]
        check(data[offset:offset + 4] == b"PE\0\0" and
              struct.unpack_from("<H", data, offset + 4)[0] == image_machine and
              struct.unpack_from("<H", data, offset + 24)[0] == 0x20b,
              "Expected matching PE32+ image")
    return data, digest


def exercise(binary, feature_cut, version, root):
    home, work, tmp = (root / name for name in ("home", "work", "tmp"))
    for directory in (home, work, tmp):
        directory.mkdir()
    # Preserve OS loader inputs only; no inherited AW/provider/proxy credentials.
    env = {key: value for key, value in os.environ.items()
           if key.upper() in {"PATH", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT"}}
    env.update(HOME=str(home), USERPROFILE=str(home),
               APPDATA=str(home / "roaming"), LOCALAPPDATA=str(home / "local"),
               XDG_CONFIG_HOME=str(home / "config"), XDG_DATA_HOME=str(home / "data"),
               XDG_STATE_HOME=str(home / "state"), XDG_CACHE_HOME=str(home / "cache"),
               TMPDIR=str(tmp), TMP=str(tmp), TEMP=str(tmp),
               TERM="dumb", NO_COLOR="1", AW_NATIVE_FIXTURE_KEY="synthetic-local-only")

    def run(args, code=0):
        result = subprocess.run([str(binary), *args], cwd=work, env=env,
                                stdin=subprocess.DEVNULL, capture_output=True,
                                encoding="utf-8", timeout=30)
        check(result.returncode == code,
              f"{args}: exit {result.returncode}: {result.stderr[-2000:]}")
        return result.stdout

    check(run(["--version"]).strip() == "aw " + version, "Wrong executable version")
    check("Usage:" in run(["--help"]), "Missing native root help")
    schema = json.loads(run(["cli", "schema"]))
    check(schema["schema"] == 2 and schema["feature_cut"] == feature_cut,
          "Wrong compiled CLI feature cut")
    check(("aw workspace" in schema["commands"]) == (feature_cut == "full"),
          "Unexpected Workspace command boundary")
    run(["cli", "not-a-real-action"], code=2)
    check(not list(home.rglob("*")) and not list(work.iterdir()),
          "Metadata probes created config/State/work files")

    requests, errors = [], []

    class Fixture(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                check(self.path == "/v1/chat/completions", "Unexpected fixture path")
                check(self.headers.get("Authorization") == "Bearer synthetic-local-only",
                      "Unexpected fixture credential")
                length = int(self.headers.get("Content-Length", "0"))
                check(0 < length <= 128 * 1024, "Unexpected fixture request size")
                body = json.loads(self.rfile.read(length))
                requests.append(body)
                check(len(requests) == 1, "Synthetic request was replayed")
                check(body["model"] == "fixture" and body["stream"] is True,
                      "Unexpected model/stream request")
                check(not body.get("tools"), "Tools advertised despite tools off")
                check(any(m.get("role") == "user" and m.get("content") == PROMPT
                          for m in body["messages"]), "Lost Unicode prompt")
                chunk = {"id": "synthetic", "object": "chat.completion.chunk",
                         "choices": [{"index": 0, "delta": {"role": "assistant", "content": ANSWER},
                                      "finish_reason": "stop"}],
                         "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}
                data = ("data: " + json.dumps(chunk) + "\n\ndata: [DONE]\n\n").encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except Exception as error:
                errors.append(str(error))
                self.send_error(500)

    class Server(HTTPServer):
        def get_request(self):
            connection, address = super().get_request()
            connection.settimeout(5)
            return connection, address

    with Server(("127.0.0.1", 0), Fixture) as server:
        config = work / "fixture.toml"
        config.write_text(f'default_profile="fixture"\n[profiles.fixture]\nkind="openai"\n'
                          f'base_url="http://127.0.0.1:{server.server_port}/v1"\n'
                          'model="fixture"\napi_key_env="AW_NATIVE_FIXTURE_KEY"\n'
                          'allow_insecure_http=true\n', encoding="utf-8")
        worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": .05}, daemon=True)
        worker.start()
        try:
            common = ["--config-file", str(config), "--provider-profile", "fixture",
                      "--state-memory-only", "--tools-mode", "off",
                      "--provider-timeout", "10", "--provider-idle-timeout", "5"]
            check(run([*common, "--output-print", "--", PROMPT]).strip() == ANSWER,
                  "Synthetic streaming answer differs")
            if feature_cut == "minimal":
                run([*common, "--ui-mode", "workspace", "--output-print", "--", PROMPT], code=2)
        finally:
            server.shutdown()
            worker.join(timeout=6)
        check(not worker.is_alive(), "Fixture did not stop")
    check(not errors and len(requests) == 1, f"Fixture failed: {errors}; requests={len(requests)}")
    check(not list(home.rglob("*")), "Memory-only probe created HOME/XDG files")
    return {"feature_cut": feature_cut, "synthetic_requests": len(requests), "unicode_stream": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("asset", choices=TARGETS)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    cargo = Path(__file__).resolve().parents[1] / "Cargo.toml"
    version = tomllib.loads(cargo.read_text(encoding="utf-8"))["package"]["version"]
    data, digest = admitted_image(args.bundle.resolve(), args.asset, args.revision, version)
    with tempfile.TemporaryDirectory(prefix="aw-native-asset-") as temp:
        root = Path(temp)
        install = root / "private install with spaces"
        install.mkdir()
        binary = install / ("aw.exe" if args.asset.endswith(".exe") else "aw")
        binary.write_bytes(data)  # Execute the checked bytes, without reopening the download.
        binary.chmod(0o700)
        result = exercise(binary, TARGETS[args.asset][2], version, root)
    print(json.dumps({"asset": args.asset, "sha256": digest, "revision": args.revision,
                      "native_host": native_host(), "version": version, **result}))


if __name__ == "__main__":
    main()
