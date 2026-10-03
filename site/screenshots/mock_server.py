#!/usr/bin/env python3
"""Mock "Bookshelf API" for the Postino screenshots. Standard library only, no logging."""

import json
import random
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8099

BOOKS = [
    (1, "Clean Code", "Robert C. Martin", 2008, "978-0132350884", 37.9, ["software", "craft"], True),
    (2, "Dune", "Frank Herbert", 1965, "978-0441172719", 14.5, ["fiction", "sci-fi"], True),
    (3, "Designing Data-Intensive Applications", "Martin Kleppmann", 2017, "978-1449373320", 49.0, ["software", "databases"], True),
    (4, "Neuromancer", "William Gibson", 1984, "978-0441569595", 12.99, ["fiction", "cyberpunk"], False),
    (5, "The Rust Programming Language", "Steve Klabnik", 2019, "978-1718500440", 39.95, ["software", "rust"], True),
    (6, "Pride and Prejudice", "Jane Austen", 1813, "978-0141439518", 8.75, ["fiction", "classic"], True),
    (7, "Structure and Interpretation of Computer Programs", "Harold Abelson", 1996, "978-0262510875", 54.0, ["software", "classic"], False),
    (8, "Kindred", "Octavia E. Butler", 1979, "978-0807083697", 15.2, ["fiction", "sci-fi"], True),
    (9, "Refactoring", "Martin Fowler", 2018, "978-0134757599", 44.8, ["software", "craft"], True),
    (10, "The Left Hand of Darkness", "Ursula K. Le Guin", 1969, "978-0441478125", 13.4, ["fiction", "sci-fi"], True),
]


def book(row):
    id_, title, author, year, isbn, price, tags, stock = row
    return {
        "id": id_,
        "title": title,
        "author": author,
        "year": year,
        "isbn": isbn,
        "price": price,
        "currency": "EUR",
        "tags": tags,
        "inStock": stock,
        "rating": round(3.8 + (id_ * 37 % 12) / 10, 1),
        "createdAt": "2026-03-%02dT09:30:00Z" % (id_ + 5),
    }


STORE = {row[0]: book(row) for row in BOOKS}
NEXT_ID = [11]
HEADERS = {
    "X-RateLimit-Limit": "1000",
    "X-RateLimit-Remaining": "987",
    "Cache-Control": "no-cache",
}


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "bookshelf/2.4.1"
    sys_version = ""

    def log_message(self, *args):
        pass

    def send_json(self, status, payload=None, extra=None):
        # A realistic server time, with a small slow tail, so timings and the load test
        # latency charts look like a real API instead of a loopback one.
        time.sleep(random.uniform(0.035, 0.085) + (0.12 if random.random() < 0.04 else 0))
        body = b"" if payload is None else json.dumps(payload, indent=2).encode()
        self.send_response(status)
        for key, value in {**HEADERS, **(extra or {})}.items():
            self.send_header(key, value)
        if payload is not None:
            self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def read_json(self):
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            return json.loads(raw) if raw else {}
        except ValueError:
            return {}

    def route(self):
        url = urlparse(self.path)
        path = url.path.rstrip("/") or "/"
        query = {k: v[-1] for k, v in parse_qs(url.query).items()}
        method = self.command
        body = self.read_json() if method in ("POST", "PUT", "PATCH") else {}

        if path == "/health":
            return self.send_json(200, {"status": "ok", "version": "2.4.1", "uptimeSeconds": 86412})
        if path == "/auth/login" and method == "POST":
            return self.send_json(200, {
                "accessToken": "eyJhbGciOiJIUzI1NiJ9.bGl2ZS10b2tlbg.4f9c1a",
                "tokenType": "Bearer",
                "expiresIn": 3600,
                "refreshToken": "rt_7f3a91c2d4",
            })
        if path == "/auth/refresh" and method == "POST":
            return self.send_json(200, {
                "accessToken": "eyJhbGciOiJIUzI1NiJ9.cmVmcmVzaGVk.9b21de",
                "tokenType": "Bearer",
                "expiresIn": 3600,
            })
        if path == "/users/me":
            return self.send_json(200, {
                "id": 42, "name": "Ada Reader", "email": "reader@bookshelf.dev",
                "plan": "pro", "memberSince": "2025-11-02",
            })
        if path == "/books" and method == "GET":
            rows = sorted(STORE.values(), key=lambda b: b.get(query.get("sort", "title"), b["title"]))
            if query.get("genre"):
                rows = [b for b in rows if query["genre"] in b["tags"]]
            if query.get("inStock") == "true":
                rows = [b for b in rows if b["inStock"]]
            limit = int(query.get("limit", 8))
            page = int(query.get("page", 1))
            chunk = rows[(page - 1) * limit: page * limit]
            return self.send_json(200, {
                "data": chunk,
                "meta": {"page": page, "limit": limit, "total": len(rows),
                         "pages": -(-len(rows) // limit)},
            })
        if path == "/books" and method == "POST":
            id_ = NEXT_ID[0]
            NEXT_ID[0] += 1
            created = {
                "id": id_, "title": body.get("title", "Untitled"),
                "author": body.get("author", "Unknown"), "year": body.get("year", 2026),
                "isbn": body.get("isbn", ""), "price": body.get("price", 0),
                "currency": "EUR", "tags": body.get("tags", []), "inStock": True,
                "createdAt": "2026-10-03T10:00:00Z",
            }
            STORE[id_] = created
            return self.send_json(201, created, {"Location": "/books/%d" % id_})
        match = re.fullmatch(r"/books/(\d+)", path)
        if match:
            id_ = int(match.group(1))
            if id_ not in STORE:
                return self.send_json(404, {"error": "not_found", "message": "Book not found"})
            if method == "GET":
                return self.send_json(200, STORE[id_])
            if method in ("PATCH", "PUT"):
                STORE[id_].update({k: v for k, v in body.items() if k != "id"})
                return self.send_json(200, STORE[id_])
            if method == "DELETE":
                del STORE[id_]
                return self.send_json(204)
        if path == "/orders" and method == "GET":
            return self.send_json(200, {"data": [
                {"id": 1001, "status": "shipped", "total": 52.4, "items": 2},
                {"id": 1002, "status": "shipped", "total": 14.5, "items": 1},
            ], "meta": {"total": 2}})
        if path == "/orders" and method == "POST":
            return self.send_json(201, {"id": 1003, "status": "pending",
                                        "items": body.get("items", []), "total": 66.8},
                                  {"Location": "/orders/1003"})
        self.send_json(404, {"error": "not_found", "message": "No such route"})

    do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = route


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
