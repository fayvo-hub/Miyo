-- 团队：现阶段仅作为项目的容器（后续扩展成员权限）。
CREATE TABLE IF NOT EXISTS teams (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- 项目挂团队：可空（备份 v3 无团队信息），存量数据回填到默认团队。
ALTER TABLE projects ADD COLUMN team_id TEXT REFERENCES teams(id);

-- 固定 id 的默认团队：老项目与新建项目（未指定团队时）都落这里，
-- 代码侧经 fox_core::model::DEFAULT_TEAM_ID 引用，勿改此 id。
INSERT INTO teams (id, name, created_at, updated_at)
VALUES ('00000000-0000-0000-0000-000000000001', '默认团队',
        strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

UPDATE projects SET team_id = '00000000-0000-0000-0000-000000000001' WHERE team_id IS NULL;
