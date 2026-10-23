//! SyncEngine：快照推/拉编排（transport 无关）。
//!
//! 依赖方向：miyo-sync 不依赖 miyo-storage。快照的"取/写回"由调用方
//! （miyo-tauri 命令层，持有 db）通过闭包注入，本层只管流程与决策点。

use miyo_backup::BackupFile;

use crate::{RemoteTransport, SnapshotDiff, SyncMessage, TransportError};

/// 拉取预览：远端快照 + 差异 + revision（确认后才执行覆盖恢复）。
#[derive(Debug)]
pub struct PullPreview {
    pub remote_snapshot: BackupFile,
    pub diff: SnapshotDiff,
    pub revision: String,
}

/// 推送结果。
#[derive(Debug)]
pub struct PushResult {
    pub revision: String,
    /// 快照是否与远端一致（无变更幂等推送）。
    pub noop: bool,
}

pub struct SyncEngine<'a> {
    transport: &'a dyn RemoteTransport,
}

impl<'a> SyncEngine<'a> {
    pub fn new(transport: &'a dyn RemoteTransport) -> Self {
        Self { transport }
    }

    /// 推送：快照 → 远端 → 返回新 revision。
    ///
    /// `noop`：远端当前快照与待推快照内容一致（幂等推送，无实际变更）。
    pub async fn push(
        &self,
        snapshot: &BackupFile,
        msg: SyncMessage,
    ) -> Result<PushResult, TransportError> {
        // 远端为空/不可比时视为有变更；仅当远端已有且内容一致才 noop。
        let noop = match self.transport.pull().await {
            Ok((head, _)) => {
                let d = crate::diff_snapshots(Some(&head), snapshot);
                d.is_empty()
            }
            Err(_) => false,
        };
        let revision = self.transport.push(snapshot, &msg).await?;
        Ok(PushResult { revision, noop })
    }

    /// 拉取预览：返回远端快照与差异，不改本地（调用方确认后再恢复）。
    ///
    /// `last_synced`：上次同步的远端 revision（diff 基准，可空）。
    pub async fn preview_pull(
        &self,
        last_synced: Option<&str>,
    ) -> Result<PullPreview, TransportError> {
        let (remote_snapshot, revision) = self.transport.pull().await?;
        let diff = self.transport.diff(last_synced).await?;
        Ok(PullPreview {
            remote_snapshot,
            diff,
            revision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::{sample_snapshot, MemoryTransport};

    /// 内存 transport：借 MemoryTransport 基础上加"远端已落后"场景。
    struct RemoteHasMore {
        inner: MemoryTransport,
    }

    #[async_trait::async_trait]
    impl RemoteTransport for RemoteHasMore {
        async fn push(&self, s: &BackupFile, m: &SyncMessage) -> Result<String, TransportError> {
            self.inner.push(s, m).await
        }
        async fn pull(&self) -> Result<(BackupFile, String), TransportError> {
            self.inner.pull().await
        }
        async fn diff(&self, _since: Option<&str>) -> Result<SnapshotDiff, TransportError> {
            Ok(SnapshotDiff {
                added: vec!["GET /new".into()],
                removed: vec![],
                modified: vec![],
            })
        }
    }

    #[tokio::test]
    async fn push_reports_revision_and_preview_pull_carries_diff() {
        let transport = MemoryTransport::default();
        let engine = SyncEngine::new(&transport);

        // 推送：空远端 diff → 全量新增（非 noop）。
        let snap = sample_snapshot("/ping");
        let result = engine
            .push(
                &snap,
                SyncMessage {
                    summary: "初始".into(),
                },
            )
            .await
            .unwrap();
        assert!(!result.revision.is_empty());
        assert!(!result.noop);

        // 再推同一快照：noop。
        let result = engine
            .push(
                &snap,
                SyncMessage {
                    summary: "无变更".into(),
                },
            )
            .await
            .unwrap();
        assert!(result.noop);

        // 拉取预览：diff 透传。
        let more = RemoteHasMore {
            inner: MemoryTransport::default(),
        };
        more.push(
            &snap,
            &SyncMessage {
                summary: "基线".into(),
            },
        )
        .await
        .unwrap();
        let engine = SyncEngine::new(&more);
        let preview = engine.preview_pull(Some("old-rev")).await.unwrap();
        assert_eq!(preview.diff.added, ["GET /new"]);
        assert_eq!(preview.remote_snapshot.endpoints.len(), 1);
    }
}
