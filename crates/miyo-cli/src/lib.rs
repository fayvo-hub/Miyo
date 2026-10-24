//! Miyo headless CLI 库：把桌面端「设置 → 备份」导出的项目 JSON 当作测试集合，
//! 在终端 / CI 里无头运行其中带内嵌测试（`request.tests`）的接口。
//!
//! 与桌面端共享同一套运行器（`miyo_test::run_endpoint`）：pre_request /
//! extract / 断言语义、变量链路（环境 > 项目 > runtime）完全一致；
//! 输出供人读的 [`RunReport`] 与 CI 用的 JUnit XML（[`junit::junit_xml`]）。

pub mod junit;

use std::collections::HashMap;
use std::path::Path;

use miyo_backup::BackupFile;
use miyo_core::model::HttpMethod;
use miyo_core::{merge_variables, resolve_variables};
use miyo_test::run_endpoint;

/// 单次运行配置（与 main.rs 的 clap 参数一一对应）。
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// 选择环境：取该环境的 Base URL 与变量（名称精确匹配）。
    pub env_name: Option<String>,
    /// 覆盖 Base URL（优先级高于环境）。
    pub base_url_override: Option<String>,
    /// 只运行名称包含该子串的接口（大小写不敏感，可多个取并集）。
    pub endpoint_filters: Vec<String>,
    /// 注入变量 `k=v`（优先级最高，覆盖环境 / 项目同名变量）。
    pub vars: Vec<(String, String)>,
    /// 单请求超时毫秒（None = 运行器内置默认）。
    pub timeout_ms: Option<u64>,
    /// 任一失败立即停止。
    pub fail_fast: bool,
}

/// 单接口执行结果。
#[derive(Debug)]
pub struct CaseOutcome {
    pub name: String,
    pub method: String,
    pub path: String,
    /// 直接所属文件夹名（根级接口为空串）。
    pub group: String,
    pub kind: CaseKind,
}

#[derive(Debug)]
pub enum CaseKind {
    Passed {
        status: Option<u16>,
        duration_ms: Option<f64>,
        /// (通过断言数, 断言总数)。
        assertions: (usize, usize),
    },
    Failed {
        status: Option<u16>,
        duration_ms: Option<f64>,
        /// 失败断言 (描述, 原因)。
        failures: Vec<(String, Option<String>)>,
        /// 请求级错误（发送失败 / 配置错误）。
        request_error: Option<String>,
    },
    Skipped {
        reason: String,
    },
}

/// 一次集合运行的整体报告。
pub struct RunReport {
    pub project: String,
    /// 完整备份中的接口总数（含未运行的）。
    pub total_endpoints: usize,
    /// 有内嵌测试、参与运行的接口数（= passed + failed + 非跳过 case 数）。
    pub cases: Vec<CaseOutcome>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration_ms: f64,
}

impl RunReport {
    /// 进程退出码语义：全过 = true。
    pub fn ok(&self) -> bool {
        self.failed == 0
    }
}

/// 读取并解析备份 JSON（接受当前与更名前的格式标识）。
pub fn load_backup(path: &Path) -> Result<BackupFile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("无法读取集合文件 {}：{e}", path.display()))?;
    BackupFile::parse(&text).map_err(|e| format!("集合文件解析失败：{e}"))
}

/// 运行集合：串行执行（extract 变量按序流向后续接口），返回整体报告。
pub async fn run_collection(backup: BackupFile, opts: &RunOptions) -> RunReport {
    let started = std::time::Instant::now();
    let project = &backup.project;

    // ---- 变量表：runtime(--var) > 环境 > 项目（与桌面端 merge 口径一致）----
    let env = opts
        .env_name
        .as_deref()
        .and_then(|name| backup.environments.iter().find(|e| e.name == name));
    let mut runtime_vars: HashMap<String, String> = HashMap::new();
    for (k, v) in &project.variables {
        runtime_vars.insert(k.clone(), v.clone());
    }
    for var in backup.global_variables.iter().filter(|v| v.enabled) {
        let value = if var.local_value.is_empty() {
            &var.remote_value
        } else {
            &var.local_value
        };
        runtime_vars.insert(var.key.clone(), value.clone());
    }
    if let Some(env) = env {
        for var in env.variables.iter().filter(|v| v.enabled) {
            let value = if var.local_value.is_empty() {
                &var.remote_value
            } else {
                &var.local_value
            };
            runtime_vars.insert(var.key.clone(), value.clone());
        }
    }
    for (k, v) in &opts.vars {
        runtime_vars.insert(k.clone(), v.clone());
    }
    let mut runtime_vars = merge_variables(&runtime_vars, &HashMap::new(), &HashMap::new());

    // ---- Base URL：--base-url > --env 指定的环境 > 唯一环境自动选用 ----
    let env = match (&opts.env_name, env, backup.environments.len()) {
        (Some(_), picked, _) => picked,
        (None, picked @ Some(_), _) => picked,
        (None, None, 1) => backup.environments.first(),
        (None, None, _) => None, // 多环境未指定：宁跳过不猜
    };
    let base_url: Option<String> = match &opts.base_url_override {
        Some(url) => Some(resolve_variables(url, &runtime_vars)),
        None => env.map(|e| resolve_variables(&e.base_url, &runtime_vars)),
    };

    // ---- 文件夹名（直接父级）----
    let folder_name = |folder_id: Option<uuid::Uuid>| -> String {
        folder_id
            .and_then(|id| backup.folders.iter().find(|f| f.id == id))
            .map(|f| f.name.clone())
            .unwrap_or_default()
    };

    let total_endpoints = backup.endpoints.len();
    let mut cases: Vec<CaseOutcome> = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut stop = false;

    for ep in &backup.endpoints {
        if stop {
            break;
        }
        // 只运行带内嵌测试的接口：与桌面端「无测试配置跳过」口径一致。
        let Some(tests) = ep.request.tests.as_ref() else {
            continue;
        };
        let _ = tests;
        if !opts.endpoint_filters.is_empty()
            && !opts
                .endpoint_filters
                .iter()
                .any(|f| ep.name.to_lowercase().contains(&f.to_lowercase()))
        {
            continue;
        }
        if ep.method == HttpMethod::Grpc {
            skipped += 1;
            cases.push(CaseOutcome {
                name: ep.name.clone(),
                method: ep.method.as_str().to_string(),
                path: ep.path.clone(),
                group: folder_name(ep.folder_id),
                kind: CaseKind::Skipped {
                    reason: "CLI 暂不支持 gRPC 调试".to_string(),
                },
            });
            continue;
        }

        // URL：{{变量}} 先按运行时表渲染，再拼 Base URL（路径变量由运行器处理）。
        let rendered_path = resolve_variables(&ep.path, &runtime_vars);
        let path_vars: HashMap<String, String> = ep
            .request
            .path_variables
            .iter()
            .filter(|kv| kv.enabled)
            .map(|kv| (kv.key.clone(), kv.value.clone()))
            .collect();
        let base_rendered = base_url.as_deref().map(|b| {
            // 环境变量开关影响 base 中引用的变量已在上面注入；此处仅做变量渲染
            resolve_variables(b, &runtime_vars)
        });
        let url = miyo_core::build_url(base_rendered.as_deref(), &rendered_path, &path_vars);
        if !miyo_core::is_absolute_url(&url) {
            skipped += 1;
            cases.push(CaseOutcome {
                name: ep.name.clone(),
                method: ep.method.as_str().to_string(),
                path: ep.path.clone(),
                group: folder_name(ep.folder_id),
                kind: CaseKind::Skipped {
                    reason: "未提供 Base URL（用 --base-url 指定，或 --env 选择环境）".to_string(),
                },
            });
            continue;
        }

        let (result, _) =
            run_endpoint(ep, &url, &ep.request, &mut runtime_vars, opts.timeout_ms).await;
        let group = folder_name(ep.folder_id);
        let total_assertions = result.outcomes.len();
        let failed_assertions: Vec<(String, Option<String>)> = result
            .outcomes
            .iter()
            .filter(|o| !o.passed)
            .map(|o| (o.description.clone(), o.reason.clone()))
            .collect();
        if result.ok {
            passed += 1;
            cases.push(CaseOutcome {
                name: result.endpoint_name,
                method: result.method,
                path: result.path,
                group,
                kind: CaseKind::Passed {
                    status: result.status,
                    duration_ms: result.duration_ms,
                    assertions: (total_assertions - failed_assertions.len(), total_assertions),
                },
            });
        } else {
            failed += 1;
            cases.push(CaseOutcome {
                name: result.endpoint_name,
                method: result.method,
                path: result.path,
                group,
                kind: CaseKind::Failed {
                    status: result.status,
                    duration_ms: result.duration_ms,
                    failures: failed_assertions,
                    request_error: result.request_error,
                },
            });
            if opts.fail_fast {
                stop = true;
            }
        }
    }

    RunReport {
        project: project.name.clone(),
        total_endpoints,
        cases,
        passed,
        failed,
        skipped,
        total_duration_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}
