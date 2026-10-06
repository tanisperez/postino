#!/usr/bin/env python3
"""Local test server for the Postino sample suite.

Standard library only. It is an httpbin-like REST server listening on one plain HTTP port and
on several HTTPS ports, each one serving a different certificate (see certs/generate.sh):

    http              plain HTTP
    self-signed       valid dates, signed by itself
    expired           expired in 2020
    not-yet-valid     only valid from 2100
    wrong-host        a certificate for other.example.test
    private-ca        signed by a private CA that nobody trusts

Run it with `make sample-server`, or `python3 samples/server/server.py --help`.
Every endpoint is documented in docs/sample-suite.md.
"""

import argparse
import base64
import binascii
import gzip
import hashlib
import json
import socket
import ssl
import sys
import threading
import time
import zlib
from email.parser import BytesParser
from email.policy import HTTP
from http.cookies import SimpleCookie
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, parse_qsl, quote, urlsplit

HERE = Path(__file__).resolve().parent
CERTS = HERE / "certs"

# name -> (default port, certificate file stem or None for plain HTTP)
PROFILES = {
    "http": (8080, None),
    "self-signed": (8443, "self-signed"),
    "expired": (8444, "expired"),
    "not-yet-valid": (8445, "not-yet-valid"),
    "wrong-host": (8446, "wrong-host"),
    "private-ca": (8447, "private-ca"),
}

DEMO_USER = "demo"
DEMO_PASSWORD = "demo-password"
DEMO_TOKEN = "demo-token-0123456789"
API_KEY = "sample-api-key"
MAX_LARGE = 20 * 1024 * 1024

# A 1x1 transparent PNG.
PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
)

STATUS_PHRASES = {
    200: "OK", 201: "Created", 202: "Accepted", 204: "No Content", 301: "Moved Permanently",
    302: "Found", 303: "See Other", 304: "Not Modified", 307: "Temporary Redirect",
    308: "Permanent Redirect", 400: "Bad Request", 401: "Unauthorized", 403: "Forbidden",
    404: "Not Found", 405: "Method Not Allowed", 409: "Conflict", 418: "I'm a teapot",
    422: "Unprocessable Entity", 429: "Too Many Requests", 500: "Internal Server Error",
    502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout",
}


class Store:
    """The in-memory users collection, shared by every listener."""

    def __init__(self):
        self.lock = threading.Lock()
        self.reset()

    def reset(self):
        with self.lock:
            self.next_id = 4
            self.users = {
                1: {"id": 1, "name": "Ada Lovelace", "email": "ada@example.com"},
                2: {"id": 2, "name": "Grace Hopper", "email": "grace@example.com"},
                3: {"id": 3, "name": "Alan Turing", "email": "alan@example.com"},
            }


STORE = Store()


class Reply:
    """What a route hands back to the connection handler."""

    def __init__(self, status=200, body=b"", content_type=None, headers=None, chunks=None):
        self.status = status
        self.body = body
        self.content_type = content_type
        self.headers = headers or []
        self.chunks = chunks  # iterable of bytes, sent with chunked transfer encoding


def json_reply(data, status=200, headers=None):
    body = (json.dumps(data, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    return Reply(status, body, "application/json; charset=utf-8", headers)


def text_reply(text, status=200, content_type="text/plain; charset=utf-8", headers=None):
    return Reply(status, text.encode("utf-8"), content_type, headers)


def error_reply(status, message, headers=None):
    return json_reply({"error": message, "status": status}, status, headers)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "PostinoSampleServer/1.0"
    profile = "http"  # overridden per listener
    tls = False

    # Any method is accepted, including custom ones such as PURGE or PROPFIND.
    def __getattr__(self, name):
        if name.startswith("do_"):
            return self.dispatch
        raise AttributeError(name)

    def log_message(self, fmt, *args):
        if self.server.verbose:
            sys.stderr.write(f"[{self.profile}] {self.address_string()} {fmt % args}\n")

    # ---- request parsing -------------------------------------------------------------------

    def read_body(self):
        if "chunked" in self.headers.get("Transfer-Encoding", "").lower():
            data = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)
                if size == 0:
                    self.rfile.readline()
                    return data
                data += self.rfile.read(size)
                self.rfile.readline()
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def parse(self):
        parts = urlsplit(self.path)
        self.route = parts.path
        self.raw_query = parts.query
        self.query = parse_qs(parts.query, keep_blank_values=True)
        self.body = self.read_body()
        self.content_type = self.headers.get("Content-Type", "").split(";")[0].strip().lower()

    def query_one(self, name, default=None):
        values = self.query.get(name)
        return values[0] if values else default

    def query_int(self, name, default, low=0, high=None):
        try:
            value = int(self.query_one(name, default))
        except ValueError:
            value = default
        value = max(low, value)
        return min(high, value) if high is not None else value

    def json_body(self):
        try:
            return json.loads(self.body.decode("utf-8")), None
        except (ValueError, UnicodeDecodeError) as error:
            return None, str(error)

    def bearer(self):
        header = self.headers.get("Authorization", "")
        scheme, _, value = header.partition(" ")
        return value.strip() if scheme.lower() == "bearer" else None

    # ---- connection handling ---------------------------------------------------------------

    def dispatch(self):
        try:
            self.parse()
            reply = self.route_request()
        except ConnectionAbortedError:
            self.close_connection = True
            self.connection.close()
            return
        except Exception as error:  # a bug in the server must be visible in the response
            reply = error_reply(500, f"{type(error).__name__}: {error}")
        self.send_reply(reply)

    def send_reply(self, reply):
        head = self.command == "HEAD"
        self.send_response(reply.status, STATUS_PHRASES.get(reply.status, "Status"))
        sent = {name.lower() for name, _ in reply.headers}
        for name, value in reply.headers:
            self.send_header(name, value)
        if "access-control-allow-origin" not in sent:
            self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("X-Sample-Profile", self.profile)
        no_body = reply.status in (204, 304) or 100 <= reply.status < 200
        if reply.content_type and not no_body:
            self.send_header("Content-Type", reply.content_type)
        if reply.chunks is not None and not no_body and not head:
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            for chunk in reply.chunks:
                if chunk:
                    self.wfile.write(f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n")
                    self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
            return
        if not no_body:
            self.send_header("Content-Length", str(len(reply.body)))
        self.end_headers()
        if not head and not no_body:
            self.wfile.write(reply.body)

    # ---- routing ---------------------------------------------------------------------------

    def route_request(self):
        path = self.route
        segments = [s for s in path.split("/") if s]
        head = segments[0] if segments else ""
        rest = segments[1:]

        if self.command == "OPTIONS" and head not in ("echo", "anything"):
            return Reply(204, headers=[
                ("Allow", "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS"),
                ("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS"),
                ("Access-Control-Allow-Headers", "*"),
            ])

        routes = {
            "": self.r_index, "health": self.r_health, "reset": self.r_reset,
            "echo": self.r_echo, "anything": self.r_echo,
            "methods": self.r_methods, "status": self.r_status, "delay": self.r_delay,
            "redirect": self.r_redirect, "redirect-to": self.r_redirect_to,
            "redirect-loop": self.r_redirect_loop,
            "basic-auth": self.r_basic_auth, "bearer": self.r_bearer, "api-key": self.r_api_key,
            "auth": self.r_auth, "users": self.r_users,
            "cookies": self.r_cookies, "response-headers": self.r_response_headers,
            "repeated-headers": self.r_repeated_headers, "etag": self.r_etag,
            "json": self.r_json, "invalid-json": self.r_invalid_json, "xml": self.r_xml,
            "html": self.r_html, "text": self.r_text, "csv": self.r_csv, "utf8": self.r_utf8,
            "empty": self.r_empty, "bytes": self.r_bytes, "image": self.r_image,
            "large": self.r_large, "gzip": self.r_gzip, "deflate": self.r_deflate,
            "chunked": self.r_chunked, "drop": self.r_drop, "tls": self.r_tls,
        }
        handler = routes.get(head)
        if handler is None:
            return error_reply(404, f"no route for {path}")
        return handler(rest)

    # ---- general ---------------------------------------------------------------------------

    def r_index(self, rest):
        return json_reply({
            "name": "Postino sample server",
            "profile": self.profile,
            "tls": self.tls,
            "docs": "docs/sample-suite.md",
        })

    def r_health(self, rest):
        return json_reply({"status": "ok", "profile": self.profile})

    def r_reset(self, rest):
        if self.command != "POST":
            return error_reply(405, "use POST", [("Allow", "POST")])
        STORE.reset()
        return json_reply({"reset": True})

    def r_tls(self, rest):
        info = {"profile": self.profile, "tls": self.tls}
        if self.tls:
            info["cipher"], info["protocol"], _ = self.connection.cipher()
        return json_reply(info)

    def r_echo(self, rest):
        """Describes the request exactly as it arrived (anything and echo behave the same)."""
        headers = {}
        raw_headers = []
        for name, value in self.headers.items():
            raw_headers.append([name, value])
            key = name.lower()
            headers[key] = value if key not in headers else f"{headers[key]}, {value}"
        cookies = {k: m.value for k, m in SimpleCookie(self.headers.get("Cookie", "")).items()}
        text = self.body.decode("utf-8", errors="replace")
        data = {
            "method": self.command,
            "scheme": "https" if self.tls else "http",
            "profile": self.profile,
            "path": self.route,
            "rest": "/" + "/".join(rest),
            "query_string": self.raw_query,
            "args": {k: v[0] if len(v) == 1 else v for k, v in self.query.items()},
            "args_list": parse_qsl(self.raw_query, keep_blank_values=True),
            "headers": headers,
            "raw_headers": raw_headers,
            "cookies": cookies,
            "content_type": self.headers.get("Content-Type"),
            "body": text,
            "body_bytes": len(self.body),
            "body_sha256": hashlib.sha256(self.body).hexdigest(),
            "json": None,
            "json_error": None,
            "form": None,
            "files": None,
            "origin": self.client_address[0],
        }
        if self.content_type.endswith("json") or self.content_type.endswith("+json"):
            data["json"], data["json_error"] = self.json_body()
        elif self.content_type == "application/x-www-form-urlencoded":
            form = parse_qs(text, keep_blank_values=True)
            data["form"] = {k: v[0] if len(v) == 1 else v for k, v in form.items()}
            data["form_list"] = parse_qsl(text, keep_blank_values=True)
        elif self.content_type == "multipart/form-data":
            data["form"], data["files"] = self.parse_multipart()
        return json_reply(data)

    def parse_multipart(self):
        message = BytesParser(policy=HTTP).parsebytes(
            b"Content-Type: " + self.headers["Content-Type"].encode() + b"\r\n\r\n" + self.body
        )
        form, files = {}, {}
        for part in message.iter_parts() if message.is_multipart() else []:
            name = part.get_param("name", header="content-disposition")
            filename = part.get_filename()
            payload = part.get_payload(decode=True) or b""
            if filename:
                files[name] = {"filename": filename, "size": len(payload),
                               "content_type": part.get_content_type()}
            else:
                form[name] = payload.decode("utf-8", errors="replace")
        return form, files

    def r_methods(self, rest):
        """/methods/<verb>: answers only to that exact method, 405 with Allow otherwise."""
        allowed = (rest[0] if rest else "get").upper()
        if self.command != allowed:
            return error_reply(405, f"{self.command} not allowed, use {allowed}",
                               [("Allow", allowed)])
        return json_reply({"method": self.command, "allowed": allowed})

    def r_status(self, rest):
        try:
            code = int(rest[0])
        except (IndexError, ValueError):
            return error_reply(400, "use /status/<code>")
        if not 200 <= code <= 599:
            return error_reply(400, "code must be between 200 and 599")
        headers = []
        if code in (301, 302, 303, 307, 308):
            headers.append(("Location", "/anything/redirected"))
        if code == 401:
            headers.append(("WWW-Authenticate", 'Basic realm="postino"'))
        if code == 429:
            headers.append(("Retry-After", "30"))
        if code in (204, 304):
            return Reply(code, headers=headers)
        return json_reply({"status": code, "phrase": STATUS_PHRASES.get(code, "")}, code, headers)

    def r_delay(self, rest):
        try:
            millis = min(int(rest[0]), 10_000)
        except (IndexError, ValueError):
            return error_reply(400, "use /delay/<milliseconds>")
        time.sleep(millis / 1000)
        return json_reply({"delayed_ms": millis})

    # ---- redirects -------------------------------------------------------------------------

    def r_redirect(self, rest):
        """/redirect/<n>: n hops of 302 and then /anything/redirected."""
        try:
            hops = int(rest[0])
        except (IndexError, ValueError):
            return error_reply(400, "use /redirect/<hops>")
        target = f"/redirect/{hops - 1}" if hops > 1 else "/anything/redirected"
        return Reply(302, headers=[("Location", target)])

    def r_redirect_to(self, rest):
        url = self.query_one("url", "/anything/redirected")
        status = self.query_int("status", 302, 300, 399)
        return Reply(status, headers=[("Location", url)])

    def r_redirect_loop(self, rest):
        return Reply(302, headers=[("Location", "/redirect-loop")])

    # ---- auth ------------------------------------------------------------------------------

    def r_basic_auth(self, rest):
        """/basic-auth/<user>/<password>"""
        if len(rest) != 2:
            return error_reply(400, "use /basic-auth/<user>/<password>")
        header = self.headers.get("Authorization", "")
        scheme, _, value = header.partition(" ")
        challenge = [("WWW-Authenticate", 'Basic realm="postino"')]
        if scheme.lower() != "basic":
            return error_reply(401, "basic credentials required", challenge)
        try:
            user, _, password = base64.b64decode(value.strip(), validate=True).decode().partition(":")
        except (binascii.Error, UnicodeDecodeError):
            return error_reply(401, "malformed basic credentials", challenge)
        if (user, password) != (rest[0], rest[1]):
            return error_reply(401, "wrong credentials", challenge)
        return json_reply({"authenticated": True, "user": user})

    def r_bearer(self, rest):
        token = self.bearer()
        if not token:
            return error_reply(401, "bearer token required", [("WWW-Authenticate", "Bearer")])
        return json_reply({"authenticated": True, "token": token})

    def r_api_key(self, rest):
        key = self.headers.get("X-API-Key") or self.query_one("api_key")
        if key != API_KEY:
            return error_reply(401, "missing or wrong API key")
        where = "header" if self.headers.get("X-API-Key") else "query"
        return json_reply({"authenticated": True, "via": where})

    def r_auth(self, rest):
        action = rest[0] if rest else ""
        if action == "login":
            if self.command != "POST":
                return error_reply(405, "use POST", [("Allow", "POST")])
            data, error = self.json_body()
            if error or not isinstance(data, dict):
                return error_reply(400, "a JSON object is required")
            if (data.get("username"), data.get("password")) != (DEMO_USER, DEMO_PASSWORD):
                return error_reply(401, "invalid username or password")
            return json_reply({"token": DEMO_TOKEN, "user": DEMO_USER, "expires_in": 3600})
        if action == "me":
            if self.bearer() != DEMO_TOKEN:
                return error_reply(401, "a valid bearer token is required",
                                   [("WWW-Authenticate", "Bearer")])
            return json_reply({"user": DEMO_USER, "email": "demo@example.com"})
        return error_reply(404, "use /auth/login or /auth/me")

    # ---- users (CRUD) ----------------------------------------------------------------------

    def r_users(self, rest):
        if self.require_token and self.bearer() != DEMO_TOKEN:
            return error_reply(401, "a valid bearer token is required",
                               [("WWW-Authenticate", "Bearer")])
        store = STORE
        with store.lock:
            if not rest:
                if self.command == "GET":
                    return self.list_users()
                if self.command == "POST":
                    data, error = self.json_body()
                    if error or not isinstance(data, dict) or not data.get("name"):
                        return error_reply(422, "a JSON body with a name is required")
                    user = {"id": store.next_id, "name": data["name"],
                            "email": data.get("email", "")}
                    store.users[user["id"]] = user
                    store.next_id += 1
                    return json_reply(user, 201, [("Location", f"/users/{user['id']}")])
                return error_reply(405, "not allowed", [("Allow", "GET, POST")])
            try:
                user_id = int(rest[0])
            except ValueError:
                return error_reply(400, "the id must be a number")
            user = store.users.get(user_id)
            if user is None:
                return error_reply(404, f"user {user_id} not found")
            if self.command == "GET":
                return json_reply(user)
            if self.command in ("PUT", "PATCH"):
                data, error = self.json_body()
                if error or not isinstance(data, dict):
                    return error_reply(422, "a JSON object is required")
                if self.command == "PUT":
                    if not data.get("name"):
                        return error_reply(422, "PUT replaces the user, a name is required")
                    user = {"id": user_id, "name": data["name"], "email": data.get("email", "")}
                else:
                    user = {**user, **{k: v for k, v in data.items() if k != "id"}}
                store.users[user_id] = user
                return json_reply(user)
            if self.command == "DELETE":
                del store.users[user_id]
                return Reply(204)
            return error_reply(405, "not allowed", [("Allow", "GET, PUT, PATCH, DELETE")])

    require_token = False  # set by main() from --require-token

    def list_users(self):
        users = list(STORE.users.values())
        needle = self.query_one("q", "").lower()
        if needle:
            users = [u for u in users if needle in u["name"].lower() or needle in u["email"].lower()]
        per_page = self.query_int("per_page", self.query_int("limit", 20, 1, 100), 1, 100)
        page = self.query_int("page", 1, 1)
        total = len(users)
        start = (page - 1) * per_page
        return json_reply({"page": page, "per_page": per_page, "total": total,
                           "data": users[start:start + per_page]})

    # ---- cookies and headers ---------------------------------------------------------------

    def r_cookies(self, rest):
        action = rest[0] if rest else ""
        if action == "set":
            headers = [("Set-Cookie", f"{quote(k)}={quote(v[0])}; Path=/")
                       for k, v in self.query.items()]
            return json_reply({"set": {k: v[0] for k, v in self.query.items()}}, 200, headers)
        if action == "delete":
            headers = [("Set-Cookie", f"{quote(k)}=; Max-Age=0; Path=/") for k in self.query]
            return json_reply({"deleted": list(self.query)}, 200, headers)
        cookies = {k: m.value for k, m in SimpleCookie(self.headers.get("Cookie", "")).items()}
        return json_reply({"cookies": cookies})

    def r_response_headers(self, rest):
        """/response-headers?Name=value: the response carries those headers."""
        headers = [(k, v) for k, values in self.query.items() for v in values]
        return json_reply({"headers": dict(headers)}, 200, headers)

    def r_repeated_headers(self, rest):
        headers = [("X-Repeated", "one"), ("X-Repeated", "two"), ("X-Repeated", "three"),
                   ("Set-Cookie", "a=1; Path=/"), ("Set-Cookie", "b=2; Path=/"),
                   ("X-Mixed-Case", "Value With Spaces")]
        return json_reply({"repeated": ["one", "two", "three"]}, 200, headers)

    def r_etag(self, rest):
        """/etag/<value>: 304 when If-None-Match matches, the document otherwise."""
        etag = f'"{rest[0] if rest else "v1"}"'
        if self.headers.get("If-None-Match") == etag:
            return Reply(304, headers=[("ETag", etag)])
        return json_reply({"etag": etag}, 200, [("ETag", etag), ("Cache-Control", "max-age=60")])

    # ---- content types ---------------------------------------------------------------------

    def r_json(self, rest):
        return json_reply({
            "string": "hello", "number": 42, "float": 3.14159, "bool": True, "null": None,
            "array": [1, 2, 3], "object": {"nested": {"deep": ["a", "b", {"c": "d"}]}},
            "unicode": "ñandú, 日本語, emoji 🚀",
        })

    def r_invalid_json(self, rest):
        return Reply(200, b'{"broken": [1, 2,', "application/json")

    def r_xml(self, rest):
        xml = ('<?xml version="1.0" encoding="UTF-8"?>\n<library>\n'
               '  <book id="1"><title>Rust in Action</title><year>2021</year></book>\n'
               '  <book id="2"><title>Programming Rust</title><year>2021</year></book>\n'
               '</library>\n')
        return text_reply(xml, content_type="application/xml; charset=utf-8")

    def r_html(self, rest):
        return text_reply("<!doctype html><html><head><title>Sample</title></head>"
                          "<body><h1>Hello from the sample server</h1></body></html>\n",
                          content_type="text/html; charset=utf-8")

    def r_text(self, rest):
        return text_reply("Plain text.\nSecond line.\n")

    def r_csv(self, rest):
        return text_reply("id,name\n1,Ada\n2,Grace\n3,Alan\n", content_type="text/csv")

    def r_utf8(self, rest):
        return text_reply("Árbol, ñandú, 日本語, Ελληνικά, العربية, emoji 🚀\n")

    def r_empty(self, rest):
        return Reply(200, b"", "text/plain")

    def r_bytes(self, rest):
        """/bytes/<n>: n deterministic bytes, as application/octet-stream."""
        try:
            count = min(int(rest[0]), MAX_LARGE)
        except (IndexError, ValueError):
            return error_reply(400, "use /bytes/<count>")
        return Reply(200, bytes(i % 256 for i in range(count)), "application/octet-stream")

    def r_image(self, rest):
        if rest and rest[0] == "png":
            return Reply(200, PNG, "image/png")
        return error_reply(404, "use /image/png")

    def r_large(self, rest):
        """/large?size=<bytes>: a JSON document of about that size (capped at 20 MiB)."""
        target = self.query_int("size", 1_000_000, 0, MAX_LARGE)
        item = '{"index":%d,"text":"lorem ipsum dolor sit amet, consectetur adipiscing elit"}'
        parts, size, index = [], 2, 0
        while size < target:
            text = item % index
            parts.append(text)
            size += len(text) + 1
            index += 1
        body = ('{"items":[' + ",".join(parts) + "]}").encode()
        return Reply(200, body, "application/json")

    def r_gzip(self, rest):
        body = json.dumps({"gzipped": True, "text": "compressed " * 50}).encode()
        return Reply(200, gzip.compress(body), "application/json",
                     [("Content-Encoding", "gzip")])

    def r_deflate(self, rest):
        body = json.dumps({"deflated": True, "text": "compressed " * 50}).encode()
        return Reply(200, zlib.compress(body), "application/json",
                     [("Content-Encoding", "deflate")])

    def r_chunked(self, rest):
        """/chunked?chunks=<n>&delay=<ms>: n chunks with a pause between them."""
        chunks = self.query_int("chunks", 5, 1, 100)
        delay = self.query_int("delay", 0, 0, 2000) / 1000

        def generate():
            for index in range(chunks):
                if delay and index:
                    time.sleep(delay)
                yield f"chunk {index + 1} of {chunks}\n".encode()

        return Reply(200, content_type="text/plain; charset=utf-8", chunks=generate())

    def r_drop(self, rest):
        """Closes the connection without answering, to test network errors."""
        raise ConnectionAbortedError


class SampleServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    request_queue_size = 128

    def __init__(self, address, handler, verbose):
        self.verbose = verbose
        super().__init__(address, handler)

    def handle_error(self, request, client_address):
        # TLS handshakes rejected by the client are expected here (that is the point of the
        # certificate profiles), so they are not worth a traceback.
        error = sys.exc_info()[1]
        if isinstance(error, (ssl.SSLError, ConnectionError, TimeoutError)):
            return
        super().handle_error(request, client_address)


class TlsServer(SampleServer):
    """Wraps every accepted connection in TLS, doing the handshake in the worker thread."""

    def __init__(self, address, handler, verbose, context):
        self.context = context
        super().__init__(address, handler, verbose)

    def get_request(self):
        sock, address = super().get_request()
        sock.settimeout(10)
        return self.context.wrap_socket(sock, server_side=True, do_handshake_on_connect=False), address

    def finish_request(self, request, client_address):
        request.do_handshake()
        request.settimeout(None)
        super().finish_request(request, client_address)


def make_context(stem):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(CERTS / f"{stem}.pem", CERTS / f"{stem}.key")
    return context


def start_listener(name, host, port, stem, verbose, require_token):
    handler = type(f"{name}Handler", (Handler,), {
        "profile": name, "tls": stem is not None, "require_token": require_token,
    })
    if stem is None:
        server = SampleServer((host, port), handler, verbose)
    else:
        server = TlsServer((host, port), handler, verbose, make_context(stem))
    threading.Thread(target=server.serve_forever, name=name, daemon=True).start()
    return server


def main():
    parser = argparse.ArgumentParser(description="Postino sample suite server.")
    parser.add_argument("--host", default="127.0.0.1", help="address to bind (default 127.0.0.1)")
    parser.add_argument("--port-offset", type=int, default=0,
                        help="add this to every default port (HTTP 8080, TLS 8443 to 8447)")
    parser.add_argument("--ephemeral", action="store_true",
                        help="let the OS pick every port; they are printed in the READY line")
    parser.add_argument("--no-tls", action="store_true", help="only start the plain HTTP listener")
    parser.add_argument("--require-token", action="store_true",
                        help="make /users require the bearer token from /auth/login")
    parser.add_argument("--verbose", action="store_true", help="log every request to stderr")
    args = parser.parse_args()

    servers = {}
    try:
        for name, (default_port, stem) in PROFILES.items():
            if stem is not None and args.no_tls:
                continue
            port = 0 if args.ephemeral else default_port + args.port_offset
            servers[name] = start_listener(name, args.host, port, stem, args.verbose,
                                           args.require_token)
    except OSError as error:
        print(f"cannot bind {args.host}: {error}", file=sys.stderr)
        sys.exit(1)

    ports = {name: server.server_address[1] for name, server in servers.items()}
    print("Postino sample server")
    for name, port in ports.items():
        scheme = "http" if name == "http" else "https"
        print(f"  {name:<14} {scheme}://localhost:{port}")
    # One machine readable line for scripts and the integration test.
    print("READY " + json.dumps(ports), flush=True)
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        print("\nbye")


if __name__ == "__main__":
    main()
