# Scripting

`.postino` requests can carry two small JavaScript sections, `::: pre` (runs before the request
is sent) and `::: post` (runs after the response is received). Both run on QuickJS, through
`rquickjs`, inside a sandbox with no access to the network, the filesystem, timers or anything
else on the host machine. This document describes the API exactly as implemented in
`crates/postino-script/src/prelude.js` and `crates/postino-script/src/quickjs_engine.rs`.

For the built-in `{{ }}` template functions available in URLs, headers and bodies without
writing a script at all, see `docs/format.md`, §10; every `util.*` function below is the exact
same Rust implementation.

## 1. Globals

### `req`

The request about to be sent (in `pre`) or the request as it was actually sent (in `post`).

- `req.method`, `req.url`: plain strings.
- `req.body`: a plain string. For a `form` body, this is the field list as `key=value&key2=value2`
  text (not yet percent-encoded; that happens later, when the request is resolved). For a request
  with no body at all, `req.body` reads as `""`.
- `req.headers`: `get(name)`, `set(name, value)`, `remove(name)`. Only *enabled* headers are ever
  visible or settable here; a disabled header in the file is invisible to the script and is
  carried through unchanged underneath whatever the script leaves behind.

**In `pre`**, every one of the above is writable: `req.method = "POST"`, `req.url = "..."`,
`req.headers.set(...)`, `req.body = "..."`. Values assigned with `set`/`body`/`method`/`url` are
converted with `String(value)`. Writing `req.body` when the request's body is `Body::None` (no
body section in the file) is silently ignored: the script has no way to say what *type* of body
it would want to create, so the write has no effect on what is actually sent.

**In `post`**, `req` is read-only: `req.method =`, `req.url =`, `req.body =`,
`req.headers.set(...)` and `req.headers.remove(...)` all throw. `req` here reflects the fully
resolved request that was actually sent over the wire (after `{{ }}` interpolation), not the
pre-script source.

### `res` (post only)

- `res.status`: number.
- `res.headers.get(name)`: read-only, `set`/`remove` throw.
- `res.body`: the response body as a string (decoded as UTF-8, lossily if it is not valid UTF-8).
- `res.json()`: `JSON.parse(res.body)`. Throws if the body is not valid JSON.
- `res.timeMs`: total time from opening the connection to reading the last byte, in milliseconds.
- `res.size`: the body size in bytes.

`res` does not exist at all in a `pre` script (there is no response yet).

### `vars`

Request-scoped variables set by the pre script, the highest-priority layer of variable
resolution (`docs/format.md`, §9).

- `vars.get(name)`
- `vars.set(name, value)` (value converted with `String(value)`)

There is no `vars.unset`. Values set here only live for the duration of this one run; they are
never written to disk and are not visible to any other request.

### `env`

Session overrides of the active environment, the second-priority layer.

- `env.get(name)`
- `env.set(name, value)` (value converted with `String(value)`)
- `env.unset(name)`

`env.set`/`env.unset` calls are collected and applied to the app's in-memory `SessionEnv` after
the script finishes, in the order they were made (a later `env.set("x", ...)` on the same key
wins). They persist **only for the app session**: they are visible to every later request in the
same run of the app (so a login token set by one request is visible to the next one) but are
never written back to an `.env` file, so they are gone the next time Postino is started.

### `test(name, fn)`

Runs `fn` immediately. If it throws, the test is recorded as failed with the thrown value's
message (`error.message`, or `String(error)` for a non-`Error` throw); otherwise it is recorded
as passed. **A failing test does not stop the script**: execution continues with whatever comes
after the `test(...)` call, and every subsequent `test()` still runs.

### `expect(value)`

Returns a matcher object. Every matcher throws (which `test()` catches) when the assertion does
not hold; `.not` returns an equivalent set of matchers with the condition inverted.

| Matcher | Passes when |
|---|---|
| `toBe(expected)` | `value === expected` |
| `toEqual(expected)` | deep equality, compared via `JSON.stringify(value) === JSON.stringify(expected)` |
| `toBeTruthy()` | `Boolean(value)` is `true` |
| `toBeFalsy()` | `!value` is `true` |
| `toContain(item)` | `value` is a string or array and contains `item` (throws if `value` is neither) |
| `toMatch(pattern)` | `String(value)` matches `pattern` (a `RegExp`, or a string turned into one with `new RegExp(pattern)`) |
| `toBeGreaterThan(other)` | `value > other` |
| `toBeLessThan(other)` | `value < other` |
| `toHaveProperty(name)` | `value` (coerced to an object) has an own property `name` |

`expect(value).not.toBe(expected)` (and so on for every matcher above) passes exactly when the
un-negated version would have failed.

### `console.log` / `.info` / `.warn` / `.error`

Each call joins its arguments with a single space and appends one line to the run's console
output (shown in the UI's Console tab), tagged with the level used. A string argument is used as
is; anything else is stringified with `JSON.stringify` when possible, falling back to
`String(value)`.

### `util`

The same built-in functions documented in `docs/format.md`, §10, callable directly:
`util.uuid()`, `util.now()`, `util.isoDate()`, `util.randomInt(min, max)`,
`util.randomString(len)`, `util.base64Encode(str)`, `util.base64Decode(str)`,
`util.urlEncode(str)`. `now()` and `randomInt()` return JavaScript numbers (so a script can do
arithmetic with them); every other function returns a string. A bad call (unknown function, wrong
arity, wrong argument type) throws an `Error` whose message is byte-for-byte the same as the
warning text the `{{ }}` template layer would produce for the equivalent call, since both go
through the exact same Rust dispatcher (`postino_core::functions::call`).

## 2. Semantics

- **Pre exception aborts sending.** An uncaught exception in a `::: pre` script (a syntax error or
  a runtime error) stops the whole run: the request is never sent, and the error is shown as the
  failure of that run.
- **Post exception is reported, response kept.** An uncaught exception in a `::: post` script does
  *not* discard anything: the response that was already received is still shown, along with
  whatever `test()`/`console.*` calls ran before the exception was thrown; the exception itself is
  reported alongside it as the run's failure.
- **Limits:** 32 MiB of QuickJS memory and 5 seconds of wall-clock time per script run (`pre` and
  `post` are each their own budget). Exceeding either one is reported as a script error; it never
  crashes the app.
- **Isolation:** the sandbox has no module loader, no `fetch`, no timers (`setTimeout` etc.), no
  `require`/`import`, and no other host access. The only things available beyond standard
  ECMAScript are the globals documented above.
- **A fresh runtime per run.** Every single call to run a `pre` or `post` script creates a brand
  new QuickJS `Runtime` and `Context` and tears it down afterward. Nothing (`globalThis` included)
  leaks from one script run into the next, even across two runs of the exact same request.
- An empty `::: pre` or `::: post` section (or a section that is entirely whitespace) is skipped
  without starting the engine at all; the request/vars pass through unchanged.

## 3. Examples

### 3.1 Login, store a token, use it later

`auth/login.postino`:

```
POST {{baseUrl}}/auth/login
Content-Type: application/json
X-Request-Id: {{requestId}}

::: body json
{ "username": "{{username}}", "password": "{{password}}" }

::: pre
vars.set("requestId", util.uuid());

::: post
test("login succeeded", () => {
  expect(res.status).toBe(200);
});
env.set("token", res.json().token);
```

Any later request in the same session can then use `{{token}}`, for example
`Authorization: Bearer {{token}}`, because `env.set` in the post script above put it in the
session environment layer.

### 3.2 A generated header

```
::: pre
req.headers.set("X-Timestamp", String(util.now()));
req.headers.set("X-Nonce", util.randomString(16));
```

### 3.3 Response assertions

```
::: post
test("status is 2xx", () => {
  expect(res.status).toBeGreaterThan(199);
  expect(res.status).toBeLessThan(300);
});

test("body has an id", () => {
  expect(res.json()).toHaveProperty("id");
});

test("content type is not missing", () => {
  expect(res.headers.get("Content-Type")).not.toBeFalsy();
});

console.log("received", res.size, "bytes in", res.timeMs, "ms");
```

### 3.4 Failing fast without aborting other tests

```
::: post
test("status is 201", () => {
  expect(res.status).toBe(201);
});
test("etag header is present", () => {
  expect(res.headers.get("ETag")).toBeTruthy();
});
```

If the first `test()` fails (say the API returned `200`), the second one still runs: a failing
`test()` never stops the script.

## 4. Postman import

When importing a Postman collection, `prerequest` and `test` events attached to an individual
request are copied into that request's `::: pre`/`::: post` sections, but **entirely commented
out**, with a fixed first line:

```
// Imported from Postman. The pm.* API is not supported, adapt it.
```

`pm.*` (Postman's own script API: `pm.environment`, `pm.test`, `pm.response`, ...) has no
equivalent in Postino, so nothing is translated automatically; the commented-out code is only a
starting point to adapt by hand using the globals documented above.

A `prerequest` or `test` event attached at the **collection or folder level**, rather than to one
specific request, is **not imported at all**: there is no single request for it to attach to.
Instead, the import reports a warning for each one it drops, naming the collection or folder and
the kind of script, for example:

```
folder "Users": a test script is not imported at the folder level, only per request
```
