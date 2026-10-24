//! 端到端集成测试：本地 axum mock + 临时集合文件 → run_collection / junit 断言。
//!
//! 覆盖：通过 / 失败断言 / 无 Base URL 跳过 / 名称过滤 / 变量注入 / JUnit 内容。

use std::path::PathBuf;

use axum::routing::get;
use miyo_cli::{junit, load_backup, run_collection, CaseKind, CaseOutcome, RunOptions};

/// 起本地 mock：/users 返回 200 JSON；/bad 返回 500；/echo/:id 回显。
async fn spawn_server() -> String {
    let app =
        axum::Router::new()
            .route(
                "/users",
                get(|| async {
                    axum::Json(serde_json::json!({ "data": [{ "id": 1, "name": "miyo" }] }))
                }),
            )
            .route(
                "/bad",
                get(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
            )
            .route(
                "/echo/{id}",
                get(
                    |axum::extract::Path(id): axum::extract::Path<String>| async move {
                        format!("id={id}")
                    },
                ),
            );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// 生成最小集合备份 JSON：三个接口（通过 / 失败 / 无测试）+ 一个环境。
fn collection_json(base_url: &str) -> String {
    serde_json::json!({
        "format": "miyo-project-backup",
        "schema_version": 4,
        "exported_at": "2026-10-24T00:00:00Z",
        "project": {
            "id": "00000000-0000-0000-0000-000000000001",
            "name": "CLI 集合",
            "description": "",
            "variables": { "team": "xiaozou" },
            "created_at": "2026-10-24T00:00:00Z",
            "updated_at": "2026-10-24T00:00:00Z"
        },
        "folders": [
            { "id": "00000000-0000-0000-0000-0000000000f1", "project_id": "00000000-0000-0000-0000-000000000001",
              "parent_id": null, "name": "用户", "sort_order": 0,
              "created_at": "2026-10-24T00:00:00Z", "updated_at": "2026-10-24T00:00:00Z" }
        ],
        "endpoints": [
            {
                "id": "00000000-0000-0000-0000-000000000101",
                "project_id": "00000000-0000-0000-0000-000000000001",
                "folder_id": "00000000-0000-0000-0000-0000000000f1",
                "name": "用户列表", "method": "GET", "path": "/users",
                "description": "", "status": "developing", "sort_order": 0,
                "request": {
                    "params": [], "headers": [], "path_variables": [],
                    "tests": {
                        "pre_request": [], "extract": [],
                        "assertions": [
                            { "type": "status", "op": "eq", "expected": 200 },
                            { "type": "jsonpath", "path": "$.data[0].name", "op": "contains", "expected": "miyo" }
                        ]
                    }
                },
                "created_at": "2026-10-24T00:00:00Z", "updated_at": "2026-10-24T00:00:00Z"
            },
            {
                "id": "00000000-0000-0000-0000-000000000102",
                "project_id": "00000000-0000-0000-0000-000000000001",
                "folder_id": null,
                "name": "失败用例", "method": "GET", "path": "/bad",
                "description": "", "status": "developing", "sort_order": 1,
                "request": {
                    "params": [], "headers": [], "path_variables": [],
                    "tests": {
                        "pre_request": [], "extract": [],
                        "assertions": [ { "type": "status", "op": "eq", "expected": 200 } ]
                    }
                },
                "created_at": "2026-10-24T00:00:00Z", "updated_at": "2026-10-24T00:00:00Z"
            },
            {
                "id": "00000000-0000-0000-0000-000000000103",
                "project_id": "00000000-0000-0000-0000-000000000001",
                "folder_id": null,
                "name": "无测试接口", "method": "GET", "path": "/users",
                "description": "", "status": "developing", "sort_order": 2,
                "request": { "params": [], "headers": [], "path_variables": [] },
                "created_at": "2026-10-24T00:00:00Z", "updated_at": "2026-10-24T00:00:00Z"
            }
        ],
        "environments": [
            { "id": "00000000-0000-0000-0000-0000000000e1", "project_id": "00000000-0000-0000-0000-000000000001",
              "name": "开发环境", "base_url": base_url, "variables": [],
              "created_at": "2026-10-24T00:00:00Z", "updated_at": "2026-10-24T00:00:00Z" }
        ],
        "mock_rules": [], "response_examples": []
    })
    .to_string()
}

fn write_collection(dir: &std::path::Path, base_url: &str) -> PathBuf {
    let path = dir.join("collection.json");
    std::fs::write(&path, collection_json(base_url)).unwrap();
    path
}

fn base_opts() -> RunOptions {
    RunOptions::default()
}

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("miyo-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn runs_collection_with_pass_fail_and_skip() {
    let dir = fresh_dir("run");
    let base = spawn_server().await;
    let backup = write_collection(&dir, &base);

    let file = load_backup(&backup).expect("集合应可解析");
    let report = run_collection(file, &base_opts()).await;

    assert_eq!(report.project, "CLI 集合");
    assert_eq!(report.total_endpoints, 3, "备份含 3 个接口");
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 1);
    assert!(!report.ok());

    // 通过用例：断言计数与分组名
    let pass = report
        .cases
        .iter()
        .find(|c| c.name == "用户列表")
        .expect("应有用户列表用例");
    assert_eq!(pass.group, "用户");
    match &pass.kind {
        CaseKind::Passed {
            status,
            duration_ms: _,
            assertions,
        } => {
            assert_eq!(*status, Some(200));
            assert_eq!(*assertions, (2, 2));
        }
        other => panic!("应为 Passed：{other:?}"),
    }

    // 失败用例：失败断言明细
    let fail = report.cases.iter().find(|c| c.name == "失败用例").unwrap();
    match &fail.kind {
        CaseKind::Failed { failures, .. } => {
            assert_eq!(failures.len(), 1, "status eq 200 应失败");
        }
        other => panic!("应为 Failed：{other:?}"),
    }

    // 无测试配置的接口不进报告（与桌面端跳过口径一致）
    assert!(report.cases.iter().all(|c| c.name != "无测试接口"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn name_filter_and_junit_output() {
    let dir = fresh_dir("junit");
    let base = spawn_server().await;
    let backup = write_collection(&dir, &base);

    let file = load_backup(&backup).unwrap();
    let junit_path = dir.join("out/report.xml");
    let options = RunOptions {
        endpoint_filters: vec!["用户".into()],
        ..base_opts()
    };
    let report = run_collection(file, &options).await;
    assert_eq!(report.cases.len(), 1, "过滤后只跑用户列表");
    assert!(report.ok());
    match &report.cases[0].kind {
        CaseKind::Passed { status, .. } => assert_eq!(*status, Some(200)),
        other => panic!("应为 Passed：{other:?}"),
    }

    junit::write_junit(&report, &junit_path).expect("JUnit 报告应写出");
    let xml = std::fs::read_to_string(&junit_path).unwrap();
    assert!(xml.contains("<testsuites"), "应含根节点");
    assert!(xml.contains("GET /users"), "应含用例名");
    assert!(
        xml.contains("http_status\" value=\"200\""),
        "应含状态码 property"
    );
    assert!(!xml.contains("<failure"), "全过时无 failure 节点");

    // 失败场景：failure 节点出现
    let file = load_backup(&backup).unwrap();
    let report = run_collection(file, &base_opts()).await;
    let xml = junit::junit_xml(&report);
    assert!(xml.contains("<failure"), "失败应有 failure 节点");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn variable_injection_reaches_request() {
    let dir = fresh_dir("var");
    let base = spawn_server().await;
    let backup = write_collection(&dir, &base);

    // 路径里带 {{who}}：--var 注入后应渲染成 echo/:id 的实际值
    let mut file = load_backup(&backup).unwrap();
    file.endpoints[2].path = "/echo/{{who}}".to_string();
    file.endpoints[2].request.tests = Some(serde_json::json!({
        "pre_request": [], "extract": [],
        "assertions": [ { "type": "body", "op": "contains", "expected": "id=miyo" } ]
    }));
    let options = RunOptions {
        endpoint_filters: vec!["无测试".into()],
        vars: vec![("who".into(), "miyo".into())],
        ..base_opts()
    };
    let report = run_collection(file, &options).await;
    assert_eq!(report.cases.len(), 1);
    match &report.cases[0].kind {
        CaseKind::Passed { .. } => {}
        other => panic!("变量注入后应通过：{other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn missing_base_url_skips_with_reason() {
    let dir = fresh_dir("skip");
    // 集合里没有环境，也不传 --base-url → 有测试的接口应跳过并给出原因
    let backup = write_collection(&dir, "http://127.0.0.1:1");
    let mut file = load_backup(&backup).unwrap();
    file.environments.clear();

    let report = run_collection(file, &base_opts()).await;
    let skipped: Vec<&CaseOutcome> = report
        .cases
        .iter()
        .filter(|c| matches!(c.kind, CaseKind::Skipped { .. }))
        .collect();
    assert_eq!(skipped.len(), 2, "两个带测试的接口都应跳过");
    for case in skipped {
        match &case.kind {
            CaseKind::Skipped { reason } => {
                assert!(
                    reason.contains("Base URL"),
                    "跳过原因应指向 Base URL：{reason}"
                );
            }
            _ => unreachable!(),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
