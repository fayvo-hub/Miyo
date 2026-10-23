-- 同步远端从项目维度改为团队维度：
-- 一个团队绑定一个 git 快照仓库路径即可（不同团队可复用同一路径）；
-- 推/拉仍按项目触发，同一仓库内按 projects/{project_id}.json 分文件存放快照。
DROP TABLE IF EXISTS sync_remotes;

CREATE TABLE IF NOT EXISTS sync_remotes (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('git', 's3', 'db')),
    -- RemoteConfig 序列化（含 kind tag，见 fox-sync::RemoteConfig）。
    config_json TEXT NOT NULL,
    -- 上次成功同步的远端 revision（git commit / S3 version / DB 行 id）。
    last_synced_revision TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sync_remotes_team ON sync_remotes(team_id);
