#!/usr/bin/env python3
"""Exercise a packaged formatter helper over stdio LSP."""

import json
import subprocess
import sys


def send(process: subprocess.Popen[bytes], message: dict) -> None:
    body = json.dumps(message, separators=(",", ":")).encode()
    process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    process.stdin.flush()


def receive(process: subprocess.Popen[bytes], request_id: int) -> dict:
    while True:
        headers: dict[str, str] = {}
        while line := process.stdout.readline():
            if line == b"\r\n":
                break
            name, value = line.decode().split(":", 1)
            headers[name.lower()] = value.strip()
        if "content-length" not in headers:
            raise RuntimeError("helper ended before returning an LSP response")
        response = json.loads(process.stdout.read(int(headers["content-length"])))
        if response.get("id") == request_id:
            return response


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} <zed-vertical-align-lsp-path>")

    process = subprocess.Popen(
        [sys.argv[1], "--stdio"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        send(
            process,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"processId": None, "rootUri": None, "capabilities": {}},
            },
        )
        initialize = receive(process, 1)
        if initialize.get("result", {}).get("capabilities", {}).get(
            "documentFormattingProvider"
        ) is not True:
            raise RuntimeError(f"helper did not advertise formatting: {initialize}")

        source = "def run():\n\tvalue = 1\n\tlong_name = 2\n"
        uri = "file:///zed-vertical-align-smoke.py"
        send(process, {"jsonrpc": "2.0", "method": "initialized", "params": {}})
        send(
            process,
            {
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "python",
                        "version": 1,
                        "text": source,
                    }
                },
            },
        )
        send(
            process,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "textDocument/formatting",
                "params": {
                    "textDocument": {"uri": uri},
                    "options": {"tabSize": 4, "insertSpaces": False},
                },
            },
        )
        edits = receive(process, 2).get("result") or []
        if len(edits) != 1 or "\tvalue" not in edits[0]["newText"]:
            raise RuntimeError(f"helper returned an unexpected formatting edit: {edits}")
    finally:
        process.terminate()
        process.wait(timeout=5)


if __name__ == "__main__":
    main()
