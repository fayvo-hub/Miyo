-- 项目同步远端：项目绑定 git / S3 / db 快照仓库。
CREATE TABLE IF NOT EXISTS sync_remotes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('git', 's3', 'db')),
    -- RemoteConfig 序列化（含 kind tag，见 fox-sync::RemoteConfig）。
    config_json TEXT NOT NULL,
    -- 上次成功同步的远端 revision（git commit / S3 version / DB 行 id）。
    last_synced_revision TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sync_remotes_project ON sync_remotes(project_id);
