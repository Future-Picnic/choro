#!/usr/bin/env python3
"""Local-only video library: allowlisted media, seeking, and explicit Finder actions.

Run: python3 docs/serve-video-library.py
No dependencies, uploads, copies, or directory browsing. Ctrl+C stops the server.
"""
import argparse
import json
import re
import secrets
import subprocess
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


PAGE = Path(__file__).with_name('how-to-video-library.html')


def make_server(library_root, port=8769):
    page = PAGE.read_text()
    manifest = json.loads(re.search(
        r'<script type="application/json" id="completed-video-files">(.*?)</script>',
        page, re.S).group(1))
    drafts = re.search(r'<script type="application/json" id="draft-video-files">(.*?)</script>', page, re.S)
    if drafts:
        manifest += json.loads(drafts.group(1))
    root = Path(library_root).resolve()
    assets = {}
    for video in manifest:
        folder = (root / video['folder']).resolve()
        movie = (folder / video['file']).resolve()
        poster = (folder / video.get('poster', 'approved-endcards/check-v2-intro.png')).resolve()
        if not all(path.is_relative_to(root) for path in (folder, movie, poster)):
            raise ValueError('Video manifest points outside the library')
        assets[video['id']] = (folder, movie, poster)
    token = secrets.token_urlsafe(32)
    bridge = '<script>window.CHORO_VIDEO_LIBRARY=' + json.dumps({'token': token}) + ';</script>'
    body = page.replace('<!-- local-video-library-bridge -->', bridge).encode()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def trusted(self):
            expected = f'127.0.0.1:{self.server.server_port}'
            if self.headers.get('Host') != expected or self.headers.get('Sec-Fetch-Site') == 'cross-site':
                self.send_error(403)
                return False
            return True

        def end_headers(self):
            self.send_header('X-Content-Type-Options', 'nosniff')
            self.send_header('Referrer-Policy', 'no-referrer')
            self.send_header('Cross-Origin-Resource-Policy', 'same-origin')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Security-Policy', "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; media-src 'self'; img-src 'self'; frame-ancestors 'none'; base-uri 'none'")
            super().end_headers()

        def do_HEAD(self):
            self.serve(head=True)

        def do_GET(self):
            self.serve()

        def serve(self, head=False):
            if not self.trusted():
                return
            path = urlsplit(self.path).path
            if path in ('/', '/how-to-video-library.html'):
                self.send_response(200)
                self.send_header('Content-Type', 'text/html; charset=utf-8')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                if not head:
                    self.wfile.write(body)
                return
            match = re.fullmatch(r'/(media|poster)/([a-z-]+)', path)
            if not match or match[2] not in assets:
                self.send_error(404)
                return
            file = assets[match[2]][1 if match[1] == 'media' else 2]
            try:
                stream = file.open('rb')
            except OSError:
                self.send_error(404, 'Video or poster not found in the local library')
                return
            with stream:
                stream.seek(0, 2)
                size = stream.tell()
                start, end = 0, size - 1
                byte_range = self.headers.get('Range')
                if byte_range:
                    parts = re.fullmatch(r'bytes=(\d*)-(\d*)', byte_range)
                    valid = bool(parts and (parts[1] or parts[2]))
                    if valid:
                        if parts[1]:
                            start = int(parts[1])
                            end = min(int(parts[2]), end) if parts[2] else end
                        else:
                            valid = int(parts[2]) > 0
                            start = max(0, size - int(parts[2]))
                        valid = valid and 0 <= start <= end < size
                    if not valid:
                        self.send_response(416)
                        self.send_header('Content-Range', f'bytes */{size}')
                        self.send_header('Content-Length', '0')
                        self.end_headers()
                        return
                self.send_response(206 if byte_range else 200)
                self.send_header('Content-Type', 'video/mp4' if match[1] == 'media' else 'image/png')
                self.send_header('Accept-Ranges', 'bytes')
                self.send_header('Content-Length', str(max(0, end - start + 1)))
                if byte_range:
                    self.send_header('Content-Range', f'bytes {start}-{end}/{size}')
                self.end_headers()
                if head:
                    return
                stream.seek(start)
                remaining = end - start + 1
                try:
                    while remaining > 0:
                        chunk = stream.read(min(256 * 1024, remaining))
                        if not chunk:
                            break
                        self.wfile.write(chunk)
                        remaining -= len(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    pass  # Normal when the player seeks or the modal closes.

        def do_POST(self):
            if not self.trusted():
                return
            origin = f'http://127.0.0.1:{self.server.server_port}'
            if (self.headers.get('Origin') != origin or not secrets.compare_digest(
                    self.headers.get('X-Choro-Library-Token', ''), token)):
                self.send_error(403)
                return
            match = re.fullmatch(r'/open-folder/([a-z-]+)', urlsplit(self.path).path)
            if not match or match[1] not in assets:
                self.send_error(404)
                return
            folder = assets[match[1]][0]
            if not folder.is_dir():
                self.send_error(404, 'Video folder not found')
                return
            try:
                subprocess.run(['open', str(folder)], check=True, timeout=10,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL)
            except (OSError, subprocess.SubprocessError):
                self.send_error(503, 'Finder could not open this folder')
                return
            self.send_response(204)
            self.send_header('Content-Length', '0')
            self.end_headers()

    return ThreadingHTTPServer(('127.0.0.1', port), Handler)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=8769, help='Local port; 0 chooses a free port')
    parser.add_argument('--library-root', type=Path, default=Path.home() / 'Movies/Choro Tutorials')
    args = parser.parse_args()
    server = make_server(args.library_root, args.port)
    print(f'Open http://127.0.0.1:{server.server_port}/how-to-video-library.html#start', flush=True)
    print('Local access only. Keep this terminal running; Ctrl+C stops it.', flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
