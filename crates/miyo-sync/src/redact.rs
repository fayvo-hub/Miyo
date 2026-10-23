//! 快照脱敏：推送到共享远端（git / S3 / db）前，把敏感值替换为占位符。
//!
//! 威胁模型：远端对**全团队可见**（同事、CI、仓库读者）。任何"只属于某台机器
//! 或某个人"的凭证都不应离开本机——一旦推出去，撤销成本远高于本地重填。
//!
//! 策略：
//! - 推送前：命中敏感规则的值替换为 [`PLACEHOLDER`]，并登记到 [`RedactionLog`]；
//! - 拉取后：占位符不覆盖本地已有值（本地值为空时留占位符，提示用户补填）。
//!
//! 注意：这是**尽力而为**的启发式过滤，不是保证。命中规则见 [`is_sensitive_key`]。

use serde::{Deserialize, Serialize};

use miyo_backup::BackupFile;
use miyo_core::model::{AuthSpec, KeyValue, RequestSpec};

/// 敏感值占位符。
pub const PLACEHOLDER: &str = "__MIYO_REDACTED__";

/// 品牌更名前写入远端快照的旧占位符：拉取合并时同样识别，
/// 避免旧快照的占位值覆盖本地真实值。
pub const LEGACY_PLACEHOLDER: &str = "__RUSTFOX_REDACTED__";

/// 判断值是否为（当前或历史版本的）脱敏占位符。
pub fn is_placeholder(value: &str) -> bool {
    value == PLACEHOLDER || value == LEGACY_PLACEHOLDER
}

/// 脱敏日志：本次推送中被替换的字段（供前端展示"同步了什么被过滤"）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RedactionLog {
    /// 被脱敏的项，形如 `环境变量 prod_token`、`接口 GET /login 的请求头 Authorization`。
    pub entries: Vec<String>,
}

impl RedactionLog {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 敏感键名启发式：命中即视为需要脱敏。
///
/// 覆盖常见凭证词根；`*_url` / `endpoint` 之类的地址键**不**命中
/// （团队内通常需要共享，且不是凭证）。
pub fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    const TOKENS: &[&str] = &[
        "authorization",
        "cookie",
        "token",
        "secret",
        "password",
        "passwd",
        "pwd",
        "api-key",
        "apikey",
        "api_key",
        "access_key",
        "accesskey",
        "private",
        "credential",
        "sessionid",
        "session_id",
        "x-auth",
    ];
    TOKENS.iter().any(|t| k.contains(t))
}

/// 脱敏结果。
#[derive(Debug, Clone)]
pub struct RedactionReport {
    pub snapshot: BackupFile,
    pub log: RedactionLog,
}

/// 对快照做脱敏，返回脱敏后的副本与日志（原快照不变）。
pub fn redact_snapshot(snapshot: &BackupFile) -> RedactionReport {
    let mut out = snapshot.clone();
    let mut log = RedactionLog::default();

    redact_project(&mut out, &mut log);
    for endpoint in &mut out.endpoints {
        redact_request(&mut endpoint.request, &mut log, &endpoint.name);
    }
    for env in &mut out.environments {
        for var in &mut env.variables {
            if is_sensitive_key(&var.key) && !var.remote_value.is_empty() {
                var.remote_value = PLACEHOLDER.to_string();
                log.entries
                    .push(format!("环境 {} 的变量 {}", env.name, var.key));
            }
            // 本地私有覆盖是明确的"只在本机生效"语义，永不外发。
            if !var.local_value.is_empty() {
                var.local_value = String::new();
                log.entries
                    .push(format!("环境 {} 的本地覆盖 {}", env.name, var.key));
            }
        }
    }
    for mock in &mut out.mock_rules {
        let sensitive: Vec<String> = mock
            .response_headers
            .keys()
            .filter(|k| is_sensitive_key(k))
            .cloned()
            .collect();
        for key in sensitive {
            let changed = mock
                .response_headers
                .get(&key)
                .map(|v| !v.is_empty())
                .unwrap_or(false);
            if changed {
                mock.response_headers
                    .insert(key.clone(), PLACEHOLDER.to_string());
                log.entries
                    .push(format!("Mock 规则 {} 的响应头 {}", mock.path, key));
            }
        }
    }
    report_unique(&mut log);
    RedactionReport { snapshot: out, log }
}

/// 拉取后还原：占位符不覆盖本地已有值。
///
/// - 本地值为空（或原本就是占位符）→ 保留占位符，提示用户补填；
/// - 本地值非空 → 用本地值覆盖占位符（凭证留在本机）。
pub fn restore_local_values(snapshot: &mut BackupFile, local: &BackupFile) {
    use std::collections::HashMap;
    let local_env: HashMap<&str, &miyo_core::model::Environment> = local
        .environments
        .iter()
        .map(|e| (e.name.as_str(), e))
        .collect();

    for env in &mut snapshot.environments {
        let Some(local_env) = local_env.get(env.name.as_str()) else {
            continue;
        };
        let local_vars: HashMap<&str, &miyo_core::model::EnvironmentVariable> = local_env
            .variables
            .iter()
            .map(|v| (v.key.as_str(), v))
            .collect();
        for var in &mut env.variables {
            let Some(local_var) = local_vars.get(var.key.as_str()) else {
                continue;
            };
            if is_placeholder(&var.remote_value) && !local_var.remote_value.is_empty() {
                var.remote_value = local_var.remote_value.clone();
            }
            var.local_value = local_var.local_value.clone();
        }
    }

    let local_endpoints: HashMap<(&str, &str), &miyo_core::model::Endpoint> = local
        .endpoints
        .iter()
        .map(|e| ((e.method.as_str(), e.path.as_str()), e))
        .collect();
    for endpoint in &mut snapshot.endpoints {
        let Some(local_ep) =
            local_endpoints.get(&(endpoint.method.as_str(), endpoint.path.as_str()))
        else {
            continue;
        };
        restore_request(&mut endpoint.request, &local_ep.request);
    }
}

/// 加密存储占位符？否——占位符本身无敏感信息，直接明文可读，
/// 便于同事看到"这里有个凭证需要自行填写"而不是一团密文。
fn redact_project(snapshot: &mut BackupFile, log: &mut RedactionLog) {
    let keys: Vec<String> = snapshot
        .project
        .variables
        .keys()
        .filter(|k| is_sensitive_key(k))
        .cloned()
        .collect();
    for key in keys {
        snapshot
            .project
            .variables
            .insert(key.clone(), PLACEHOLDER.into());
        log.entries.push(format!("项目变量 {key}"));
    }
}

fn redact_request(request: &mut RequestSpec, log: &mut RedactionLog, scope: &str) {
    redact_kv_list(&mut request.headers, log, scope, "请求头");
    redact_kv_list(&mut request.params, log, scope, "查询参数");
    redact_auth(&mut request.auth, log, scope);
}

fn redact_kv_list(items: &mut [KeyValue], log: &mut RedactionLog, scope: &str, label: &str) {
    for kv in items.iter_mut() {
        if !kv.enabled || kv.value.is_empty() {
            continue;
        }
        if is_sensitive_key(&kv.key) || looks_like_secret(&kv.value) {
            kv.value = PLACEHOLDER.to_string();
            log.entries
                .push(format!("接口 {scope} 的{label} {}", kv.key));
        }
    }
}

fn redact_auth(auth: &mut AuthSpec, log: &mut RedactionLog, scope: &str) {
    match auth {
        AuthSpec::None => {}
        AuthSpec::Bearer { token } => {
            if !token.is_empty() {
                *token = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的 Bearer Token"));
            }
        }
        AuthSpec::Basic { password, .. } => {
            if !password.is_empty() {
                *password = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的 Basic 密码"));
            }
        }
        AuthSpec::ApiKey { value, key, .. } => {
            if !value.is_empty() {
                *value = PLACEHOLDER.into();
                log.entries
                    .push(format!("接口 {scope} 的 API Key（{key}）"));
            }
        }
        AuthSpec::DynamicSignature { config } => {
            if !config.app_secret.is_empty() {
                config.app_secret = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的签名 Secret"));
            }
        }
        AuthSpec::OAuth2 {
            client_secret,
            token,
            ..
        } => {
            if !client_secret.is_empty() {
                *client_secret = PLACEHOLDER.into();
                log.entries
                    .push(format!("接口 {scope} 的 OAuth2 Client Secret"));
            }
            // 令牌带过期时间且属个人会话，绝不同步。
            if token.is_some() {
                *token = None;
                log.entries.push(format!("接口 {scope} 的 OAuth2 令牌"));
            }
        }
        AuthSpec::Digest { password, .. } => {
            if !password.is_empty() {
                *password = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的 Digest 密码"));
            }
        }
        AuthSpec::Hawk { key, .. } => {
            if !key.is_empty() {
                *key = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的 Hawk 密钥"));
            }
        }
        AuthSpec::AwsV4 {
            secret_key,
            session_token,
            ..
        } => {
            if !secret_key.is_empty() {
                *secret_key = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的 AWS Secret Key"));
            }
            if session_token.is_some() {
                *session_token = Some(PLACEHOLDER.into());
                log.entries
                    .push(format!("接口 {scope} 的 AWS 临时会话令牌"));
            }
        }
        AuthSpec::Hmac { secret_key, .. } => {
            if !secret_key.is_empty() {
                *secret_key = PLACEHOLDER.into();
                log.entries.push(format!("接口 {scope} 的 HMAC Secret Key"));
            }
        }
    }
}

fn restore_request(target: &mut RequestSpec, local: &RequestSpec) {
    for kv in target.headers.iter_mut() {
        if kv.value != PLACEHOLDER {
            continue;
        }
        if let Some(local_kv) = local.headers.iter().find(|l| l.key == kv.key) {
            if !local_kv.value.is_empty() {
                kv.value = local_kv.value.clone();
            }
        }
    }
    match (&mut target.auth, &local.auth) {
        (AuthSpec::Bearer { token }, AuthSpec::Bearer { token: local_token }) => {
            if *token == PLACEHOLDER && !local_token.is_empty() {
                *token = local_token.clone();
            }
        }
        (
            AuthSpec::Basic { password, .. },
            AuthSpec::Basic {
                password: local_password,
                ..
            },
        ) => {
            if *password == PLACEHOLDER && !local_password.is_empty() {
                *password = local_password.clone();
            }
        }
        (
            AuthSpec::ApiKey { value, .. },
            AuthSpec::ApiKey {
                value: local_value, ..
            },
        ) => {
            if *value == PLACEHOLDER && !local_value.is_empty() {
                *value = local_value.clone();
            }
        }
        (
            AuthSpec::Digest { password, .. },
            AuthSpec::Digest {
                password: local_password,
                ..
            },
        ) => {
            if *password == PLACEHOLDER && !local_password.is_empty() {
                *password = local_password.clone();
            }
        }
        (AuthSpec::Hawk { key, .. }, AuthSpec::Hawk { key: local_key, .. }) => {
            if *key == PLACEHOLDER && !local_key.is_empty() {
                *key = local_key.clone();
            }
        }
        (
            AuthSpec::AwsV4 { secret_key, .. },
            AuthSpec::AwsV4 {
                secret_key: local_secret,
                ..
            },
        ) => {
            if *secret_key == PLACEHOLDER && !local_secret.is_empty() {
                *secret_key = local_secret.clone();
            }
        }
        (
            AuthSpec::Hmac { secret_key, .. },
            AuthSpec::Hmac {
                secret_key: local_secret,
                ..
            },
        ) if *secret_key == PLACEHOLDER && !local_secret.is_empty() => {
            *secret_key = local_secret.clone();
        }
        _ => {}
    }
}

/// 值形态启发式：像 JWT / 长随机串时即便键名不敏感也脱敏。
fn looks_like_secret(value: &str) -> bool {
    // JWT：三段 base64url 以 . 分隔。
    let jwt_like = value.matches('.').count() == 2
        && value.len() > 40
        && value.split('.').all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
    jwt_like
}

fn report_unique(log: &mut RedactionLog) {
    log.entries.sort();
    log.entries.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::sample_snapshot;
    use miyo_core::model::{Environment, EnvironmentVariable, KeyValue};

    #[test]
    fn redacts_sensitive_headers_and_auth() {
        let mut snapshot = sample_snapshot("/login");
        snapshot.endpoints[0].request.headers = vec![
            KeyValue::new("Authorization", "Bearer abc123"),
            KeyValue::new("X-Trace-Id", "keep-me"),
        ];
        snapshot.endpoints[0].request.auth = AuthSpec::Bearer {
            token: "secret-token".into(),
        };

        let report = redact_snapshot(&snapshot);
        let ep = &report.snapshot.endpoints[0];
        assert_eq!(ep.request.headers[0].value, PLACEHOLDER);
        assert_eq!(ep.request.headers[1].value, "keep-me");
        assert_eq!(
            ep.request.auth,
            AuthSpec::Bearer {
                token: PLACEHOLDER.into()
            }
        );
        assert_eq!(report.log.entries.len(), 2);
    }

    #[test]
    fn redacts_env_secrets_and_drops_local_overrides() {
        let mut snapshot = sample_snapshot("/x");
        snapshot.environments = vec![Environment {
            id: uuid::Uuid::new_v4(),
            project_id: snapshot.project.id,
            name: "prod".into(),
            base_url: "https://api.example.com".into(),
            variables: vec![
                EnvironmentVariable {
                    key: "api_token".into(),
                    remote_value: "tok-123".into(),
                    local_value: "".into(),
                    enabled: true,
                    description: None,
                },
                EnvironmentVariable {
                    key: "region".into(),
                    remote_value: "cn-north".into(),
                    local_value: "local-only".into(),
                    enabled: true,
                    description: None,
                },
            ],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }];

        let report = redact_snapshot(&snapshot);
        let vars = &report.snapshot.environments[0].variables;
        assert_eq!(vars[0].remote_value, PLACEHOLDER);
        // 非敏感键保留；本地覆盖清空。
        assert_eq!(vars[1].remote_value, "cn-north");
        assert_eq!(vars[1].local_value, "");
    }

    #[test]
    fn restore_keeps_local_secrets_and_fills_placeholder() {
        let mut remote = sample_snapshot("/login");
        remote.endpoints[0].request.headers = vec![KeyValue::new("Authorization", PLACEHOLDER)];
        remote.endpoints[0].request.auth = AuthSpec::Bearer {
            token: PLACEHOLDER.into(),
        };

        let mut local = sample_snapshot("/login");
        local.endpoints[0].request.headers = vec![KeyValue::new("Authorization", "Bearer mine")];
        local.endpoints[0].request.auth = AuthSpec::Bearer {
            token: "my-token".into(),
        };

        restore_local_values(&mut remote, &local);
        assert_eq!(remote.endpoints[0].request.headers[0].value, "Bearer mine");
        assert_eq!(
            remote.endpoints[0].request.auth,
            AuthSpec::Bearer {
                token: "my-token".into()
            }
        );
    }

    #[test]
    fn looks_like_secret_detects_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        assert!(looks_like_secret(jwt));
        assert!(!looks_like_secret("application/json"));
        assert!(!looks_like_secret("1.2.3"));
    }
}
