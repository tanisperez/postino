# Postman import

Postino imports Postman Collection format v2.1 (v2.0 is accepted when it parses the same) and
Postman environment exports. The import never overwrites existing files and returns a report with
the files it created and the warnings it raised. Only the fields that map to something in
Postino are read; the rest of the JSON is ignored.

The code is in `postino_format::postman` (parsing and mapping, no IO) and
`postino_workspace::postman` (writing the result into a workspace).

## Collections

The import writes into a new folder named after the collection, inside the workspace. Names are
sanitized for every OS (Windows-reserved names and characters, trailing dots and spaces, at most
120 characters), and a duplicate gets a ` (2)`, ` (3)` suffix.

| Postman | Postino |
| --- | --- |
| Folder | Directory |
| Request | A `.postino` file |
| Method | Request line method |
| `url.raw` without its query string, plus `url.query` | URL, and the `::: query` section |
| Headers, including `disabled` ones | Headers, disabled ones as `#` lines |
| Body mode `raw` (language json, xml or text, text by default) | `::: body json`, `xml` or `text` |
| Body mode `urlencoded` | `::: body form` |
| Body mode `formdata` | `::: body multipart`, a file field as `@path` |
| Body modes `file` and `graphql` | Imported as an empty body, with a warning |
| Auth `bearer`, `basic` and `apikey` (header or query location) | `::: auth` section |
| Auth `oauth2` | An OAuth profile (`oauth_import::ProfileBuilder`), `::: auth oauth2`; secrets in clear go to `.local.env`, the stored token is dropped |
| Description | `::: docs` section |
| `{{var}}` | Kept as is, the syntax is compatible |
| Collection `variable` array | Copied into every imported environment; `environments/<collection name>.env` when there is none. Those of type `secret` go to `.local.env` |

Auth set on the collection or on a folder is inherited by the requests below it that use
`inherit` or define no auth of their own.

Scripts of a request are converted by `postman/script.rs`, on tokens outside strings and comments.
A statement that still uses something Postino does not have (`pm.sendRequest`, `CryptoJS`, a
matcher with no equivalent) stays as a comment, and a script whose brackets do not balance is
commented out whole. `tests/postman_scripts.rs` in `postino-runner` runs converted scripts in the
sandbox. The result per request is a list of `RequestIssue`, for the wizard's preview. Events at
the collection or folder level are not imported at all, and each one raises a warning. See
`docs/scripting.md`.

## Environments

A Postman environment export (`*.postman_environment.json`) becomes `environments/<name>.env`.
Entries of type `secret` go to `environments/<name>.local.env` instead, which is meant to be
ignored by git.

## The wizard

Postman uses the import wizard of OpenAPI (`state/openapi_import.rs`, `views/openapi_import.rs`).
`detect_format` tells the format from the content, on any source. `build_postman_plan` gives an
`OpenApiPlan`, so the preview, the selection, `Workspace::import_openapi` and the manifest are one
code path. The request key is `<folders>/<METHOD> <name>`, ` #2` when two collide, and the manifest
source is `kind = "postman"`, `location = <info._postman_id>`. The report is written by the wizard
to `.postino/import-report.md`.

An export whose id is already in the destination's manifests turns the Preview into the update of
`docs/openapi-import.md` (`diff_plan`, `Workspace::update_openapi`).
