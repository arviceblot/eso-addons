use std::path::Path;
use std::process::{Command, Output, Stdio};

use snafu::{ResultExt, ensure};
use tracing::log::info;

use crate::error::{self, Result};

pub(super) fn is_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git_output(path: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .output()
        .context(error::GitCommandStartSnafu {
            command: args.join(" "),
            path,
        })
}

fn command_error(path: &Path, args: &[&str], output: &Output) -> error::Error {
    error::GitCommandFailedSnafu {
        command: args.join(" "),
        path,
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    }
    .build()
}

fn git_success(path: &Path, args: &[&str]) -> Result<Output> {
    let output = git_output(path, args)?;
    if !output.status.success() {
        return Err(command_error(path, args, &output));
    }
    Ok(output)
}

pub(super) fn pull_repository(path: &Path) -> Result<()> {
    let remotes = git_success(path, &["remote"])?;
    if String::from_utf8_lossy(&remotes.stdout).trim().is_empty() {
        info!("Skipping Git repository without remotes {}", path.display());
        return Ok(());
    }

    let args = ["diff", "--cached", "--quiet", "--ignore-submodules=none"];
    let staged = git_output(path, &args)?;
    ensure!(
        staged.status.code() != Some(1),
        error::GitStagedChangesSnafu { path }
    );
    if !staged.status.success() {
        return Err(command_error(path, &args, &staged));
    }

    git_success(path, &["pull", "--ff-only", "--no-rebase"])?;
    // --ff-only succeeds when HEAD is ahead and the remote has not changed.
    let ahead = git_success(path, &["rev-list", "--count", "@{upstream}..HEAD"])?;
    ensure!(
        String::from_utf8_lossy(&ahead.stdout).trim() == "0",
        error::GitLocalCommitsSnafu { path }
    );
    info!("Pulled Git repository {}", path.display());
    Ok(())
}

/// Get the URL of the default remote (origin).
pub(super) fn get_remote_url(path: &Path) -> Result<String> {
    let output = git_success(path, &["remote", "get-url", "origin"])?;
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err(error::GitCommandFailedSnafu {
            command: "remote get-url origin".to_string(),
            path: path.to_path_buf(),
            status: std::process::ExitStatus::default(),
            stderr: "Empty remote URL".to_string(),
        }
        .build());
    }
    Ok(url)
}
