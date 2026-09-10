#!/usr/bin/env python3
"""Sample a running NETCONF language server while the catalog scan runs.

Usage:
    python3 scripts/lspsample.py <server-binary> <root-dir|root-uri> \
        [timeout-seconds] [interval-seconds]

Sends `initialize` with the same framing as scripts/lsp_scan_driver.py, then
every `interval` seconds (default 0.5 s) reads /proc/<pid>/status and
/proc/<pid>/stat and prints threads, user/sys CPU, VmRSS and voluntary /
involuntary context switches until the `initialize` response arrives or the
process exits. It also relays the server's `window/logMessage` lines.

This is the harness behind the 69%-sys CPU evidence in
docs/perf/catalog-scan-regression-2026-09-11.md -- section 3.1 / Appendix B.

No third-party dependencies; Python 3 only (Linux /proc interface).
"""

import json
import os
import select
import subprocess
import sys
import time
from pathlib import Path

try:
    CLK_TCK = os.sysconf("SC_CLK_TCK")
except (ValueError, OSError):
    CLK_TCK = 100


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


def proc_stat(pid):
    """Return (threads, rss_kb, utime, stime, vol_ctx, nonvol_ctx) for `pid`."""
    with open(f"/proc/{pid}/stat") as f:
        raw = f.read()
    # `comm` (field 2) can contain spaces/parentheses, so split after the last
    # ')': rest[0] is field 3, hence fields 14/15 (utime/stime) are rest[11]/[12].
    rest = raw[raw.rfind(")") + 2:].split()
    utime, stime = int(rest[11]), int(rest[12])

    with open(f"/proc/{pid}/status") as f:
        status = f.read()

    def field(key):
        return next(
            (line.split()[1] for line in status.splitlines() if line.startswith(key)),
            "0",
        )

    return (field("Threads:"), field("VmRSS:"), utime, stime,
            field("voluntary_ctxt_switches:"), field("nonvoluntary_ctxt_switches:"))


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    binpath, target = argv[1], argv[2]
    timeout = float(argv[3]) if len(argv) > 3 else 900.0
    interval = float(argv[4]) if len(argv) > 4 else 0.5
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
    last_sample = 0.0
    done = False
    status = 0

    while True:
        now = time.time()
        remaining = deadline - now
        if remaining <= 0:
            print(f"[{now - t0:8.3f}s] timeout after {timeout:.1f}s", file=sys.stderr)
            status = 1
            break

        if now - last_sample >= interval:
            last_sample = now
            try:
                threads, rss, utime, stime, vol, nonvol = proc_stat(proc.pid)
                print(
                    f"t={now - t0:7.1f}s threads={int(threads):>3} "
                    f"user={utime / CLK_TCK:7.1f}s sys={stime / CLK_TCK:7.1f}s "
                    f"rss={int(rss) / 1024:6.1f} MB "
                    f"vol_ctx={vol} nonvol_ctx={nonvol}",
                    flush=True,
                )
            except (FileNotFoundError, ProcessLookupError, IndexError, ValueError):
                pass  # server already exited; keep draining its output below

        ready, _, _ = select.select([proc.stdout], [], [], min(interval, remaining))
        if not ready:
            continue
        chunk = proc.stdout.read1(65536)
        if not chunk:
            print(f"[{time.time() - t0:8.3f}s] server closed stdout", file=sys.stderr)
            break
        buf += chunk

        while True:
            msg, buf = extract_msg(buf)
            if msg is None:
                break
            el = time.time() - t0
            if msg.get("method") == "window/logMessage":
                print(f"[{el:8.3f}s] LOG: {msg.get('params', {}).get('message')}",
                      flush=True)
            elif "id" in msg and "method" not in msg:
                print(f"[{el:8.3f}s] initialize done", flush=True)
                done = True
        if done:
            break

    proc.kill()
    proc.wait()
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv))
