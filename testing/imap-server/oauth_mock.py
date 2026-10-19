"""A fake OAuth2 provider for integration tests. Not for production use.

POST /token       refresh_token=alice-refresh  -> a new access token
POST /introspect  token=<access token>         -> whether Dovecot should accept it
"""
import json
import secrets
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs

REFRESH_TOKENS = {"alice-refresh": "alice"}
issued = {}


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        form = {k: v[0] for k, v in parse_qs(self.rfile.read(length).decode()).items()}
        if self.path == "/token":
            user = REFRESH_TOKENS.get(form.get("refresh_token", ""))
            if form.get("grant_type") != "refresh_token" or user is None:
                return self.reply(400, {"error": "invalid_grant", "error_description": "Token has been expired or revoked."})
            token = secrets.token_urlsafe(24)
            issued[token] = user
            return self.reply(200, {"access_token": token, "token_type": "Bearer", "expires_in": 3600})
        if self.path == "/introspect":
            user = issued.get(form.get("token", ""))
            return self.reply(200, {"active": True, "username": user} if user else {"active": False})
        self.reply(404, {"error": "not_found"})

    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
