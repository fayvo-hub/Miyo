//! Swagger 2.0 导入（M12）。
//!
//! 支持 paths + parameters（query/header/path/body/formData）、
//! securityDefinitions（basic / apiKey / bearer）、produces/consumes、
//! responses → 响应示例。输出与 OpenAPI 3.0 导入相同的 `ImportedEndpoint`。

use std::collections::HashMap;

use fox_core::model::{ApiKeyLocation, AuthSpec, BodySpec, HttpMethod, KeyValue, RequestSpec};
use fox_core::AppError;
use serde_json::Value;

use crate::import::{parse_value, value_to_string, ImportedEndpoint, ImportedExample};

/// 导入 Swagger 2.0 文档，返回待落库的接口列表。
pub fn import_swagger2(text: &str) -> Result<Vec<ImportedEndpoint>, AppError> {
    let doc = parse_value(text)?;
    let root = doc
        .as_object()
        .ok_or_else(|| AppError::Validation("Swagger 文档根节点不是对象".to_string()))?;
    let swagger = root
        .get("swagger")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !swagger.starts_with("2.0") {
        return Err(AppError::Validation(format!(
            "暂不支持 Swagger 版本 {}，仅支持 2.0",
            swagger
        )));
    }

    let global_consumes = str_list(root.get("consumes"));
    let global_produces = str_list(root.get("produces"));
    let security = map_security(root.get("securityDefinitions"));

    let mut out = Vec::new();
    let paths = root
        .get("paths")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AppError::Validation("Swagger 文档缺少 paths".to_string()))?;
    for (path, item) in paths {
        let Some(ops) = item.as_object() else {
            continue;
        };
        for (method_str, op_val) in ops {
            let Ok(method) = method_str.parse::<HttpMethod>() else {
                continue;
            };
            let Some(op) = op_val.as_object() else {
                continue;
            };
            if let Some(ep) = map_operation(
                path,
                method,
                op,
                &global_consumes,
                &global_produces,
                &security,
            ) {
                out.push(ep);
            }
        }
    }
    Ok(out)
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|x| x.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 由 JSON Schema 推导一份占位示例值（Swagger 2.0 的 body 参数）。
///
/// 优先级：`example` > `default` > `enum[0]` > 按类型骨架。
/// `$ref` 不解析（拿不到 definitions，且导入对 schema 无损要求低），
/// 走 `{}` 兜底，与旧行为一致。
fn example_from_schema_json(schema: &Value, depth: usize) -> Value {
    if let Some(ex) = schema.get("example") {
        return ex.clone();
    }
    if let Some(default) = schema.get("default") {
        return default.clone();
    }
    // 深度上限：自引用 schema 防爆栈。
    if depth >= MAX_SCHEMA_DEPTH {
        return Value::Object(serde_json::Map::new());
    }
    // `$ref` 或 allOf 等组合：合并所有分支的属性。
    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        let mut map = serde_json::Map::new();
        for b in all_of {
            if let Value::Object(part) = example_from_schema_json(b, depth + 1) {
                map.extend(part);
            }
        }
        return Value::Object(map);
    }
    // 显式 `properties` 是唯一可靠的「有字段」信号：优先于 type 判断，
    // 因为很多文档省略 type: object 只写 properties。
    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        let mut map = serde_json::Map::new();
        for (name, prop) in props {
            map.insert(name.clone(), example_from_schema_json(prop, depth + 1));
        }
        return Value::Object(map);
    }
    match schema.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "array" => {
            let item = schema
                .get("items")
                .map(|i| example_from_schema_json(i, depth + 1))
                .unwrap_or(Value::Null);
            Value::Array(vec![item])
        }
        "object" => Value::Object(serde_json::Map::new()),
        "string" => schema
            .get("enum")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
        "integer" => Value::Number(0.into()),
        "number" => Value::Number(serde_json::Number::from_f64(0.0).unwrap_or_else(|| 0.into())),
        "boolean" => Value::Bool(false),
        _ => Value::Object(serde_json::Map::new()),
    }
}

/// schema 深度上限：防御自引用 / 极深嵌套。
const MAX_SCHEMA_DEPTH: usize = 12;

/// securityDefinitions → AuthSpec（显式优先级：basic > bearer > apiKey）。
fn map_security(v: Option<&Value>) -> Option<AuthSpec> {
    let defs = v?.as_object()?;
    for def in defs.values() {
        let Some(obj) = def.as_object() else {
            continue;
        };
        if obj.get("type").and_then(|x| x.as_str()) == Some("basic") {
            return Some(AuthSpec::Basic {
                username: String::new(),
                password: String::new(),
            });
        }
    }
    for def in defs.values() {
        let Some(obj) = def.as_object() else {
            continue;
        };
        let ty = obj.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if ty != "apiKey" {
            continue;
        }
        let name = obj
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let location = if obj.get("in").and_then(|x| x.as_str()) == Some("header") {
            ApiKeyLocation::Header
        } else {
            ApiKeyLocation::Query
        };
        if name.eq_ignore_ascii_case("authorization") {
            return Some(AuthSpec::Bearer {
                token: String::new(),
            });
        }
        return Some(AuthSpec::ApiKey {
            key: name,
            value: String::new(),
            location,
        });
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn map_operation(
    path: &str,
    method: HttpMethod,
    op: &serde_json::Map<String, Value>,
    global_consumes: &[String],
    global_produces: &[String],
    global_security: &Option<AuthSpec>,
) -> Option<ImportedEndpoint> {
    let mut params = Vec::new();
    let mut headers = Vec::new();
    let mut path_variables = Vec::new();
    let mut form_fields: Vec<KeyValue> = Vec::new();
    let mut body_json: Option<String> = None;

    let parameters = op
        .get("parameters")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for p in &parameters {
        let Some(p) = p.as_object() else {
            continue;
        };
        let name = p
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let description = p
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let default = p
            .get("default")
            .or_else(|| p.get("x-example"))
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let location = p.get("in").and_then(|x| x.as_str()).unwrap_or("");
        let kv = KeyValue {
            key: name.clone(),
            value: default,
            enabled: true,
            description,
            field_type: Default::default(),
            required: true,
            example: String::new(),
        };
        match location {
            "query" => params.push(kv),
            "header" => headers.push(kv),
            "path" => path_variables.push(kv),
            "formData" => form_fields.push(kv),
            "body" => {
                // Swagger 2.0 的 body 参数用 `schema` 描述字段，且绝大多数
                // 文档只给 `properties` 不给 `example`/`default`；旧实现此时
                // 一律落成 `{}`，等于把请求体参数丢掉。改为 schema 推导。
                let schema = p.get("schema").unwrap_or(&Value::Null);
                body_json = Some(value_to_string(&example_from_schema_json(schema, 0)));
            }
            _ => {}
        }
    }

    let consumes = str_list(op.get("consumes"));
    let produces = str_list(op.get("produces"));
    let consumes_owned = if consumes.is_empty() {
        global_consumes.to_vec()
    } else {
        consumes
    };
    let produces_owned = if produces.is_empty() {
        global_produces.to_vec()
    } else {
        produces
    };

    let body = if let Some(raw) = body_json {
        BodySpec::Json { raw }
    } else if !form_fields.is_empty() {
        if consumes_owned.iter().any(|c| c.contains("multipart")) {
            BodySpec::Multipart {
                fields: form_fields
                    .iter()
                    .map(|kv| fox_core::model::MultipartField {
                        key: kv.key.clone(),
                        value: kv.value.clone(),
                        value_type: fox_core::model::MultipartValueType::Text,
                        enabled: true,
                    })
                    .collect(),
            }
        } else {
            BodySpec::UrlEncoded {
                fields: form_fields,
            }
        }
    } else {
        BodySpec::None
    };

    let auth = op
        .get("security")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.as_object())
        .and_then(|s| s.keys().next())
        .and_then(|_| global_security.clone())
        .or_else(|| global_security.clone());

    let mut examples = Vec::new();
    if let Some(responses) = op.get("responses").and_then(|v| v.as_object()) {
        for (code, resp) in responses {
            let status = code.parse::<u16>().unwrap_or(200);
            let resp_obj = resp.as_object().cloned().unwrap_or_default();
            let content_type = resp_obj
                .get("headers")
                .and_then(|h| h.as_object())
                .and_then(|h| h.get("Content-Type"))
                .and_then(|v| v.as_object())
                .and_then(|h| h.get("example"))
                .and_then(|v| v.as_str())
                .map(|x| x.to_string())
                .or_else(|| produces_owned.first().cloned())
                .unwrap_or_else(|| "application/json".to_string());
            let body = resp_obj
                .get("schema")
                .and_then(|s| s.get("example").or_else(|| s.get("default")))
                .map(|v| v.to_string())
                .or_else(|| {
                    resp_obj
                        .get("examples")
                        .and_then(|ex| ex.get("application/json"))
                        .map(|v| v.to_string())
                })
                .unwrap_or_default();
            let name = resp_obj
                .get("description")
                .and_then(|x| x.as_str())
                .map(|x| x.to_string())
                .unwrap_or_else(|| format!("响应 {status}"));
            examples.push(ImportedExample {
                name,
                status,
                content_type,
                headers: HashMap::new(),
                body,
            });
        }
    }

    let name = op
        .get("summary")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| op.get("operationId").and_then(|x| x.as_str()))
        .map(|x| x.to_string())
        .unwrap_or_else(|| format!("{method} {path}"));
    let description = op
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let folder_hint = op
        .get("tags")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|x| x.as_str())
        .map(|x| x.to_string());

    Some(ImportedEndpoint {
        name,
        method,
        path: path.to_string(),
        description,
        request: RequestSpec {
            params,
            headers,
            path_variables,
            auth: auth.unwrap_or(AuthSpec::None),
            body,
            ..Default::default()
        },
        examples,
        folder_hint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{
      "swagger": "2.0",
      "info": { "title": "t", "version": "1.0" },
      "securityDefinitions": {
        "basicAuth": { "type": "basic" },
        "api_key": { "type": "apiKey", "name": "X-API-Key", "in": "header" }
      },
      "consumes": ["application/json"],
      "produces": ["application/json"],
      "paths": {
        "/users/{id}": {
          "get": {
            "operationId": "getUser",
            "tags": ["users"],
            "parameters": [
              { "name": "id", "in": "path", "required": true, "type": "integer" },
              { "name": "verbose", "in": "query", "default": "1" },
              { "name": "X-Debug", "in": "header", "default": "true" },
              { "name": "body", "in": "body", "schema": { "example": { "id": "5" } } }
            ],
            "responses": { "200": { "description": "OK", "schema": { "example": { "name": "x" } } } }
          }
        },
        "/login": {
          "post": {
            "summary": "登录",
            "consumes": ["application/x-www-form-urlencoded"],
            "parameters": [
              { "name": "user", "in": "formData", "type": "string" },
              { "name": "pass", "in": "formData", "type": "string" }
            ],
            "responses": { "200": { "description": "ok" } }
          }
        }
      }
    }"#;

    #[test]
    fn swagger_version_rejected() {
        let doc = DOC.replace("\"swagger\": \"2.0\"", "\"swagger\": \"1.2\"");
        assert!(import_swagger2(&doc).is_err());
    }

    #[test]
    fn parses_paths_parameters_and_body() {
        let list = import_swagger2(DOC).unwrap();
        assert_eq!(list.len(), 2);

        let get = list.iter().find(|e| e.method.to_string() == "GET").unwrap();
        assert_eq!(get.path, "/users/{id}");
        assert_eq!(get.name, "getUser");
        assert_eq!(get.folder_hint.as_deref(), Some("users"));
        assert_eq!(get.request.path_variables.len(), 1);
        assert_eq!(get.request.path_variables[0].key, "id");
        assert_eq!(get.request.params[0].key, "verbose");
        assert_eq!(get.request.params[0].value, "1");
        assert_eq!(get.request.headers[0].key, "X-Debug");
        let fox_core::model::BodySpec::Json { raw } = &get.request.body else {
            panic!("期望 JSON body");
        };
        assert!(raw.contains("id"));
        assert!(raw.contains("5"));
        assert_eq!(get.examples.len(), 1);
        assert_eq!(get.examples[0].status, 200);
        assert!(get.examples[0].body.contains("name"));
        assert_eq!(get.examples[0].content_type, "application/json");
    }

    #[test]
    fn formdata_becomes_urlencoded() {
        let list = import_swagger2(DOC).unwrap();
        let post = list
            .iter()
            .find(|e| e.method.to_string() == "POST")
            .unwrap();
        assert_eq!(post.name, "登录");
        let fox_core::model::BodySpec::UrlEncoded { fields } = &post.request.body else {
            panic!("期望 urlencoded body");
        };
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn security_definitions_mapped() {
        let list = import_swagger2(DOC).unwrap();
        let get = list.iter().find(|e| e.method.to_string() == "GET").unwrap();
        // 未显式 security 的接口沿用全局 basic。
        assert!(matches!(
            get.request.auth,
            fox_core::model::AuthSpec::Basic { .. }
        ));
    }

    /// Swagger 2.0 `in: body` 只给 `schema.properties`（无 example/default）。
    ///
    /// 旧实现此时把 body 落成 `{}`，请求体字段全丢——与 OpenAPI 3.0 同一类 bug。
    #[test]
    fn body_parameter_schema_properties_are_not_lost() {
        let doc = r#"{
          "swagger": "2.0",
          "info": { "title": "t", "version": "1.0" },
          "consumes": ["application/json"],
          "paths": {
            "/posts": {
              "post": {
                "parameters": [
                  { "name": "body", "in": "body", "required": true,
                    "schema": {
                      "type": "object",
                      "required": ["title"],
                      "properties": {
                        "title":  { "type": "string", "example": "小秦技术" },
                        "body":   { "type": "string" },
                        "userId": { "type": "integer" },
                        "tags":   { "type": "array", "items": { "type": "string" } }
                      }
                    } }
                ],
                "responses": { "201": { "description": "ok" } }
              }
            }
          }
        }"#;
        let list = import_swagger2(doc).unwrap();
        let post = list
            .iter()
            .find(|e| e.method.to_string() == "POST")
            .unwrap();
        let fox_core::model::BodySpec::Json { raw } = &post.request.body else {
            panic!("期望 json body，实得 {:?}", post.request.body);
        };
        let v: serde_json::Value = serde_json::from_str(raw).expect("body 应是合法 JSON");
        assert_ne!(v, serde_json::json!({}), "body 参数被丢掉了");
        assert_eq!(v["title"], "小秦技术");
        assert_eq!(v["body"], "");
        assert_eq!(v["userId"], 0);
        assert_eq!(v["tags"], serde_json::json!([""]));
    }

    /// 顶层 schema 是数组/标量时，body 不应被包成对象。
    #[test]
    fn body_parameter_non_object_schema() {
        let doc = r#"{
          "swagger": "2.0",
          "info": { "title": "t", "version": "1.0" },
          "paths": {
            "/bulk": {
              "post": {
                "parameters": [
                  { "name": "body", "in": "body",
                    "schema": { "type": "array", "items": { "type": "integer" } } }
                ],
                "responses": { "200": { "description": "ok" } }
              }
            }
          }
        }"#;
        let list = import_swagger2(doc).unwrap();
        let post = list
            .iter()
            .find(|e| e.method.to_string() == "POST")
            .unwrap();
        let fox_core::model::BodySpec::Json { raw } = &post.request.body else {
            panic!("期望 json body");
        };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(raw).unwrap(),
            serde_json::json!([0])
        );
    }
}
