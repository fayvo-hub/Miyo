//! 通用工具：时间、UUID、URL 构建、JSON 格式化。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::AppError;

/// 当前 UTC 时间。
pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

/// 新的 UUID v4。
pub fn new_uuid() -> Uuid {
    Uuid::new_v4()
}

/// 判断字符串是否以 http:// 或 https:// 开头。
pub fn is_absolute_url(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("http://") || t.starts_with("https://")
}

/// 路径变量替换：支持 `{id}` 与 `{{id}}` 两种写法，按 key 长度降序替换避免前缀冲突。
pub fn replace_path_variables(path: &str, path_vars: &HashMap<String, String>) -> String {
    let mut result = path.to_string();
    let mut keys: Vec<&String> = path_vars.keys().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for key in keys {
        let value = &path_vars[key];
        result = result.replace(&format!("{{{{{key}}}}}"), value);
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}

/// 路径变量名的合法形态：字母/下划线/`$` 开头，后续允许字母数字、`_`、`$`、`-`。
/// 与前端 `PathVariablesPanel` 的 `NAME` 保持一致（`:id` / `{id}` / `{{id}}` 共用）。
const PATH_VAR_NAME: &str = r"[A-Za-z_$][\w$-]*";

/// 从路径模板抽取变量名，按出现顺序去重。
///
/// 支持三种写法（与前端 `extractNames` 对齐）：
/// - `{{id}}`（本应用变量语法）
/// - `{id}`（OpenAPI / Swagger 路径模板）
/// - `:id`（Postman 路径模板）
///
/// 抽取前先剥掉协议与主机（`https://api.x.com` 里的 `://` 不应被当作 `:x`），
/// 以及 query / hash，避免把 `?a=:b` 误判成路径变量。
///
/// 用途：OpenAPI `paths` 键里的 `{id}` 常不配 `in: path` 参数声明，
/// 单靠 parameters 会漏；导入时以本函数兜底补齐。
pub fn extract_path_variables(path: &str) -> Vec<String> {
    // 只取路径部分：去 origin、去 query/hash。
    let cleaned = match path.find("://") {
        Some(i) => {
            let after = &path[i + 3..];
            match after.find('/') {
                Some(j) => &after[j..],
                None => "",
            }
        }
        None => path,
    };
    let cleaned = match cleaned.find(['?', '#']) {
        Some(i) => &cleaned[..i],
        None => cleaned,
    };

    let mut found: Vec<(usize, String)> = Vec::new();
    // 先认 `{{id}}`，再认 `{id}`；双花括号必须先匹配，否则会被单花括号拆开。
    let double = regex::Regex::new(&format!(r"\{{\{{\s*({PATH_VAR_NAME})\s*\}}\}}")).unwrap();
    let single = regex::Regex::new(&format!(r"\{{\s*({PATH_VAR_NAME})\s*\}}")).unwrap();
    let colon = regex::Regex::new(&format!(r"(?:^|/):({PATH_VAR_NAME})")).unwrap();

    for m in double.captures_iter(cleaned) {
        let full = m.get(0).unwrap();
        found.push((full.start(), m[1].to_string()));
    }
    // 单花括号扫描时先把 `{{...}}` 屏蔽掉，避免重复/错切。
    let masked = double.replace_all(cleaned, |c: &regex::Captures| " ".repeat(c[0].len()));
    for m in single.captures_iter(&masked) {
        let full = m.get(0).unwrap();
        found.push((full.start(), m[1].to_string()));
    }
    for m in colon.captures_iter(cleaned) {
        found.push((m.get(1).unwrap().start(), m[1].to_string()));
    }

    found.sort_by_key(|(i, _)| *i);
    let mut out: Vec<String> = Vec::new();
    for (_, name) in found {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// URL 拼接规则（SPEC 第 13 节）：
/// 1. path 是完整 URL 时直接使用；
/// 2. 否则与 base_url 拼接；
/// 3. path 变量必须先替换。
pub fn build_url(
    base_url: Option<&str>,
    path: &str,
    path_vars: &HashMap<String, String>,
) -> String {
    let rendered_path = replace_path_variables(path, path_vars);
    if is_absolute_url(&rendered_path) {
        return rendered_path;
    }
    let p = rendered_path.trim().trim_start_matches('/');
    match base_url {
        Some(base) => {
            let base = base.trim().trim_end_matches('/');
            if base.is_empty() {
                format!("/{p}")
            } else {
                format!("{base}/{p}")
            }
        }
        None => format!("/{p}"),
    }
}

/// 格式化 JSON 文本；非法时返回错误。
pub fn format_json(raw: &str) -> Result<String, AppError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(AppError::Json)?;
    serde_json::to_string_pretty(&value).map_err(AppError::Json)
}

/// 判断 Content-Type 是否为 JSON。
pub fn is_json_content_type(content_type: &str) -> bool {
    let ct = content_type.split(';').next().unwrap_or("").trim();
    ct == "application/json" || ct == "text/json" || ct.ends_with("+json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn absolute_url_used_directly() {
        let v = vars(&[]);
        let out = build_url(
            Some("https://base.com"),
            "https://api.example.com/users",
            &v,
        );
        assert_eq!(out, "https://api.example.com/users");
    }

    #[test]
    fn relative_path_joined_with_base() {
        let v = vars(&[]);
        assert_eq!(
            build_url(Some("https://api.example.com"), "/users", &v),
            "https://api.example.com/users"
        );
    }

    #[test]
    fn trailing_slash_normalized() {
        let v = vars(&[]);
        assert_eq!(
            build_url(Some("https://api.example.com/"), "users", &v),
            "https://api.example.com/users"
        );
    }

    #[test]
    fn path_variables_replaced() {
        let v = vars(&[("id", "10")]);
        assert_eq!(
            build_url(Some("https://api.example.com"), "/users/{id}", &v),
            "https://api.example.com/users/10"
        );
    }

    #[test]
    fn path_variables_double_braces_replaced() {
        let v = vars(&[("id", "10")]);
        assert_eq!(
            build_url(Some("https://api.example.com"), "/users/{{id}}", &v),
            "https://api.example.com/users/10"
        );
    }

    #[test]
    fn no_base_url_keeps_path() {
        let v = vars(&[]);
        assert_eq!(build_url(None, "/users", &v), "/users");
    }

    #[test]
    fn format_json_pretty() {
        let out = format_json("{\"a\":1}").unwrap();
        assert!(out.contains("\n  \"a\": 1"));
    }

    #[test]
    fn format_json_invalid() {
        assert!(format_json("{not json}").is_err());
    }

    #[test]
    fn json_content_type_detection() {
        assert!(is_json_content_type("application/json"));
        assert!(is_json_content_type("application/json; charset=utf-8"));
        assert!(is_json_content_type("application/problem+json"));
        assert!(!is_json_content_type("text/html"));
    }

    // ---------- extract_path_variables ----------

    #[test]
    fn extract_curly_braces() {
        assert_eq!(extract_path_variables("/users/{id}"), vec!["id"]);
        assert_eq!(
            extract_path_variables("/orgs/{org}/repos/{repo}"),
            vec!["org", "repo"]
        );
    }

    #[test]
    fn extract_double_braces_not_split() {
        // `{{id}}` 必须整体识别成 id，不能被单花括号拆成 `{id` 之类。
        assert_eq!(extract_path_variables("/users/{{id}}"), vec!["id"]);
        assert_eq!(
            extract_path_variables("/a/{{x}}/b/{y}"),
            vec!["x", "y"],
            "双花括号与单花括号混用"
        );
    }

    #[test]
    fn extract_colon_style() {
        assert_eq!(extract_path_variables("/users/:id"), vec!["id"]);
        assert_eq!(
            extract_path_variables("/orgs/:org/repos/:repo"),
            vec!["org", "repo"]
        );
    }

    #[test]
    fn extract_order_preserved_and_deduped() {
        assert_eq!(
            extract_path_variables("/{b}/{a}/{b}"),
            vec!["b", "a"],
            "按出现顺序，重复只留一个"
        );
    }

    #[test]
    fn extract_ignores_origin_and_query() {
        // origin 里的 `://` 不能被当成 `:x`；query/hash 里的也不认。
        assert_eq!(
            extract_path_variables("https://api.x.com/users/{id}"),
            vec!["id"]
        );
        assert_eq!(
            extract_path_variables("https://api.x.com:8443/users/{id}?a=:b#c"),
            vec!["id"],
            "端口号的冒号与 query 里的 :b 都不算"
        );
    }

    #[test]
    fn extract_no_variables_returns_empty() {
        assert!(extract_path_variables("/users/list").is_empty());
        assert!(extract_path_variables("").is_empty());
        assert!(extract_path_variables("/").is_empty());
    }

    #[test]
    fn extract_colon_needs_segment_boundary() {
        // `:id` 必须是路径段开头，`/a:b` 不算（避免误伤 `http://` 之类）。
        assert!(extract_path_variables("/items/a:b").is_empty());
        assert_eq!(extract_path_variables("/items/:id"), vec!["id"]);
    }
}
