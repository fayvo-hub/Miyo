//! 迁移升级回归：旧库中接口的遗留 `timeout_ms: 30000`（d9e2a65 之前的硬编码
//! 默认）在升级后被清除，回到「跟随全局设置」；其它自定义超时值不动。
//!
//! 价值：14 个存量接口残留 30000 会覆盖「设置 → 请求超时」的 300s，前端按 30s
//! 提前取消并提示「请求超时（>30000ms）」，与全局设置不符。`0019` 用
//! `json_remove` 清掉该值。若迁移写错（如误伤其它值 / JSON 路径拼错导致无效
//! JSON），真实用户升级即出问题——本测试用真库文件走完整 `init_db` 链路。

use sqlx::Row;

use miyo_storage::db;

/// 独立临时目录（路径带 UUID，避免并发用例互相踩）。
fn temp_db(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "miyo-migrate-timeout-{tag}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.db");
    (dir, path)
}

/// 预置项目 + 三条接口：旧默认 30000、用户自定义 1500、无超时字段。
async fn seed_legacy(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO projects (id, name, description, created_at, updated_at)
         VALUES ('p-1','旧项目','','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
    )
    .execute(pool)
    .await
    .unwrap();

    let rows = [
        (
            "ep-legacy",
            r#"{"timeout_ms":30000,"follow_redirects":true}"#,
        ),
        (
            "ep-custom",
            r#"{"timeout_ms":1500,"follow_redirects":true}"#,
        ),
        ("ep-none", r#"{"follow_redirects":true}"#),
    ];
    for (id, spec) in rows {
        sqlx::query(
            "INSERT INTO endpoints
               (id, project_id, name, method, path, request_json, created_at, updated_at)
             VALUES (?, 'p-1', ?, 'GET', '/x', ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(id)
        .bind(id)
        .bind(spec)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// 读取接口的 request_json。
async fn spec_of(pool: &sqlx::SqlitePool, id: &str) -> serde_json::Value {
    let row = sqlx::query("SELECT request_json FROM endpoints WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    serde_json::from_str(&row.get::<String, _>("request_json")).unwrap()
}

#[tokio::test]
async fn upgrade_clears_legacy_30000_keeps_custom() {
    let (dir, path) = temp_db("upgrade");

    // 1) 造旧库：建表 + 预置数据（此时还没有 _sqlx_migrations）
    {
        let pool = db::connect(&path).await.unwrap();
        sqlx::query(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
                variables_json TEXT NOT NULL DEFAULT '{}', created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE endpoints (
                id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                folder_id TEXT NULL, name TEXT NOT NULL, method TEXT NOT NULL, path TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'developing',
                sort_order INTEGER NOT NULL DEFAULT 0, request_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        seed_legacy(&pool).await;
    }

    // 2) 真实升级：init_db 跑全量迁移 + integrity_check
    let pool = db::init_db(&path).await.unwrap();

    // 3) 遗留 30000 被清除 → 回到跟随全局
    let legacy = spec_of(&pool, "ep-legacy").await;
    assert!(
        legacy.get("timeout_ms").is_none(),
        "遗留 30000 应被清掉（回到跟随全局），实际：{legacy}"
    );
    // 其它字段必须保留（json_remove 只删一个键，不能整段重写）
    assert_eq!(legacy["follow_redirects"], serde_json::json!(true));

    // 4) 非 30000 的自定义值不动
    assert_eq!(
        spec_of(&pool, "ep-custom").await["timeout_ms"],
        serde_json::json!(1500)
    );
    // 5) 本就没有该键的接口不受影响
    assert!(spec_of(&pool, "ep-none").await.get("timeout_ms").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}
