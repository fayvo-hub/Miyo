//! 数据库连接、路径与迁移。

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use miyo_core::{AppError, Result};

/// {SystemDataDir}/Miyo（开发构建用 Miyo-dev，与正式版数据隔离：
/// 避免 tauri dev 跑过更新的迁移后，旧正式版打开同一数据库因迁移版本
/// 校验失败而启动即退出）。
/// 可覆盖：`MIYO_DATA_DIR` 环境变量 > 默认目录下的 `data_dir.txt`
/// （设置页写入）> 默认。见 `miyo_core::paths`。
pub fn data_dir() -> PathBuf {
    miyo_core::paths::resolve_data_dir_with_default(default_data_dir())
}

/// 默认数据目录（无覆盖时；见 `miyo_core::paths::default_data_dir`）。
pub fn default_data_dir() -> PathBuf {
    miyo_core::paths::default_data_dir()
}

/// bootstrap 文件路径（设置页写入的覆盖配置所在，见 `miyo_core::paths`）。
pub fn bootstrap_path() -> PathBuf {
    miyo_core::paths::bootstrap_path()
}

/// 日志目录 {SystemDataDir}/Miyo/logs
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// 数据库文件路径。
pub fn database_path() -> PathBuf {
    data_dir().join("miyo.db")
}

/// 开发构建（debug）启动前删除数据库文件（含 WAL / SHM）：迁移重建后由
/// `seed::seed_dev_data` 写入一致的测试数据集，`npm run tauri dev`
/// 每次重启都从干净数据开始。release 构建为空操作，不影响正式数据。
/// 品牌更名的目录迁移会把旧 `rustfox.db` 带进新目录，旧名文件也必须清理，
/// 否则随后被 [`migrate_legacy_db_file`] 改名回来，dev 不再是干净启动。
pub fn reset_dev_database() {
    if !cfg!(debug_assertions) {
        return;
    }
    reset_database_files(&database_path());
}

/// [`reset_dev_database`] 的可注入实现：删除当前名与旧品牌名的
/// `*.db` / `*-wal` / `*-shm`。
fn reset_database_files(path: &Path) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let Some(stem) = name.strip_suffix(".db") else {
        return;
    };
    let mut stems = vec![stem.to_string()];
    let legacy = stem.replacen("miyo", "rustfox", 1);
    if legacy != stem {
        stems.push(legacy);
    }
    for stem in stems {
        for suffix in ["", "-wal", "-shm"] {
            let p = path.with_file_name(format!("{stem}.db{suffix}"));
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// 快照保留目录（数据目录下 `snapshots/`）。
pub fn snapshot_dir() -> PathBuf {
    data_dir().join("snapshots")
}

/// 快照保留份数（超出按最旧删除）。
pub const SNAPSHOT_RETENTION: usize = 5;

/// 建立连接并执行迁移。
///
/// - 库文件名品牌更名兼容：同目录 `rustfox.db`（含 WAL/SHM）先改名为
///   `miyo.db`（[`migrate_legacy_db_file`]，失败仅告警）；
/// - 迁移前：已存在库文件先 WAL checkpoint + 全量快照（`snapshots/` 保留 5 份），
///   升级失败可回滚，之前是"裸跑迁移，坏了只能靠 backups/ JSON 重建"；
/// - 迁移后：`PRAGMA integrity_check` 自检，非 ok 直接报错并指引恢复路径。
pub async fn init_db(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    migrate_legacy_db_file(path);
    let fresh = !path.exists();
    let pool = connect(path).await?;
    if !fresh {
        snapshot_database(&pool, path).await?;
    }
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| AppError::Database(sqlx::Error::Protocol(e.to_string())))?;
    integrity_check(&pool, path).await?;
    Ok(pool)
}

/// 库文件名品牌更名兼容（RustFox → Miyo）：目标 `miyo.db` 不存在且同目录
/// 存在 `rustfox.db` 时连 WAL/SHM 一起改名。必须在打开连接前调用。
/// 任一步失败仅告警不阻断——失败时 SQLite 会新建空库，旧文件原样留存
/// 供用户手动恢复，不会丢数据。
fn migrate_legacy_db_file(path: &Path) {
    if path.exists() {
        return;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let Some(stem) = name.strip_suffix(".db") else {
        return;
    };
    let legacy_stem = stem.replacen("miyo", "rustfox", 1);
    if legacy_stem == stem {
        return;
    }
    for suffix in ["", "-wal", "-shm"] {
        let from = path.with_file_name(format!("{legacy_stem}.db{suffix}"));
        if !from.exists() {
            continue;
        }
        let to = path.with_file_name(format!("{stem}.db{suffix}"));
        if let Err(e) = std::fs::rename(&from, &to) {
            tracing::warn!("旧库文件 {from:?} 改名失败（将继续新建空库）：{e}");
        } else {
            tracing::info!("旧库文件已迁移：{from:?} → {to:?}");
        }
    }
}

/// 迁移前快照：checkpoint 落盘 + 复制库文件 + 修剪旧快照（失败仅告警，不阻断启动）。
async fn snapshot_database(pool: &SqlitePool, path: &Path) -> Result<()> {
    // data_dir() 在测试/正式环境下不同：快照落在库文件同级 `snapshots/`，
    // 避免测试污染正式目录、也避免多环境互相覆盖。
    let dir = path
        .parent()
        .map(|p| p.join("snapshots"))
        .unwrap_or_else(snapshot_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("创建快照目录失败（跳过快照）：{e}");
        return Ok(());
    }
    // 同步 checkpoint，保证快照文件自包含（WAL 内容并回主库）。
    let _: Option<(i32, i32, i32)> = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_optional(pool)
        .await
        .unwrap_or(None);
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dest = dir.join(format!("miyo-{stamp}.db"));
    if let Err(e) = std::fs::copy(path, &dest) {
        tracing::warn!("迁移前快照失败（继续启动）：{e}");
        return Ok(());
    }
    // 修剪：仅保留最近 SNAPSHOT_RETENTION 份。
    let mut snaps: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        // 更名前的旧快照（rustfox- 前缀）一并纳入修剪，避免无限增长
                        (n.starts_with("miyo-") || n.starts_with("rustfox-")) && n.ends_with(".db")
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    snaps.sort();
    while snaps.len() > SNAPSHOT_RETENTION {
        let Some(old) = snaps.first().cloned() else {
            break;
        };
        if let Err(e) = std::fs::remove_file(&old) {
            tracing::warn!("清理旧快照失败：{e}");
            break;
        }
        snaps.remove(0);
    }
    tracing::info!("迁移前快照已保存：{}", dest.display());
    Ok(())
}

/// 迁移后自检：`PRAGMA integrity_check` 非 ok 即报错并指引恢复。
async fn integrity_check(pool: &SqlitePool, path: &Path) -> Result<()> {
    let row: (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?;
    if row.0.eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    Err(AppError::Database(sqlx::Error::Protocol(format!(
        "数据库完整性检查失败（{}）：请用设置页备份 JSON 恢复，或从 snapshots/ 复制快照回 {} 后重启",
        row.0.lines().next().unwrap_or(&row.0),
        path.display()
    ))))
}

/// 只建连接，不跑迁移（测试用）。
pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        // WAL 下的推荐档位：崩溃不丢事务，且写入时不再每笔 fsync（性能差异可达数倍）
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    Ok(pool)
}

/// 内存数据库（测试用）。
pub async fn memory_pool() -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(":memory:")
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|e| AppError::Database(sqlx::Error::Protocol(e.to_string())))?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 二次启动：已存在库 → 快照 1 份 + integrity_check 通过。
    #[tokio::test]
    async fn init_db_snapshots_and_self_checks() {
        let dir = std::env::temp_dir().join(format!("miyo-db-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("miyo.db");
        let pool = init_db(&path).await.expect("首次建库");
        pool.close().await;
        let pool = init_db(&path).await.expect("二次启动应快照+自检通过");
        pool.close().await;
        let snaps: Vec<_> = std::fs::read_dir(dir.join("snapshots"))
            .expect("快照目录")
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(snaps.len(), 1, "应产生 1 份快照");
        assert!(snaps[0]
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with("miyo-") && n.ends_with(".db")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 品牌更名兼容：同目录旧 `rustfox.db`（含 WAL/SHM）改名为主库文件名。
    #[test]
    fn legacy_db_file_renamed_with_wal() {
        let dir = std::env::temp_dir().join(format!("miyo-db-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("miyo.db");
        std::fs::write(dir.join("rustfox.db"), b"old-main").unwrap();
        std::fs::write(dir.join("rustfox.db-wal"), b"old-wal").unwrap();
        migrate_legacy_db_file(&path);
        assert_eq!(std::fs::read(&path).unwrap(), b"old-main", "主库文件已迁入");
        assert_eq!(std::fs::read(dir.join("miyo.db-wal")).unwrap(), b"old-wal");
        assert!(!dir.join("rustfox.db").exists());
        // 目标已存在：空操作，旧文件原样保留
        std::fs::write(dir.join("rustfox.db"), b"leftover").unwrap();
        migrate_legacy_db_file(&path);
        assert_eq!(std::fs::read(&path).unwrap(), b"old-main");
        assert_eq!(std::fs::read(dir.join("rustfox.db")).unwrap(), b"leftover");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// dev 重置：当前名与旧品牌名的库文件（含 WAL/SHM）都清掉，别的文件不动。
    #[test]
    fn dev_reset_clears_legacy_named_files_too() {
        let dir = std::env::temp_dir().join(format!("miyo-db-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("miyo.db");
        for name in [
            "miyo.db",
            "miyo.db-wal",
            "rustfox.db",
            "rustfox.db-shm",
            "keep.txt",
        ] {
            std::fs::write(dir.join(name), b"x").expect("写占位文件");
        }
        reset_database_files(&path);
        for name in ["miyo.db", "miyo.db-wal", "rustfox.db", "rustfox.db-shm"] {
            assert!(!dir.join(name).exists(), "{name} 应被清理");
        }
        assert!(dir.join("keep.txt").exists(), "无关文件不应被误删");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
