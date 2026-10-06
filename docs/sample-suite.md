# The sample suite

`samples/` holds a complete, deterministic set of `.postino` requests and the local server they
run against. It exists to try every feature of Postino by hand and to run the same checks
automatically. It never touches the internet: the server only listens on `127.0.0.1`.

```
samples/
  server/
    server.py            httpbin-like REST server, Python 3 standard library only
    certs/               the test certificates and generate.sh, the script that makes them
  workspace/             the requests, a normal Postino workspace
    environments/        local (HTTP) and secure (HTTPS)
```

`examples/sample-workspace` is a different, tiny workspace used for `POSTINO_AUTOSEND` and quick
manual checks. It also works against this server (`/health`, `/auth/login`, `/users`).

## Running it

```
make sample-server     # the server only, on the default ports
make sample            # the server plus the app on samples/workspace
```

Then pick the `local` environment, open `health` and send it. `make sample` stops the server when
the app closes. Requests are written to be run in sidebar order with one session: `auth/login`
stores a token, `crud/` stores a user id, `variables/session-set` stores a value that
`variables/session-use` reads. Each such request says so in its docs.

Every request has at least one `test()`, so a run is green or red at a glance. The exceptions are
meant to be red or failing, they live in `expected-failures/` and in two requests of `scripting/`
and `variables/` whose docs say what to expect.

## The server

One process, several listeners. Each one serves the same routes and reports its own profile in
`X-Sample-Profile` and in `/tls`.

| Profile | Default port | Certificate (`certs/`) | What Postino reports |
|---|---|---|---|
| `http` | 8080 | none | no warning |
| `self-signed` | 8443 | `self-signed.pem`, valid dates, signs itself | unknown issuer |
| `expired` | 8444 | `expired.pem`, valid only on 2020-01-01 | certificate expired |
| `not-yet-valid` | 8445 | `not-yet-valid.pem`, valid from 2100 | certificate not valid yet |
| `wrong-host` | 8446 | `wrong-host.pem`, for `other.example.test` | unknown issuer, see below |
| `private-ca` | 8447 | `private-ca.pem`, signed by `ca.pem` | unknown issuer |

Options: `--port-offset N` adds `N` to every default port, `--ephemeral` lets the OS choose and
prints the result, `--no-tls` starts only HTTP, `--require-token` makes `/users` demand the
bearer token from `/auth/login`, `--verbose` logs every request. After startup it prints a
`READY {...}` line with the ports as JSON, which the integration test reads.

### Endpoints

| Route | Behavior |
|---|---|
| `/echo`, `/anything[/...]` | Describes the request: method, path, query (`args`, `args_list`), headers (lowercase names), cookies, body, parsed `json`, `form` and multipart `files`, body size and SHA-256. |
| `/health`, `/`, `/tls` | Liveness, a description, and the profile with the negotiated TLS version. |
| `/methods/<verb>` | Answers only to that method, `405` with `Allow` otherwise. |
| `/status/<200-599>` | That status, with `Location`, `WWW-Authenticate` or `Retry-After` where it fits. |
| `/delay/<ms>` | Waits, up to 10 s. |
| `/redirect/<n>`, `/redirect-to?url=&status=`, `/redirect-loop` | Redirect chains, one hop of any 3xx, and an endless loop. |
| `/basic-auth/<user>/<password>`, `/bearer`, `/api-key` | Basic, bearer token, and API key in `X-API-Key` or `?api_key=` (`sample-api-key`). |
| `/auth/login`, `/auth/me` | `demo` / `demo-password` returns `demo-token-0123456789`, which `/auth/me` accepts. |
| `/users[/<id>]` | In-memory CRUD with `page`, `per_page` (or `limit`) and `q`. `POST /reset` restores the three seed users. |
| `/cookies`, `/cookies/set?k=v`, `/cookies/delete?k` | Shows the received cookies, sets or expires cookies. |
| `/response-headers?Name=value`, `/repeated-headers`, `/etag/<v>` | Custom response headers, repeated headers, conditional `304`. |
| `/json`, `/invalid-json`, `/xml`, `/html`, `/text`, `/csv`, `/utf8`, `/empty` | Fixed bodies in several formats. |
| `/bytes/<n>`, `/image/png`, `/large?size=<n>` | Binary, an image, and a JSON document of about `n` bytes, up to 20 MiB. |
| `/gzip`, `/deflate`, `/chunked?chunks=&delay=` | Content encodings and chunked transfer, optionally slow. |
| `/drop` | Closes the connection without answering. |

## The workspace

| Folder | What it covers |
|---|---|
| `methods/` | The seven standard methods, custom ones (`PURGE`, `PROPFIND`, `M3THOD-X`), a `405`. |
| `params/` | `::: query`: disabled entries, a literal query in the URL plus a section, reserved characters, Unicode, repeated keys, empty values, variables in keys, values and paths. |
| `headers/` | Custom, disabled, variable and template function headers, a `User-Agent` override, response headers, repeated headers, `ETag` and `304`, cookies. |
| `bodies/` | `json`, `text`, `xml` and `form` bodies, disabled form fields, special characters, empty and invalid bodies, a `Content-Type` override, a body rewritten by a pre script. |
| `responses/` | JSON, XML, HTML, text, CSV, UTF-8, empty, binary, image, gzip, chunked, and 1, 5 and 15 MB bodies. |
| `auth/` | Basic, bearer, API key in a header and in the query, wrong credentials, login that stores a token, and a request that uses it. |
| `variables/` | The lookup order (request, session, environment), `env.set` and `env.unset`, an unresolved variable, no recursive expansion, every template function. |
| `scripting/` | Pre scripts changing headers, URL, method and body, every matcher, the `res` and `req` objects, `console`, a failing `test()` that does not stop the others, `util.*`, a fresh runtime per run. |
| `status/` | One request per common status code. |
| `redirects/` | A chain of three hops, and 301, 302, 303, 307 and 308. |
| `timing/` | 300 ms and 2 s delays, a slow chunked response. |
| `tls/` | One request per certificate profile, and a `POST` over a retried handshake. |
| `crud/` | A full create, read, update, delete cycle with pagination and search, numbered to run in order. |
| `expected-failures/` | Things that must fail: connection refused, a dropped connection, a redirect loop, an invalid URL, a throwing or malformed pre or post script, a red test. |

### Environments

`local` points `baseUrl` at the plain HTTP listener. `secure` points it at the self-signed HTTPS
listener, so the whole suite also runs over TLS: with the default setting every response then
carries a certificate warning. Both define the credentials, a few values used by `variables/`
and one URL per certificate profile (`tlsSelfSignedUrl`, ...), which the `tls/` requests use in
either environment.

## Certificates

Postino can only decide what to do with an untrusted certificate (Settings, "Invalid TLS
certificates": warn, reject or accept). It has no way to add a trusted CA or a client
certificate, so every HTTPS listener here is "invalid" for it and the profiles differ only in the
reason it reports. Two consequences:

- The verifier checks dates and the issuer before the host name, so `wrong-host` is reported as an
  unknown issuer. The profile stays for the day custom CAs exist, together with `ca.pem`, which
  is the CA that signed `private-ca.pem`.
- Retrying a request after a rejected certificate is safe for any method, because the handshake
  fails before a byte of the request is written. `tls/post-with-body` checks it.

The keys in `certs/` are public test fixtures that protect nothing. They are committed so that
running the server never needs OpenSSL. `certs/generate.sh` recreates all of them (OpenSSL 3.4 or
newer); the certificates are valid for 100 years, the `expired` and `not-yet-valid` ones on
purpose are not.

## Automatic checks

- `crates/postino-format/tests/samples_roundtrip.rs` parses every file and checks that
  `serialize(parse(text)) == text`, so a hand written request is already canonical.
- `crates/postino-runner/tests/sample_suite.rs` starts the server on ephemeral ports, runs the
  whole workspace in sidebar order with one session, once per environment, and checks each
  request: green tests, no unresolved variable (except the one that is meant to have it), the
  expected failure for everything in `expected-failures/`, and the expected certificate warning.
  It also runs `tls/` with the "reject" setting (every request must fail with a certificate
  error and send nothing) and with "accept" (no warning at all). It needs Python 3 and skips
  itself, printing why, when there is none.

Adding a request: put it in the right folder, give it a `test()`, run `cargo test -p
postino-runner --test sample_suite` and `cargo test -p postino-format --test samples_roundtrip`.
A request that is supposed to fail needs an entry in `expectation()` in `sample_suite.rs`.

## Known gaps

- There is no `multipart/form-data` body type yet. The server parses multipart, so a request can
  be added when the type exists.
- Postino keeps no cookie jar: `Set-Cookie` is shown, never stored or sent back.
