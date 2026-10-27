//! The git commands that change the repository (stage, unstage, discard, init). They run one at
//! a time on the background executor, and the status is read again when each one ends.

use std::path::Path;

use gpui_kit::prelude::*;
use gpui_kit::*;
use postino_git::GitError;

use crate::state::git::RepoState;

use super::super::root::AppView;

/// The `.gitignore` written by "Initialize repository" when the folder has none: the same lines
/// as a new workspace's.
const GITIGNORE: &str = postino_workspace::GITIGNORE;

impl AppView {
    /// Runs `write` on the background executor after the write in flight, if any, then reads the
    /// status again. A failure shows in the error banner with git's own message.
    pub(crate) fn run_git_write(
        &mut self,
        cx: &mut Context<Self>,
        write: impl FnOnce() -> Result<(), GitError> + Send + 'static,
    ) {
        let previous = self.git_tasks.write.take();
        self.git_tasks.write = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = cx.background_spawn(async move { write() }).await;
            let _ = this.update(cx, |view, cx| {
                if let Err(error) = result {
                    log::warn!("git command failed: {error}");
                    view.workspace_error = Some(error.to_string());
                }
                view.refresh_git_status(cx);
                cx.notify();
            });
        }));
    }

    /// 6b: runs `git init` in the workspace folder, adds the standard `.gitignore` when there is
    /// none, then discovers the new repository.
    pub(crate) fn init_git_repository(&mut self, cx: &mut Context<Self>) {
        let (Some(git), Some(workspace)) =
            (self.git.availability.git(), self.state.workspace.as_ref())
        else {
            return;
        };
        if !matches!(self.git.repo, RepoState::NotARepo) {
            return;
        }
        let git = git.clone();
        let root = workspace.root().to_path_buf();
        self.git_tasks.write = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { init_with_gitignore(&git, &root) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(()) => log::info!("initialized a git repository in the workspace"),
                    Err(error) => {
                        log::warn!("git init failed: {error}");
                        view.workspace_error = Some(error);
                    }
                }
                view.discover_git_repo(cx);
                cx.notify();
            });
        }));
    }
}

/// `git init` in `root`, plus a `.gitignore` when the folder has none.
pub(crate) fn init_with_gitignore(git: &postino_git::Git, root: &Path) -> Result<(), String> {
    git.init(root).map_err(|error| error.to_string())?;
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        std::fs::write(&gitignore, GITIGNORE).map_err(|error| error.to_string())?;
    }
    Ok(())
}
