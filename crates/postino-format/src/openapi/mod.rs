//! Importing OpenAPI 3.1, 3.0 and Swagger 2.0 specs (see `docs/openapi-import.md`).
//!
//! Pure like the rest of `postino-format`: [`parse_spec`] reads spec text (JSON or YAML) into a
//! normalized [`Spec`], loading any external `$ref` through the caller's [`RefLoader`], and
//! [`build_plan`] turns one or more specs and the [`ImportOptions`] into an [`OpenApiPlan`].
//! `postino-workspace` writes the plan into a workspace.

mod document;
mod error;
mod example;
mod generate;
mod spec;

pub use document::{NoExternalRefs, RefLoader};
pub use error::OpenApiError;
pub use generate::{
    BASE_URL_VARIABLE, Grouping, ImportOptions, OpenApiPlan, PlanEnvironment, PlanFolder,
    PlanRequest, SpecVersion, build_plan, operation_key, used_security_schemes,
};
pub use spec::{
    Operation, Parameter, ParameterLocation, RequestBody, SchemeKind, SecurityScheme, Server, Spec,
    Success, parse_spec,
};
