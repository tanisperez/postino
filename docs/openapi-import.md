# OpenAPI import

How an OpenAPI 3.1, 3.0 or Swagger 2.0 spec becomes Postino requests (GitHub #201). The user
guide is `site/docs-src/pages/openapi-import.html`; this document is for developers and must agree
with it.

## Layers

| Layer | Where | IO |
| --- | --- | --- |
| Reading a spec into a normalized `Spec` | `postino-format`, `openapi/spec.rs` | none |
| Documents and `$ref` resolution | `postino-format`, `openapi/document.rs` | through a `RefLoader` |
| Example values from schemas | `postino-format`, `openapi/example.rs` | none |
| `Spec`s plus `ImportOptions` to an `OpenApiPlan` | `postino-format`, `openapi/generate.rs` | none |
| Writing the selected part of a plan | `postino-workspace`, `openapi.rs` | files |
| Reading a file, downloading a URL, the wizard state | `postino-app`, `state/openapi_import.rs` | files, HTTP |
| The wizard | `postino-app`, `views/openapi_import.rs` | none |

`postino-format` stays pure: `parse_spec(text, location, loader)` takes a `RefLoader` that the
caller implements. The app's `SourceLoader` reads files relative to a file and fetches URLs
relative to a URL, with a `postino_runner::Client` built from the request send options, so the
download honors the TLS settings. Pasted text uses `NoExternalRefs`.

## Reading a spec

1. The text is JSON when it starts with `{`, YAML otherwise (`serde_norway`, merge keys applied),
   and becomes a `serde_json::Value`. YAML keys that are numbers (`200:`) become strings.
2. The `openapi` field must start with `3.0` or `3.1`, or `swagger` must be `2.0`. Anything else
   is `OpenApiError::NotOpenApi` or `UnsupportedVersion`.
3. Every external `$ref` (the part before `#`) is loaded up front, recursively, at most 64
   documents. Afterwards `Node` navigation follows `$ref` chains transparently, at most 32 hops.
   A reference that cannot be followed is a warning and the value counts as missing.
4. Swagger 2.0 is read into the same model: `host`, `basePath` and `schemes` give one server
   (`https` when listed), `body` and `formData` parameters give the request body, `consumes`
   picks the media type, `securityDefinitions` the schemes.

Normalization rules worth knowing:

- Server URL variables take their `default`. A relative server URL is resolved against the spec's
  location when it is a URL; an OpenAPI 3 spec without `servers` fetched from a URL gets that
  URL's origin.
- Path item parameters merge with the operation's, which win by `(name, in)`.
- Header parameters named `Accept`, `Content-Type` or `Authorization` are dropped, as the spec
  says.
- The body media type is chosen in this order: `application/json`, other JSON (`*/json`,
  `*+json`), forms, XML, `text/*`, else the first.
- `security` on the operation replaces the global one; only the first requirement object is used.
- Success is the first `2xx` (or `2XX`) response in document order.

## Building the plan

`build_plan(collection_name, &[SpecVersion { label, spec }], &options)`. A label names the folder
of one spec when several are imported together; with one spec it is empty.

- **Request key**: `[<label> ]<METHOD> <path>`, stable across rebuilds, so the wizard keeps the
  selection when an option or the grouping changes.
- **Folders**: path segments (`/customers/{id}` in `customers/{id}`) or the first tag.
- **URL**: `{{baseUrl}}` + the server's path + the path with `{x}` turned into `{{x}}`. Without
  the environments option, the first server URL is written as is.
- **Environments**: one per distinct server origin, `baseUrl` = the origin. One origin: named
  after the collection. Several: `<collection> <label>`, the label being the server description
  (32 characters at most) or the host, or the whole origin when two labels clash. No absolute
  server URL: `baseUrl = http://localhost` and a warning.
- **Auth**: basic, bearer (also OAuth 2 and OpenID Connect, with a warning) and API keys map to
  `Auth`; variable names are the scheme names with other characters replaced by `_`. Secrets are
  written empty to `<environment>.local.env` of every environment. Further API keys of the same
  requirement become headers or query entries.
- **Bodies**: `example`, the first of `examples`, or a value built from the schema
  (`example.rs`: explicit values first, `allOf` merged, `oneOf`/`anyOf` first, read only
  properties skipped, depth 6, 40 properties, `$ref` cycles stop). A `Content-Type` header is
  added when the media type is not the body type's default. An empty body on GET or HEAD is
  dropped.
- **Tests**: `expect(res.status).toBe(<status>)`, or a range check for `2XX`.
- **Docs**: `# summary`, `**Deprecated.**`, the description and a parameter list.

`PlanFolder::to_import_folder(keep)` drops unselected requests and empty folders, and gives the
same `ImportFolder` the Postman import writes, so `Workspace::import_openapi` reuses its writer:
sanitized names, ` (2)` suffixes, nothing overwritten.

## The wizard

`state::openapi_import::Wizard` holds the step, the source, the loaded specs, the options, the
plan (an `Arc`, rebuilt only when entering Options or Preview), a second plan with every option
on (for the descriptions of the options), the excluded keys, the collapsed folders and the
preview rows with their counts. Rows and counts are rebuilt when the selection or a folder
changes, never in render. The preview tree is a `uniform_list`.

Loading a file or a URL runs on the background executor; parsing the 11 MB GitHub spec takes about
250 ms in release. Without a workspace, Import hands the plan and the exclusions to the "New
workspace" dialog (`NewWorkspacePurpose::OpenApi`), which imports once the folder is open.

## Not done yet

- Folder and Git repository sources, `catalog-info.yaml`, several specs in one import with a
  Versions list (#202).
- Re-importing an updated spec with a diff (#203).
- Headers for a URL that needs auth to download.
