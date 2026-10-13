//! git 远端传输：快照作为仓库内 JSON 文件，git CLI 负责版本与传输。
//!
//! 布局：`{workdir}/rustfox/{project_id}.json`，一次 push 一个 commit。
//! 依赖系统 git（PATH 可用）；凭证走 git 自身机制（ssh key / credential helper）。

use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use fox_backup::BackupFile;

use crate::{diff_snapshots, RemoteTransport, Revision, SnapshotDiff, SyncMessage, TransportError};

/// git 传输：workdir 为本地缓存的仓库克隆（惰性 clone）。
pub struct GitTransport {
    url: String,
    branch: String,
    /// 快照文件相对路径（如 `projects/{project_id}.json`）。
    path: String,
    /// 本地缓存仓库目录。
    workdir: PathBuf,
}

impl GitTransport {
    pub fn new(
        url: impl Into<String>,
        branch: impl Into<String>,
        path: impl Into<String>,
        workdir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            url: url.into(),
            branch: branch.into(),
            path: path.into(),
            workdir: workdir.into(),
        }
    }

    /// 确保 workdir 是目标仓库的最新克隆（clone 或 pull）。
    async fn ensure_repo(&self) -> Result<(), TransportError> {
        let url = self.url.clone();
        let branch = self.branch.clone();
        let workdir = self.workdir.clone();
        tokio::task::spawn_blocking(move || -> Result<(), TransportError> {
            if workdir.join(".git").is_dir() {
                run(Command::new("git")
                    .arg("-C")
                    .arg(&workdir)
                    .arg("fetch")
                    .arg("origin")
                    .arg("--prune"))?;
                checkout_branch(&workdir, &branch)?;
            } else {
                if let Some(parent) = workdir.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                // 不带 --branch：远端可能尚无任何提交（空仓库首次 push）。
                run(Command::new("git").arg("clone").arg(&url).arg(&workdir))?;
                // 远端已有目标分支则切过去，否则建本地分支（首推时）。
                checkout_branch(&workdir, &branch)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| TransportError::Remote(format!("git 任务异常：{e}")))?
    }

    /// 读取远端指定 ref 的快照文件内容。
    async fn read_at(&self, rev: &str) -> Result<Option<String>, TransportError> {
        let workdir = self.workdir.clone();
        let path = self.path.clone();
        let rev = rev.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>, TransportError> {
            let out = Command::new("git")
                .arg("-C")
                .arg(&workdir)
                .arg("show")
                .arg(format!("{rev}:{path}"))
                .output()?;
            if !out.status.success() {
                // 文件在该 revision 不存在 → 视为空。
                return Ok(None);
            }
            Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
        })
        .await
        .map_err(|e| TransportError::Remote(format!("git 任务异常：{e}")))?
    }

    /// 读取 workdir 当前 HEAD 的快照文件。
    fn read_worktree(&self) -> Result<Option<String>, TransportError> {
        let file = self.workdir.join(&self.path);
        if !file.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(&file).map_err(|e| TransportError::Remote(e.to_string()))?;
        Ok(Some(text))
    }
}

/// 切到 `branch`：远端已有该分支则跟踪 `origin/{branch}`（并 hard reset 对齐），
/// 否则建本地分支（空仓库/远端尚无该分支时的首推场景）。
fn checkout_branch(workdir: &Path, branch: &str) -> Result<(), TransportError> {
    let has_branch = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .arg("rev-parse")
        .arg("--verify")
        .arg(format!("refs/remotes/origin/{branch}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if has_branch {
        let start = format!("origin/{branch}");
        run(Command::new("git")
            .arg("-C")
            .arg(workdir)
            .arg("checkout")
            .arg("-B")
            .arg(branch)
            .arg(&start))?;
        run(Command::new("git")
            .arg("-C")
            .arg(workdir)
            .arg("reset")
            .arg("--hard")
            .arg(&start))?;
    } else {
        run(Command::new("git")
            .arg("-C")
            .arg(workdir)
            .arg("checkout")
            .arg("-B")
            .arg(branch))?;
    }
    Ok(())
}

fn run(cmd: &mut Command) -> Result<(), TransportError> {
    // 非交互：http(s) 远端触发凭据询问时直接失败（避免等待输入挂死）。
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_ASKPASS", "");
    let out = cmd
        .output()
        .map_err(|e| TransportError::Remote(format!("无法执行 git（确认已安装且在 PATH）：{e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(TransportError::Remote(format!(
            "git 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn parse_snapshot(text: &str) -> Result<BackupFile, TransportError> {
    let file = BackupFile::parse(text).map_err(|e| TransportError::Remote(e.to_string()))?;
    if file.schema_version > fox_backup::SCHEMA_VERSION {
        return Err(TransportError::SchemaMismatch {
            remote: file.schema_version,
            local: fox_backup::SCHEMA_VERSION,
        });
    }
    Ok(file)
}

#[async_trait]
impl RemoteTransport for GitTransport {
    async fn push(
        &self,
        snapshot: &BackupFile,
        msg: &SyncMessage,
    ) -> Result<Revision, TransportError> {
        self.ensure_repo().await?;
        let file_path = self.workdir.join(&self.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| TransportError::Remote(e.to_string()))?;
        }
        let json = snapshot
            .serialize()
            .map_err(|e| TransportError::Remote(e.to_string()))?;
        std::fs::write(&file_path, json).map_err(|e| TransportError::Remote(e.to_string()))?;

        let workdir = self.workdir.clone();
        let path = self.path.clone();
        let summary = msg.summary.clone();
        let branch = self.branch.clone();
        tokio::task::spawn_blocking(move || -> Result<Revision, TransportError> {
            run(Command::new("git")
                .arg("-C")
                .arg(&workdir)
                .arg("add")
                .arg(&path))?;
            // 无变更时 commit 会失败，视为幂等成功。
            let status = Command::new("git")
                .arg("-C")
                .arg(&workdir)
                .arg("commit")
                .arg("-m")
                .arg(&summary)
                .status()
                .map_err(|e| TransportError::Remote(e.to_string()))?;
            if !status.success() {
                let head = head_rev(&workdir)?;
                return Ok(head);
            }
            // -u：空仓库首推时建立 upstream 跟踪。
            run(Command::new("git")
                .arg("-C")
                .arg(&workdir)
                .arg("push")
                .arg("-u")
                .arg("origin")
                .arg(format!("HEAD:refs/heads/{branch}")))?;
            head_rev(&workdir)
        })
        .await
        .map_err(|e| TransportError::Remote(format!("git 任务异常：{e}")))?
    }

    async fn pull(&self) -> Result<(BackupFile, Revision), TransportError> {
        self.ensure_repo().await?;
        let rev = {
            let workdir = self.workdir.clone();
            tokio::task::spawn_blocking(move || head_rev(&workdir))
                .await
                .map_err(|e| TransportError::Remote(format!("git 任务异常：{e}")))??
        };
        let text = self
            .read_worktree()?
            .ok_or_else(|| TransportError::Remote("远端暂无快照".into()))?;
        let file = parse_snapshot(&text)?;
        Ok((file, rev))
    }

    async fn diff(&self, since: Option<&str>) -> Result<SnapshotDiff, TransportError> {
        self.ensure_repo().await?;
        let head_text = self.read_worktree()?;
        let head = head_text.as_deref().map(parse_snapshot).transpose()?;
        let old = match since {
            Some(rev) => self
                .read_at(rev)
                .await?
                .as_deref()
                .map(parse_snapshot)
                .transpose()?,
            None => None,
        };
        match head {
            Some(h) => Ok(diff_snapshots(old.as_ref(), &h)),
            None => Ok(SnapshotDiff::default()),
        }
    }

    /// 列出仓库内快照目录下的全部项目快照（导入入口：发现远端新增项目）。
    async fn list_projects(&self) -> Result<Vec<crate::RemoteProjectEntry>, TransportError> {
        self.ensure_repo().await?;
        // path 形如 `projects/{id}.json`：取其父目录扫描。
        let dir = Path::new(&self.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let dir = self.workdir.join(dir);
        let mut entries = Vec::new();
        if !dir.is_dir() {
            return Ok(entries);
        }
        for f in std::fs::read_dir(&dir)? {
            let file = f?.path();
            if file.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            // 损坏 / 他人工具产物：跳过不阻塞整体列表。
            let Ok(snap) = BackupFile::parse(&text) else {
                continue;
            };
            entries.push(crate::RemoteProjectEntry {
                project_id: snap.project.id.to_string(),
                name: snap.project.name.clone(),
                endpoint_count: snap.endpoints.len(),
                updated_at: snap.exported_at.clone(),
            });
        }
        entries.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        Ok(entries)
    }
}

fn head_rev(workdir: &Path) -> Result<Revision, TransportError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()?;
    if !out.status.success() {
        return Err(TransportError::Remote("无法读取 HEAD".into()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::*;

    /// 端到端：裸仓库当远端，push → 第二个 transport pull → diff。
    #[tokio::test]
    async fn push_pull_roundtrip_via_bare_repo() {
        // CI 环境无全局 git 身份：commit 会因缺 user.name/email 失败，测试内兜底。
        std::env::set_var("GIT_AUTHOR_NAME", "RustFox CI");
        std::env::set_var("GIT_AUTHOR_EMAIL", "ci@rustfox.local");
        std::env::set_var("GIT_COMMITTER_NAME", "RustFox CI");
        std::env::set_var("GIT_COMMITTER_EMAIL", "ci@rustfox.local");

        let tmp = std::env::temp_dir().join(format!("fox-sync-test-{}", uuid::Uuid::new_v4()));
        let bare = tmp.join("remote.git");
        // 建裸仓库 + 初始化默认分支。
        std::fs::create_dir_all(&bare).unwrap();
        run(Command::new("git")
            .arg("-C")
            .arg(&bare)
            .arg("init")
            .arg("--bare")
            .arg("-b")
            .arg("main"))
        .unwrap();

        let snapshot = sample_snapshot("GET /ping");
        let t1 = GitTransport::new(
            bare.to_str().unwrap(),
            "main",
            "projects/p1.json",
            tmp.clone().join("work1"),
        );
        let rev1 = t1
            .push(
                &snapshot,
                &SyncMessage {
                    summary: "初始快照".into(),
                },
            )
            .await
            .unwrap();
        assert!(!rev1.is_empty());

        // 另一台"机器"：独立 workdir，pull 得到同一快照。
        let t2 = GitTransport::new(
            bare.to_str().unwrap(),
            "main",
            "projects/p1.json",
            tmp.clone().join("work2"),
        );
        let (pulled, rev) = t2.pull().await.unwrap();
        assert_eq!(pulled.project.name, snapshot.project.name);
        assert_eq!(pulled.endpoints.len(), 1);

        // 无新提交时 diff 为空。
        let d = t2.diff(Some(&rev)).await.unwrap();
        assert!(d.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
