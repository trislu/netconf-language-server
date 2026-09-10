#!/usr/bin/env python3
"""Drive a NETCONF language server through one `initialize` and time the scan.

Usage:
    python3 scripts/lsp_scan_driver.py <server-binary> <root-dir|root-uri> [timeout-seconds]

Spawns the server, sends `initialize` with `rootUri`, prints every
`window/logMessage` notification with a relative timestamp, and exits once the
`initialize` response arrives (or after the timeout). This is the harness that
produces the user-visible startup number in
docs/perf/catalog-scan-regression-2026-09-11.md -- section 3.1 / Appendix A.

No third-party dependencies; Python 3 only.
"""

import json
import select
import subprocess
import sys
import time
from pathlib import Path


def frame(obj):
    """Encode one LSP message with its Content-Length header."""
    body = json.dumps(obj).encode()
    return b"Content-Length: %d\r\n\r\n" % len(body) + body


def extract_msg(buf):
    """Pop one complete LSP message from `buf`; return (message_or_None, rest)."""
    i = buf.find(b"\r\n\r\n")
    if i < 0:
        return None, buf
    headers = buf[:i].decode(errors="replace").split("\r\n")
    length = None
    for header in headers:
        if header.lower().startswith("content-length"):
            length = int(header.split(":", 1)[1])
            break
    if length is None or len(buf) < i + 4 + length:
        return None, buf
    body = buf[i + 4:i + 4 + length]
    try:
        return json.loads(body), buf[i + 4 + length:]
    except json.JSONDecodeError:
        return None, buf[i + 4 + length:]


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    binpath, target = argv[1], argv[2]
    timeout = float(argv[3]) if len(argv) > 3 else 900.0
    uri = target if "://" in target else Path(target).resolve().as_uri()

    proc = subprocess.Popen(
        [binpath], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )

    try:
        proc.stdin.write(frame({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"processId": None, "rootUri": uri, "capabilities": {}},
        }))
        proc.stdin.flush()
    except BrokenPipeError:
        print("server closed stdin before initialize could be sent", file=sys.stderr)
        proc.kill()
        return 1

    t0 = time.time()
    deadline = t0 + timeout
    buf = b""
    status = 0

    while True:
        msg, buf = extract_msg(buf)
        if msg is not None:
            el = time.time() - t0
            if msg.get("method") == "window/logMessage":
                print(f"[{el:8.3f}s] LOG: {msg.get('params', {}).get('message')}",
                      flush=True)
            elif "id" in msg and "method" not in msg:
                print(f"[{el:8.3f}s] initialize done", flush=True)
                break
            continue

        remaining = deadline - time.time()
        if remaining <= 0:
            print(f"[{time.time() - t0:8.3f}s] timeout after {timeout:.1f}s",
                  file=sys.stderr)
            status = 1
            break

        ready, _, _ = select.select([proc.stdout], [], [], min(remaining, 1.0))
        if not ready:
            continue
        chunk = proc.stdout.read1(65536)
        if not chunk:
            print(f"[{time.time() - t0:8.3f}s] server closed stdout", file=sys.stderr)
            status = 1
            break
        buf += chunk

    proc.kill()
    proc.wait()
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv))
