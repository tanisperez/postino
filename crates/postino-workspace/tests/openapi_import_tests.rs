//! `Workspace::import_openapi`: the selected requests and the environments end up on disk, and
//! every written request parses back.

#![allow(clippy::expect_used)]

use postino_format::openapi::{ImportOptions, NoExternalRefs, SpecVersion, build_plan, parse_spec};
use postino_workspace::Workspace;
use pretty_assertions::assert_eq;

const SPEC: &str = r##"
openapi: 3.1.0
info: {title: Acme Payments API, version: 2.0.0}
servers: [{url: https://api.acme.dev/v2}]
security: [{bearerAuth: []}]
components:
  securitySchemes:
    bearerAuth: {type: http, scheme: bearer}
paths:
  /customers:
    get: {summary: List customers, responses: {'200': {description: ok}}}
    post:
      summary: Create customer
      requestBody: {content: {application/json: {example: {name: Ada}}}}
      responses: {'201': {description: created}}
  /customers/{id}:
    get: {summary: Get customer, responses: {'200': {description: ok}}}
"##;

#[test]
fn writes_the_selected_requests_and_the_environments() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("Acme Payments API")).expect("a clashing folder");
    let mut workspace = Workspace::open(dir.path()).expect("an empty folder opens");
    let spec = parse_spec(SPEC, "", &NoExternalRefs).expect("valid");
    let plan = build_plan(
        &spec.title,
        &[SpecVersion {
            label: "",
            spec: &spec,
        }],
        &ImportOptions::default(),
    );

    let report = workspace
        .import_openapi(&plan, &|key| key != "GET /customers/{id}")
        .expect("the import succeeds");

    assert_eq!(
        report.created_files,
        vec![
            "Acme Payments API (2)/customers/List customers.postino",
            "Acme Payments API (2)/customers/Create customer.postino",
            "environments/Acme Payments API.env",
            "environments/Acme Payments API.local.env",
        ]
    );
    // The unselected request's folder is not created.
    assert!(
        !dir.path()
            .join("Acme Payments API (2)/customers/{id}")
            .exists()
    );
    for id in report
        .created_files
        .iter()
        .filter(|id| id.ends_with(".postino"))
    {
        workspace
            .load_request(id)
            .unwrap_or_else(|error| panic!("{id} failed to parse back: {error}"));
    }
    let env = std::fs::read_to_string(dir.path().join("environments/Acme Payments API.env"))
        .expect("written");
    assert_eq!(env, "baseUrl=https://api.acme.dev\n");
    let secret =
        std::fs::read_to_string(dir.path().join("environments/Acme Payments API.local.env"))
            .expect("written");
    assert_eq!(secret, "bearerAuth=\n");
}
