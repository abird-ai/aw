#!/usr/bin/env python3
"""Independent Python stdio RPC client for aw v1.

Does not import any aw internals or make web calls. Transport loss after a
request write is UNKNOWN, never "not dispatched" and never auto-retried.
"""
import argparse
import json
import subprocess
import sys

MAX_FRAME = 640 * 1024
MAX_REQUEST = 192 * 1024

class UncertainIO(Exception):
    """The request may have caused paid/provider/tool effects before pipe loss."""

class RpcClient:
    def __init__(self, reader, writer):
        self.reader = reader
        self.writer = writer
        self.seq = 0
        self.next_id = 0
        hello = self._read()
        if hello.get("type") != "hello" or hello["data"].get("protocol") != "aw.rpc":
            raise ValueError("not a versioned aw RPC stdio handshake")
        self.capabilities = hello["data"]["capabilities"]

    def _read(self):
        try:
            line = self.reader.readline(MAX_FRAME + 2)
        except OSError as error:
            raise UncertainIO("stdio RPC read failed; inspect saved Session") from error
        if not line:
            raise UncertainIO("stdio RPC ended; inspect saved Session before any resend")
        if len(line) > MAX_FRAME or not line.endswith(b"\n"):
            raise ValueError("oversized or unterminated RPC frame")
        value = json.loads(line)
        if value.get("v") != 1 or value.get("seq", 0) <= self.seq:
            raise ValueError("invalid version or nonmonotonic JSONL sequence")
        self.seq = value["seq"]
        return value

    def call(self, method, params=None, on_event=lambda _: None):
        self.next_id += 1
        ident = f"python_{self.next_id}"
        request = json.dumps({
            "v": 1, "id": ident, "method": method, "params": params or {}
        }, separators=(",", ":")).encode()
        if len(request) > MAX_REQUEST:
            raise ValueError("request exceeds bounded protocol line")
        try:
            self.writer.write(request + b"\n")
            self.writer.flush()
        except OSError as error:
            raise UncertainIO("RPC write failed; request may already be executing") from error
        accepted = False
        while True:
            frame = self._read()
            if frame.get("id") != ident:
                raise ValueError("unexpected concurrent response ID")
            if frame["type"] == "accepted":
                accepted = True
                on_event(frame)
            elif frame["type"] == "response":
                return frame["data"]
            elif frame["type"] == "settled":
                if not accepted:
                    raise ValueError("settlement without prior accepted record")
                return frame["data"]
            elif frame["type"] == "error":
                raise ValueError(f"RPC rejected: {frame['data'].get('code', 'unspecified')}")
            else:
                on_event(frame)

def demo(binary, base_url, full):
    command = [binary]
    if full:
        command += ["rpc", "--agent-stdio"]
    command += ["--state-memory-only", "--provider-profile", "llama", "--provider-base-url", base_url,
                "--model-id", "mock", "--tools-mode", "off"]
    if not full:
        command += ["rpc", "--agent-stdio"]
    with subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE) as child:
        client = RpcClient(child.stdout, child.stdin)
        before = client.call("get_state")
        if before["next_turn"] != 1:
            raise ValueError("unexpected initial state")
        result = client.call("prompt", {"text": "independent-python-client"})
        if result["result"]["outcome"] != "completed":
            raise ValueError("prompt did not settle")
        after = client.call("get_state")
        if after["next_turn"] != 2:
            raise ValueError("wrong next turn after prompt")
        child.stdin.close()
        if child.wait(timeout=10) != 0:
            raise ValueError("RPC child did not exit cleanly")
        print(json.dumps({"client": "python-independent", "status": "settled",
                          "next_turn": after["next_turn"]}))

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--full", action="store_true")
    args = parser.parse_args()
    try:
        demo(args.binary, args.base_url, args.full)
    except (UncertainIO, ValueError) as error:
        print(f"RPC client: {error}", file=sys.stderr)
        raise SystemExit(2)
