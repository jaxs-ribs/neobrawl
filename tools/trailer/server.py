# The trailer recorder's server: serves the game (web/) and takes frames and
# audio posted by the page into recordings/trailer/<take>/.
import http.server, os, sys
WEB = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "web")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "recordings", "trailer")
HERE = os.path.dirname(os.path.abspath(__file__))
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=WEB, **k)
    def log_message(self, *a): pass
    def translate_path(self, path):
        # /trailer/ -> this folder (the compositor), /rec/ -> the takes
        for pre, root in (("/trailer/", HERE), ("/rec/", OUT)):
            if path.startswith(pre):
                return os.path.join(root, path[len(pre):].split("?")[0])
        return super().translate_path(path)
    def do_POST(self):
        q = dict(kv.split("=") for kv in self.path.split("?", 1)[1].split("&"))
        d = os.path.join(OUT, q["take"])
        os.makedirs(os.path.join(d, "frames"), exist_ok=True)
        n = int(self.headers["Content-Length"])
        name = "audio.wav" if self.path.startswith("/audio") else os.path.join("frames", "%05d.jpg" % int(q["i"]))
        with open(os.path.join(d, name), "wb") as f: f.write(self.rfile.read(n))
        self.send_response(204); self.end_headers()
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8652), H).serve_forever()
