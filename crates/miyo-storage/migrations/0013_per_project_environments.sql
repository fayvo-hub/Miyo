-- 环境回归项目维度：project_id 归属 + 单一 Base URL，移除模块概念。
--
-- 存量全局环境（含模块）按决策「清空并重新初始化」：
-- 1. test_runs 的环境引用置空（环境即将全部删除）；
-- 2. 重建 environments 表（project_id 外键，删项目级联删环境）；
-- 3. 为每个现有项目预置「开发环境 / 测试环境」（基址留空待填）。
--
-- SQLite 无 DROP COLUMN 旧版本兼容路径，用「建新表 + 重命名」重建。
UPDATE test_runs SET environment_id = NULL;

DROP TABLE environments;

CREATE TABLE environments (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL DEFAULT '',
    variables_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_environments_project ON environments(project_id);

-- 重新初始化：每个项目一份「开发环境 / 测试环境」
INSERT INTO environments (id, project_id, name, base_url, variables_json, created_at, updated_at)
SELECT
    lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' ||
    lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' ||
    lower(hex(randomblob(6))),
    p.id,
    e.name,
    '',
    '[]',
    strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM projects p
JOIN (SELECT '开发环境' AS name UNION ALL SELECT '测试环境') e
ORDER BY p.created_at, p.rowid;
