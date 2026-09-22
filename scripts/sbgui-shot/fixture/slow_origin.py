#!/usr/bin/env python3
"""A deliberately slow origin, so proxied requests are still open — and still
counting bytes — when the screenshot is taken.

clash_api's /connections lists live connections only, so the connections table
could never be photographed: a request that finished in milliseconds was gone
before the frame, and a tunnel to one of the demo's closed node ports is not a
connection at all. This streams a body a chunk at a time, so each request stays
open for `seconds` and its download total climbs while it is on screen.
"""
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CHUNK = 32 * 1024


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        total = CHUNK * 64
        self.send_response(200)
        self.send_header("Content-Length", str(total))
        self.end_headers()
        step = max(self.server.seconds * 1000 // 64, 1)
        for _ in range(64):
            try:
                self.wfile.write(b"s" * CHUNK)
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                return
            time.sleep(step / 1000)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8099
    seconds = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    httpd = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    httpd.seconds = seconds
    httpd.serve_forever()
