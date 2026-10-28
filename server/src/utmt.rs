//! Runs UndertaleModCli (UTMT 0.9.2.0+) as a child process.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::chapters::Chapter;
use crate::jobs::Jobs;
use crate::store::ChapterStore;

pub const EXPORT_SCRIPT: &str = include_str!("../utmt-scripts/export_all.csx");

#[derive(Debug, Clone)]
pub struct Utmt {
    pub cli: PathBuf,
}

impl Utmt {
    /// Runs the CLI and sends every output line to the job log.
    async fn run(&self, jobs: &Jobs, job: u64, cwd: &Path, args: &[&std::ffi::OsStr], env: &[(&str, &Path)]) -> Result<(), String> {
        jobs.log(
            job,
            format!(
                "$ {} {}",
                self.cli.display(),
                args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
            ),
        );
        let mut cmd = Command::new(&self.cli);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot start UndertaleModCli at '{}': {e}", self.cli.display()))?;
        let pipe = |r: Box<dyn tokio::io::AsyncRead + Unpin + Send>, jobs: Jobs| {
            tokio::spawn(async move {
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    jobs.log(job, line);
                }
            })
        };
        let out = pipe(Box::new(child.stdout.take().unwrap()), jobs.clone());
        let err = pipe(Box::new(child.stderr.take().unwrap()), jobs.clone());
        let status = child.wait().await.map_err(|e| e.to_string())?;
        let _ = tokio::join!(out, err);
        if status.success() {
            Ok(())
        } else {
            Err(format!("UndertaleModCli exited with {status}"))
        }
    }

    /// Exports the chapter into `base/` (UTMT project) and `generic/` (all lists). Keeps `mod/`.
    pub async fn extract(&self, jobs: &Jobs, job: u64, chapter: &Chapter, store: &ChapterStore) -> Result<(), String> {
        let io = |e: std::io::Error| e.to_string();
        tokio::fs::create_dir_all(&store.root).await.map_err(io)?;
        let base_tmp = store.root.join("base.tmp");
        let generic_tmp = store.root.join("generic.tmp");
        for d in [&base_tmp, &generic_tmp] {
            if d.exists() {
                tokio::fs::remove_dir_all(d).await.map_err(io)?;
            }
        }
        let script = store.root.join("export_all.csx");
        tokio::fs::write(&script, EXPORT_SCRIPT).await.map_err(io)?;

        let args = [
            std::ffi::OsStr::new("load"),
            chapter.data_file.as_os_str(),
            "-s".as_ref(),
            script.as_os_str(),
        ];
        self.run(
            jobs,
            job,
            &store.root,
            &args,
            &[("DRE_PROJECT_DIR", &base_tmp), ("DRE_GENERIC_DIR", &generic_tmp)],
        )
        .await?;
        if !base_tmp.join("project.json").is_file() || !generic_tmp.join("_index.json").is_file() {
            return Err("export finished, but output is missing".into());
        }

        for (tmp, dst) in [(base_tmp, store.base()), (generic_tmp, store.generic())] {
            replace_dir(&tmp, &dst).await.map_err(io)?;
        }
        let meta = std::fs::metadata(&chapter.data_file).map_err(io)?;
        let info = serde_json::json!({
            "data_file": chapter.data_file,
            "data_file_size": meta.len(),
            "data_file_modified": meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            "extracted_at": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        });
        tokio::fs::write(store.root.join("meta.json"), serde_json::to_vec_pretty(&info).unwrap())
            .await
            .map_err(io)?;
        jobs.log(job, "extract: done");
        Ok(())
    }

    /// Applies `mod/` to the original data file. Output: `build/<data file name>`.
    pub async fn build(&self, jobs: &Jobs, job: u64, chapter: &Chapter, store: &ChapterStore) -> Result<PathBuf, String> {
        store.ensure_mod_project().map_err(|e| e.to_string())?;
        let out_dir = store.build_dir();
        tokio::fs::create_dir_all(&out_dir).await.map_err(|e| e.to_string())?;
        let out = out_dir.join(chapter.data_file.file_name().unwrap());
        let project = store.mod_dir().join("project.json");
        let args = [
            std::ffi::OsStr::new("project"),
            "build".as_ref(),
            project.as_os_str(),
            "-s".as_ref(),
            chapter.data_file.as_os_str(),
            "-d".as_ref(),
            out.as_os_str(),
        ];
        self.run(jobs, job, &store.root, &args, &[]).await?;
        if !out.is_file() {
            return Err("build finished, but output is missing".into());
        }
        jobs.log(job, format!("build: wrote {}", out.display()));
        Ok(out)
    }
}

/// Puts `new` at `dst`. The old `dst` is moved aside first and only removed after the new one is in place,
/// so a failure keeps the old export.
async fn replace_dir(new: &Path, dst: &Path) -> std::io::Result<()> {
    let mut old = dst.as_os_str().to_owned();
    old.push(".old");
    let old = PathBuf::from(old);
    if old.exists() {
        tokio::fs::remove_dir_all(&old).await?;
    }
    let had_old = dst.exists();
    if had_old {
        tokio::fs::rename(dst, &old).await?;
    }
    if let Err(e) = tokio::fs::rename(new, dst).await {
        if had_old {
            let _ = tokio::fs::rename(&old, dst).await;
        }
        return Err(e);
    }
    if had_old {
        tokio::fs::remove_dir_all(&old).await?;
    }
    Ok(())
}

/// Folder of the running server executable.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Finds UndertaleModCli: the bundled copy (`tools/UndertaleModCli/` next to the server), then ./, ./tools/, `PATH`.
pub fn locate_cli() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "UndertaleModCli.exe" } else { "UndertaleModCli" };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = exe_dir() {
        dirs.push(dir.join("tools/UndertaleModCli"));
        dirs.push(dir.join("tools"));
        dirs.push(dir);
    }
    dirs.extend([PathBuf::from("tools/UndertaleModCli"), PathBuf::from("tools"), PathBuf::from(".")]);
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replace_dir_keeps_old_on_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("base");
        std::fs::create_dir(&dst).unwrap();
        std::fs::write(dst.join("project.json"), "old").unwrap();
        // The new dir is missing: the rename fails, and the old export stays.
        assert!(replace_dir(&tmp.path().join("missing"), &dst).await.is_err());
        assert_eq!(std::fs::read_to_string(dst.join("project.json")).unwrap(), "old");
        let new = tmp.path().join("base.tmp");
        std::fs::create_dir(&new).unwrap();
        std::fs::write(new.join("project.json"), "new").unwrap();
        replace_dir(&new, &dst).await.unwrap();
        assert_eq!(std::fs::read_to_string(dst.join("project.json")).unwrap(), "new");
        assert!(!tmp.path().join("base.old").exists() && !new.exists());
    }
}
