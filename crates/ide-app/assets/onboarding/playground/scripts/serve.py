#!/usr/bin/env python3
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import errno
import os
import time

PROJECT_ROOT = Path(__file__).resolve().parents[1]
HOST = "127.0.0.1"
PORT = 4173


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        print(f"  {self.address_string()} · {format % args}", flush=True)


os.chdir(PROJECT_ROOT)
try:
    server = ThreadingHTTPServer((HOST, PORT), QuietHandler)
except OSError as error:
    if error.errno != errno.EADDRINUSE:
        raise
    print("", flush=True)
    print("  CHORO PLAYGROUND", flush=True)
    print(f"  ✓ Preview already live at http://{HOST}:{PORT}", flush=True)
    print("  Reusing the running preview for this onboarding session.", flush=True)
    print("", flush=True)
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass
    raise SystemExit(0)

server.daemon_threads = True
print("", flush=True)
print("  CHORO PLAYGROUND", flush=True)
print(f"  ✓ Preview ready at http://{HOST}:{PORT}", flush=True)
print("  Watching the project while you explore Choro.", flush=True)
print("", flush=True)

try:
    server.serve_forever()
except KeyboardInterrupt:
    pass
finally:
    server.server_close()
