# The `.postino` file format

This document describes the `.postino` request file format and the `.env` environment file
format as actually implemented in `crates/postino-format`. It is the reference for anyone
writing or editing these files by hand, or building another tool that reads or writes them.

A `.postino` file is one HTTP request: method, URL, headers, query parameters, a body, an
optional pre-request script, an optional post-response script, and free-form docs. A collection
is just a folder of `.postino` files, so folders and files are what you see in git.

## 1. Example

This is the full example used as a fixture in `crates/postino-format/src/format.rs` (it round
trips exactly, byte for byte):

```
POST {{baseUrl}}/users
Content-Type: application/json
Authorization: Bearer {{token}}
X-Timestamp: {{ now() }}
# X-Debug: 1

::: query
page = 1
# verbose = true

::: body json
{ "name": "{{userName}}", "id": "{{requestId}}" }

::: pre
vars.set("requestId", util.uuid());

::: post
test("created", () => {
  expect(res.status).toBe(201);
});
env.set("userId", res.json().id);
```

## 2. General rules

- Encoding is UTF-8.
- Line endings: both `\n` and `\r\n` are accepted when reading. The serializer always writes
  `\n`, and normalizes any input to `\n` internally before parsing, so a file saved by Postino
  never has `\r\n` in it even if it started with some.
- The request **name** shown in the sidebar is the file name without the `.postino` extension.
  Sidebar order is a natural sort of names (so `item2` sorts before `item10`), folders first.

## 3. Request line

The first non-blank line of the file is the request line: `<METHOD> <URL>`.

- `METHOD` is the first whitespace-separated token. It becomes one of the standard `Method`
  variants (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`) when it matches one of
  those words exactly, or `Method::Custom(token)` otherwise, keeping the token verbatim. This
  means any token works as a custom method as long as it parses (see the validation rule below),
  including one made only of digits or symbols; it does not have to look like a real HTTP verb.
- The parser rejects a method token that contains any lowercase ASCII letter, with
  `invalid method`. It does **not** require every character to be an uppercase letter: digits and
  symbols are fine as long as there is no lowercase letter. `get https://...` is rejected;
  `PURGE https://...` and `M3THOD https://...` are accepted.
- The rest of the line, trimmed, is the URL. It may contain `{{ }}` markers and can already
  include a literal query string; if there is also a `::: query` section, its entries are
  appended (see §7.5).
- A missing URL (`GET` alone on the line) is a parse error, `missing URL in request line`.
- A file that is empty or contains only blank lines has no request line at all: `missing request
  line`, reported at line 1.

## 4. Header block

Every line right after the request line is a header, until the first blank line or the first
section marker (`::: `), whichever comes first.

- An active header line is `Name: value`, split on the **first** `:` only, both sides trimmed.
  So a value that itself contains a colon (a URL, a time) is kept intact:
  `Location: https://a.test/b:8080` parses to `Location` / `https://a.test/b:8080`.
- A line whose first non-space character is `#` is a **disabled** header: the `#` and any spaces
  after it are stripped, then the rest is parsed as `Name: value` as usual, with `enabled: false`.
- A header line with no `:` at all is a parse error, `invalid header line`.
- Header order is preserved exactly as written, including the mix of enabled and disabled
  entries.
- A header line is subject to the same `::: ` escaping described in §7.4, in case a header value
  were ever to start with the section marker text (practically never happens, since the line
  starts with `Name:`, but the serializer applies the same escaping function uniformly).

If the header block ends because of a blank line, that blank line is consumed. If it ends because
a `::: ` marker was found directly (no blank line in between), the marker line is left for the
section parser to read next.

## 5. Sections

After the header block, the rest of the file is zero or more **sections**, each introduced by a
line starting with `::: ` (three colons and a space): `::: <kind> [argument]`.

- **Kind matching is case-sensitive.** `::: Query` or `::: PRE` are `unknown section kind`
  errors, not `query`/`pre` in disguise.
- Blank lines between the header block and the first section, and between sections, are skipped
  by the parser; any number of blank lines is fine. A non-blank line that is not a section marker
  where a section is expected is a parse error, `expected a section marker`, whether that happens
  right after the header block or between two sections.
- Each section kind may appear **at most once**. A second `::: query`, or a second `::: body`
  even with a different type argument, is `duplicated section`.
- An unrecognized kind is `unknown section kind`. The only recognized kinds are `query`, `body`,
  `pre`, `post` and `docs`.
- `query`, `pre`, `post` and `docs` take no argument; giving one (`::: pre extra`) is
  `section "pre" does not take an argument`.
- `body` requires exactly one argument, its type: `json`, `text`, `xml` or `form`. No argument at
  all is `missing body type`; any other word is `unknown body type`.
- The content of a section is every line after its marker, up to the next `::: ` marker or end of
  file. **Trailing blank lines of that content are dropped** (interior blank lines are kept, so a
  multi-paragraph `docs` section can have blank lines between paragraphs; only the ones right
  before the next marker or EOF are trimmed).

### 5.1 `query`

Lines are `key = value`, split on the first `=`, both sides trimmed. A `#` prefix (with optional
following spaces) disables the entry, same convention as headers. A line without `=` is a key
with an empty value (`verbose` alone becomes the key `verbose` with value `""`). Blank lines
inside the section are ignored and produce no entry.

### 5.2 `body <type>`

- `json`, `text`, `xml`: the section content is kept as raw text, joined back with `\n`.
- `form`: the content uses the same `key = value` lines as `query` (including the `#` disable
  prefix and the no-`=` rule).
- Unlike the other sections, `body` is still written to the file even when its text is empty:
  `Body::Json(String::new())` serializes as a `::: body json` heading with no content lines
  underneath, because the body's *type* is information that would otherwise be lost. This is the
  one section whose presence does not depend on whether it has any content, only on whether the
  request has a body at all (`Body::None` is what is actually omitted).

### 5.3 `pre` / `post`

Raw JavaScript text, run as described in `docs/scripting.md`.

### 5.4 `docs`

Free-form Markdown notes, raw text, not interpreted by Postino itself.

## 6. Canonical serialization order

The serializer always writes, in this exact order: request line, headers, then (only if there is
at least one non-empty section) a blank line followed by the sections `query`, `body`, `pre`,
`post`, `docs` in that fixed order, each pair of sections separated by exactly one blank line. The
file always ends with a single trailing `\n`.

An important detail: **the blank line that separates the header block from the sections is only
written when there is at least one section to write.** A request with no query, no body, no
scripts and no docs (just method, URL and headers) serializes as the request line and headers
with no trailing blank line at all, for example:

```
GET https://example.com
Accept: */*
```

A section that is structurally absent is omitted entirely: an empty `query` list, `Body::None`, or
an empty `pre_script`/`post_script`/`docs` string. See §5.2 for the one exception (a `body` whose
type is set but whose text is empty).

The parser accepts sections in any order and with any spacing; only the serializer is canonical.
This is what keeps git diffs clean: two semantically identical requests always serialize to the
exact same bytes.

## 7. Escaping

### 7.1 Why

A raw section's content (a script, a JSON body, a docs paragraph) is plain text that could,
coincidentally, contain a line that looks like a section marker (`::: something`). Since the
parser scans for the next `::: ` line to know where a section ends, that line would otherwise be
misread as the start of a new section.

### 7.2 The rule

Exactly one escaping rule covers this, applied uniformly wherever a line could be confused with a
marker (raw section content, and header lines, see §4):

- **On write:** if a line, after stripping any number of leading backslashes, starts with
  `::: `, one backslash is prepended to it.
- **On read:** if a line, after stripping any number of leading backslashes, starts with `::: `,
  exactly one leading backslash is removed from it (if the line has no leading backslash at all,
  it is left as is, since it does not need unescaping).

This means:

| Original content line | Written as | Read back as |
|---|---|---|
| `::: not a section` | `\::: not a section` | `::: not a section` |
| `\::: already had a backslash` | `\\::: already had a backslash` | `\::: already had a backslash` |
| `regular text` | `regular text` | `regular text` |

So a single backslash is always added on write and always removed on read, regardless of how many
backslashes were already there; a genuinely literal backslash right before `::: ` in the source
data survives by picking up one more backslash each round trip, which is exactly what makes
`serialize(parse(text)) == text` and `parse(serialize(model)) == model` both hold.

### 7.3 Round-trip guarantee

`serialize(parse(text)) == text` for every canonically formatted file, and
`parse(serialize(model)) == model` for every model built in Rust. Both directions are covered by
tests in `crates/postino-format/src/format.rs` and the fixtures under
`crates/postino-format/tests/fixtures/`, including CRLF input, escaped markers, empty values,
unicode, `=` inside values, and `:` inside header values.

## 8. Environment files (`.env`)

`environments/<name>.env`, one flat list of variables, versioned normally.
`environments/<name>.local.env` is the optional, gitignored counterpart for secrets; its values
override `<name>.env`. Add this to the workspace's `.gitignore`:

```
*.local.env
```

The `environments/` folder itself is not shown as a collection in the sidebar; it is read
separately.

Rules, exactly as implemented in `crates/postino-format/src/env.rs`:

- One `KEY=value` per line, split on the **first** `=`.
- The **key** is trimmed. The **value is kept verbatim**, with no trimming, no quoting and no
  escaping: `KEY = value with spaces ` parses to key `KEY`, value `" value with spaces "` (the
  leading/trailing spaces around the value are part of it).
- A line is a comment, and is skipped, when its **trimmed** content starts with `#` (so an
  indented `  # comment` still counts as a comment). Comment detection happens on the trimmed
  line; the actual key/value split, when the line is not a comment, still uses the untrimmed line.
- Blank lines (empty or whitespace-only) are skipped.
- A non-blank, non-comment line with no `=` at all is a parse error: `missing '=' in ...`.
- Both `\n` and `\r\n` are accepted on read; the serializer always writes `\n`.
- Duplicate keys are **not** merged: every line becomes its own entry, in file order. When looking
  a variable up (see §9), the **first** matching entry in a layer wins, so if a key is defined
  twice in the same file, its first definition is the one that is actually used; the later
  duplicate is parsed but never seen.

Merging `<name>.local.env` over `<name>.env` (done by `postino-workspace`, not this module): a key
present in both keeps its **position** from the base file but takes its **value** from the local
file; a key only in the local file is appended at the end, in the local file's order. At least one
of the two files must exist for `load_environment` to succeed.

## 9. Variable resolution

`{{name}}` (whitespace inside the braces is allowed: `{{ name }}`) is replaced wherever
interpolation runs: the URL, header names and values, query keys and values, form field keys and
values, and raw bodies (`json`/`text`/`xml`). Lookup order, **first hit wins**:

1. request variables set by the pre script this run (`vars.set`);
2. runtime environment overrides set by scripts (`env.set`), kept for the whole app session;
3. the active environment (`.local.env` merged over `.env`, see §8).

A disabled entry (`enabled: false`) in any layer is skipped as if it were not present, falling
through to the next layer.

An unresolved `{{name}}` (no matching variable in any layer) is left exactly as written in the
output, and reported as a warning (`unknown variable`) alongside the run's result; it never aborts
anything. Interpolation is a **single pass**: the text that replaces a marker is copied into the
output as is and never rescanned, so a variable whose own value contains `{{x}}` does not expand
further (`{{outer}}` where `outer` = `{{inner}}` stays `{{inner}}` in the output). An unmatched
`{{` with no following `}}` anywhere in the rest of the text is left untouched with no warning at
all: it is simply not treated as a marker.

## 10. Template functions

Inside `{{ }}`, content that looks like `name(arg, arg, ...)` (an identifier immediately followed
by `(`, ending in `)`) is evaluated as a function call instead of looked up as a variable name.

**Grammar:** `name(arg, arg, ...)`, comma-separated, no nesting of calls and no operators. Each
argument is one of:

- a double-quoted string literal, with `\"` and `\\` as the only recognized escapes
  (`"user:pass"`, `"say \"hi\""`);
- a bare integer literal (`1`, `-5`);
- a bare identifier, looked up as a variable using the exact same three-layer lookup as §9.

**Built-in functions:**

| Function | Arguments | Result |
|---|---|---|
| `uuid()` | none | a random UUID v4 |
| `now()` | none | milliseconds since the Unix epoch |
| `isoDate()` | none | current UTC time, RFC 3339 (`2026-09-25T13:11:00Z`) |
| `randomInt(min, max)` | two integers | a random integer in `[min, max]` inclusive (bounds swapped if `min > max`) |
| `randomString(len)` | one integer | a random alphanumeric string of that length |
| `base64Encode(str)` | one string | standard base64 encoding |
| `base64Decode(str)` | one string | the decoded string (errors if not valid base64 or not valid UTF-8 once decoded) |
| `urlEncode(str)` | one string | percent-encoded, unreserved characters `A-Z a-z 0-9 - . _ ~` left as is |

**Error behavior:** an unknown function name, the wrong number of arguments, an argument of the
wrong type (for example a string where an integer is expected), or an unresolved variable used as
an argument, all behave exactly like an unresolved plain variable (§9): the whole `{{ ... }}`
marker is left untouched in the output, and one warning is added. Nothing here ever aborts
interpolation.

Each occurrence is evaluated independently: `{{ uuid() }}|{{ uuid() }}` produces two different
UUIDs.

These functions are implemented once, in plain Rust, in `postino-core::functions`, and reused
verbatim by `postino-script` as the `util.*` object described in `docs/scripting.md`, so a
template call and the equivalent `util.*` call in a script always behave identically, including
the exact wording of their errors.
