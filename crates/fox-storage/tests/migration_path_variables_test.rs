//! 迁移升级回归：旧库（已含 test_cases 行、无 path_variables 列）升级后，
//! 存量行保留且新列取默认 `'[]'`，新列可正常读写。
//!
//! 价值：`TestCase` 新增字段靠 `0018_test_case_path_variables.sql` 的
//! `ALTER TABLE ... ADD COLUMN ... DEFAULT '[]'` 落地。若迁移写错（如 NOT NULL
//! 无默认值），真实用户升级启动即失败——本测试用真库文件走完整 `init_db` 链路。

use sqlx::Row;
use uuid::Uuid;

use fox_storage::db;

/// 旧库 schema：模拟 0005 建表时的 test_cases（无 path_variables）。
const OLD_TEST_CASES: &str = "CREATE TABLE test_cases (
    id TEXT PRIMARY KEY, request_id TEXT NOT NULL, name TEXT NOT NULL,
    category TEXT NOT NULL, method TEXT NOT NULL, url_path TEXT NOT NULL,
    params TEXT, headers TEXT, body_type TEXT NOT NULL, body_content TEXT NOT NULL,
    last_run_status TEXT NOT NULL, created_at TEXT NOT NULL)";

/// 独立临时目录（路径带 UUID，避免并发用例互相踩）。
fn temp_db(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("rustfox-migrate-{tag}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("legacy.db");
    (dir, path)
}

#[tokio::test]
async fn upgrade_old_db_keeps_rows_and_adds_path_variables() {
    let (dir, path) = temp_db("upgrade");

    // 1) 造旧库：建旧表 + 预置一条老用例（此时还没有 _sqlx_migrations）
    {
        let pool = db::connect(&path).await.unwrap();
        sqlx::query(OLD_TEST_CASES).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO test_cases
               (id, request_id, name, category, method, url_path, params, headers,
                body_type, body_content, last_run_status, created_at)
             VALUES ('c-old','ep-1','旧用例','正向','GET','/users/{id}','[]','[]',
                     'none','','Untested','2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
    }

    // 2) 真实升级：init_db 跑全量迁移 + integrity_check
    let pool = db::init_db(&path).await.unwrap();

    // 3) 存量行还在，且新列取默认 '[]'
    let row = sqlx::query("SELECT name, path_variables FROM test_cases WHERE id = 'c-old'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("name"), "旧用例");
    assert_eq!(
        row.get::<String, _>("path_variables"),
        "[]",
        "存量用例应补默认 '[]'（不能升级失败）"
    );

    // 4) 新列可写可读（仓库层实际用的就是这两步）
    sqlx::query("UPDATE test_cases SET path_variables = ? WHERE id = 'c-old'")
        .bind(r#"[{"key":"id","value":"42","enabled":true,"description":""}]"#)
        .execute(&pool)
        .await
        .unwrap();
    let back = sqlx::query("SELECT path_variables FROM test_cases WHERE id = 'c-old'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(back.get::<String, _>("path_variables").contains("42"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 全新库（无历史数据）直接建到最新 schema：列存在即可（写入受 endpoints 外键约束，
/// 真实写入路径已由 repository_test / 上面那支覆盖）。
#[tokio::test]
async fn fresh_db_has_path_variables_column() {
    let (dir, path) = temp_db("fresh");
    let pool = db::init_db(&path).await.unwrap();
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(test_cases)")
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    assert!(cols.contains(&"path_variables".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}
