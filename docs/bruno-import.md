# Bruno import

Postino imports Bruno collections in both of Bruno's on-disk formats (#213, #221):

- `.bru` files: `bruno.json` at the root, `collection.bru`, `folder.bru` in a folder, one `.bru`
  file per request, `environments/*.bru`.
- OpenCollection YAML, which Bruno 3.0 added and recommends for new collections:
  `opencollection.yml` at the root, `folder.yml`, one `.yml` file per request,
  `environments/*.yml`.

Both can be mixed in one collection, as Bruno allows during a migration.

The code is in `postino_format::bruno` (no IO: the caller passes each file's path and text). It
produces the same `ImportPlan` and `ImportedEnvironment` values as a Postman import, so
`build_postman_plan`, the import wizard, the report and `postino-workspace` are shared. The
references for the formats are in usebruno/bruno: `packages/bruno-lang/v2/src/bruToJson.js` and
`envToJson.js` for `.bru`, and `packages/bruno-converters/src/opencollection` for YAML (what
Bruno writes; `body: { mode, json }`, an older form, is read too).

## In the app

The import wizard (`state/openapi_import.rs`, `views/openapi_import.rs`, #222) has a Bruno
format next to OpenAPI and Postman, with the Folder, File and Paste sources. A folder is a Bruno
collection when it has `bruno.json` or `opencollection.yml`; a collection one or two folders
down is only taken when the folder has no spec or Postman export, so a repository with specs
still imports them. A dropped folder reads like the Folder source, a `.zip` with a root file or a
`.bru` file is read as a collection, and picking a root file reads its folder. The Destination
block never offers to use a Bruno folder as the workspace. The options are a Postman import's,
with the docs on, plus Environment colors: each color is mapped to the nearest of the nine
named colors by hue (`EnvColor::nearest`) and written to `.postino/project.toml` after the
import, for the environments the workspace has.

Importing a collection already imported into the destination updates it (#223), as for Postman
(#212): the manifest source is `kind = "bruno"` with the collection's name as `location`
(`postino_format::bruno::MANIFEST_KIND`, `Wizard::bruno_source`), so it is found wherever the
folder or `.zip` is. Loose request files have no name to match and always import as new.

## Files

| Module | Role |
| --- | --- |
| `bruno/bru.rs` | Reads `.bru` text into blocks: dictionaries (`~` disabled keys, quoted keys, `'''` values, lists), text blocks outdented by two spaces, list blocks, single lines (`color:`, `extends:`). Annotations (`@description(...)`) are skipped. |
| `bruno/yaml.rs` | Reads OpenCollection YAML with `serde_norway`. |
| `bruno/model.rs` | The shape both formats are read into. |
| `bruno/convert.rs` | From that shape to `ImportPlan`. |
| `bruno/script.rs` | Scripts, tests, `assert` blocks and request variables to Postino's script API. |
| `script_convert.rs` | The statement splitter and Chai chain conversion shared with the Postman import. |

## Collections

The root is the shallowest folder with `bruno.json` or `opencollection.yml`. Without one, the
files passed are taken as a collection when at least one is a request, so a single `.bru` or
`.yml` request imports. Folders named `node_modules` and hidden folders (`.git`) are skipped.

| Bruno | Postino |
| --- | --- |
| Collection name (`bruno.json` `name`, `opencollection.yml` `info.name`) | The collection folder |
| Folder, named by its `folder.bru` / `folder.yml`, else by its directory | Directory |
| Request (`meta.name` or `info.name`, else the file name) | A `.postino` file |
| Method block (`get`, ..., `http` with `method` for a custom one) | Request line method |
| URL with `:name` path parameters | The URL with the `params:path` values filled in |
| `params:query` | `::: query`. The URL's own query string is dropped when they are listed, or it would be sent twice |
| Headers of the collection, its folders and the request | Headers, the nearest one winning for a name (case insensitive) |
| `body:json`, `text`, `xml` | `::: body json`, `text`, `xml` |
| `body:form-urlencoded` | `::: body form` |
| `body:multipart-form`, `@file(path)` | `::: body multipart`, `@path`; several files keep the first, with a warning |
| GraphQL request (`body:graphql`, `body:graphql:vars`) | `::: body json` with `{"query": ..., "variables": ...}` and `Content-Type: application/json`, how GraphQL is sent over HTTP |
| `body:sparql`, `body:file` | No body, reported |
| Auth `basic`, `bearer`, `apikey` (header or `queryparams`) | `::: auth` |
| Auth `inherit` | The nearest folder's or the collection's auth |
| Auth `oauth2` | An OAuth profile named after the collection, `::: auth oauth2`, as for Postman |
| Auth `digest`, `ntlm`, `awsv4`, `wsse`, `oauth1`, ... | No auth, reported |
| `docs` | `::: docs`, with the Docs option |
| gRPC and WebSocket requests | Not imported, reported |
| Variables of the collection and its folders | Copied into every environment, a folder's value winning over the collection's |
| Scripts and tests of the collection or a folder | Not imported, reported per level, as for Postman |
| `.env` at the root, `process.env.*` | Not read, reported |

## Environments

`vars` become the environment's `.env` file; disabled ones are left out. Secret variables
(`vars:secret [...]`, or `secret: true` in YAML) have no value in the collection, since Bruno
keeps them encrypted on each machine: they become empty entries of `.local.env`, and a warning
names them. `extends` is resolved: the parent's variables the environment does not set are copied
in. The environment's `color` is returned as written, for the wizard to map to one of Postino's
nine colors in `.postino/project.toml` (#217).

## Scripts

A request's parts are composed in the order Bruno runs them:

- `::: pre`: `vars:pre-request` as `vars.set("name", "value")`, then `script:pre-request`.
- `::: post`: `vars:post-response` as `env.set` (`vars.set` for a `@local` one), then
  `script:post-response`, the `assert` block as `test()` calls, then `tests`.

Rewrites (`bruno/script.rs`):

| Bruno | Postino |
| --- | --- |
| `bru.getEnvVar` / `setEnvVar` / `deleteEnvVar` | `env.get` / `env.set` / `env.unset` |
| `bru.getVar` / `setVar` / `deleteVar`, `bru.getCollectionVar`, `getFolderVar`, `getGlobalEnvVar` and their setters | `env.get` / `env.set` / `env.unset` (session values) |
| `bru.getRequestVar` | `vars.get` |
| `req.getUrl()`, `req.setUrl(x)` | `req.url`, `req.url = x` |
| `req.getMethod()`, `req.setMethod(x)` | `req.method`, `req.method = x` |
| `req.getHeader`, `setHeader`, `deleteHeader` | `req.headers.get`, `set`, `remove` |
| `req.getBody()`, `req.getBody({ raw: true })` | `JSON.parse(req.body)`, `req.body` |
| `req.setBody(x)` | `req.body = x`, an object written as JSON |
| `res.body`, `res.getBody()` | `res.json()`: Bruno parses JSON bodies, Postino's `res.body` is the text |
| `res.getStatus()` | `res.status` |
| `res.getHeader(name)` | `res.headers.get(name)` |
| `res.responseTime`, `res.getResponseTime()` | `res.timeMs` |
| `expect(x).to...` (Chai) | Postino's matchers, as for `pm.expect` |

`expect(x).toBe(...)` and the other Postino matchers are left as they are. A statement that
still uses something with no equivalent is kept as a comment: any other `bru.*` (`sendRequest`,
`runRequest`, `setNextRequest`, `runner`, `cookies`, `sleep`, `interpolate`, ...), a `req` or
`res` member Postino does not have, `res.headers` read as an object, a Chai chain with no
matcher (`jsonSchema`, `lengthOf`, `a('string')`), `require` and `process`.

Differences that remain after a conversion:

- Postino's variables are text: a number stored with `bru.setVar` comes back as a string.
- `bru.setEnvVar` writes the environment file in Bruno; `env.set` keeps the value for the app
  session only.
- `res.json()` throws on a body that is not JSON, where Bruno's `res.body` was the text.

### Assertions

Each enabled entry of `assert` becomes a `test()` named after the line, for example
`$res.status: eq 200` becomes `expect(res.status).toBe(200)`. The expression may be `res.status`,
`res.responseTime`, `res.body...` (read from `res.json()`) or `res.headers.<name>`; a value is a
number, a boolean, `null`, a quoted string, a `{{variable}}` (read with `vars.get` or
`env.get`) or else a string. The operators are those of Bruno: `eq`, `neq`, `gt`, `gte`, `lt`,
`lte`, `in`, `notIn`, `contains`, `notContains`, `length`, `matches`, `notMatches`,
`startsWith`, `endsWith`, `between`, `isEmpty`, `isNotEmpty`, `isNull`, `isUndefined`,
`isDefined`, `isTruthy`, `isFalsy`, `isNumber`, `isString`, `isBoolean`, `isJson`, `isArray`. An
entry without an operator is `eq`. One that cannot be converted (`res('path')` queries) stays as
a comment.

## Tests

- `postino-format`: `bruno/*` unit tests, and `bruno/tests.rs` with whole collections in both
  formats.
- `postino-runner/tests/bruno_scripts.rs` runs a converted request, scripts, assertions and
  request variables included, in the QuickJS sandbox against a local server.
- Checked by hand against Bruno's own test collections (`packages/bruno-tests/collection`, 227
  requests, and `yml-collection`): every request converts to a `.postino` file that parses back
  the same.
