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
| Body modes `formdata`, `file` and `graphql` | Imported as an empty body, with a warning |
| Auth `bearer`, `basic` and `apikey` (header location) | Headers |
| Description | `::: docs` section |
| `{{var}}` | Kept as is, the syntax is compatible |
| Collection `variable` array | `environments/<collection name>.env` |

Auth set on the collection or on a folder is inherited by the requests below it that use
`inherit` or define no auth of their own.

Scripts are not translated. `prerequest` and `test` events of a request are copied into `::: pre`
and `::: post` fully commented out, so they stay as a starting point to adapt by hand. Events at
the collection or folder level are not imported at all, and each one raises a warning. See
`docs/scripting.md`.

## Environments

A Postman environment export (`*.postman_environment.json`) becomes `environments/<name>.env`.
Entries of type `secret` go to `environments/<name>.local.env` instead, which is meant to be
ignored by git.
