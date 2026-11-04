//! Writing an OpenAPI import plan into a [`Workspace`] (see `docs/openapi-import.md`).
//!
//! `postino_format::openapi` builds the plan from the spec; this writes the part the user
//! selected, the same way a Postman import does: a new collection folder, never overwriting
//! anything that already exists.

use std::fs;

use postino_format::openapi::OpenApiPlan;

use crate::Workspace;
use crate::error::WorkspaceError;
use crate::postman::{ImportReport, unique_path, write_environment, write_folder};
use crate::sanitize::sanitize_file_name;

impl Workspace {
    /// Imports the requests of `plan` that `keep` accepts (by [`PlanRequest::key`]), and the
    /// plan's environments.
    ///
    /// Writes a new top-level folder named after the collection (`plan.root.name`, sanitized,
    /// with a ` (2)`, ` (3)`, ... suffix when the name is taken), with the plan's folders inside;
    /// folders left without a selected request are not created. Each environment goes to
    /// `environments/<name>.env`, its secrets to `environments/<name>.local.env`, also suffixed
    /// when the name is taken. Existing files are never overwritten.
    ///
    /// [`PlanRequest::key`]: postino_format::openapi::PlanRequest::key
    pub fn import_openapi(
        &mut self,
        plan: &OpenApiPlan,
        keep: &dyn Fn(&str) -> bool,
    ) -> Result<ImportReport, WorkspaceError> {
        let folder = plan.root.to_import_folder(keep);
        let mut created = Vec::new();
        let target = unique_path(self.root(), &sanitize_file_name(&plan.root.name));
        fs::create_dir(&target).map_err(|source| WorkspaceError::Io {
            path: target.clone(),
            source,
        })?;
        write_folder(&target, self.root(), &folder, &mut created)?;
        for environment in &plan.environments {
            write_environment(
                self.root(),
                &environment.name,
                &environment.base,
                &environment.secret,
                &mut created,
            )?;
        }
        self.rescan()?;
        Ok(ImportReport {
            created_files: created,
            warnings: plan.warnings.clone(),
        })
    }
}
