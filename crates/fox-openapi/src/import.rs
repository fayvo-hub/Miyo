//! OpenAPI 3.0 导入（M7）。

use std::collections::HashMap;
use std::str::FromStr;

use fox_core::model::{BodySpec, HttpMethod, KeyValue, RequestSpec};
use fox_core::AppError;
use indexmap::IndexMap;
use openapiv3::{
    MediaType, OpenAPI, Parameter, ReferenceOr, RequestBody, Schema, SchemaKind, Type,
};
use serde::Serialize;

/// 导入冲突策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictStrategy {
    /// 跳过重复接口（默认）。
    #[default]
    Skip,
    /// 覆盖重复接口。
    Overwrite,
    /// 复制为新接口。
    Duplicate,
}

impl ConflictStrategy {
    pub fn from_str_cn(s: &str) -> Option<Self> {
        match s {
            "skip" => Some(ConflictStrategy::Skip),
            "overwrite" => Some(ConflictStrategy::Overwrite),
            "duplicate" => Some(ConflictStrategy::Duplicate),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictStrategy::Skip => "skip",
            ConflictStrategy::Overwrite => "overwrite",
            ConflictStrategy::Duplicate => "duplicate",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ConflictStrategy::Skip => "跳过",
            ConflictStrategy::Overwrite => "覆盖",
            ConflictStrategy::Duplicate => "复制",
        }
    }
}

/// 导入的响应示例（尚未落库）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportedExample {
    pub name: String,
    pub status: u16,
    pub content_type: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// 导入的接口（尚未落库）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportedEndpoint {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,
    pub description: String,
    pub request: RequestSpec,
    pub examples: Vec<ImportedExample>,
    /// 目标文件夹名（Postman 分组 / OpenAPI tags 兜底），None 表示顶层。
    pub folder_hint: Option<String>,
}

impl ImportedEndpoint {
    fn new(
        name: String,
        method: HttpMethod,
        path: String,
        description: String,
        request: RequestSpec,
        examples: Vec<ImportedExample>,
        folder_hint: Option<String>,
    ) -> Self {
        ImportedEndpoint {
            name,
            method,
            path,
            description,
            request,
            examples,
            folder_hint,
        }
    }
}

/// 支持的导入文档格式（M12；N14 起含 OpenAPI 3.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    OpenApi30,
    OpenApi31,
    Swagger20,
    Postman21,
    Unknown,
}

impl ImportFormat {
    pub fn label(&self) -> &'static str {
        match self {
            ImportFormat::OpenApi30 => "OpenAPI 3.0",
            ImportFormat::OpenApi31 => "OpenAPI 3.1（已转换为 3.0 子集导入）",
            ImportFormat::Swagger20 => "Swagger 2.0",
            ImportFormat::Postman21 => "Postman 集合 v2.1",
            ImportFormat::Unknown => "无法识别",
        }
    }
}

/// 解析文本为 JSON Value（自动识别 JSON / YAML）。
pub(crate) fn parse_value(text: &str) -> Result<serde_json::Value, AppError> {
    let trimmed = text.trim_start();
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(v) => Ok(v),
        Err(_) => serde_norway::from_str::<serde_json::Value>(text).map_err(|e| {
            AppError::Validation(format!("文档解析失败，请检查是否为合法的 JSON/YAML：{e}"))
        }),
    }
}

/// 自动识别文档格式（OpenAPI 3.0 / 3.1 / Swagger 2.0 / Postman v2.1）。
pub fn detect_format(text: &str) -> ImportFormat {
    let Ok(v) = parse_value(text) else {
        return ImportFormat::Unknown;
    };
    let Some(root) = v.as_object() else {
        return ImportFormat::Unknown;
    };
    if let Some(version) = root.get("openapi").and_then(|v| v.as_str()) {
        if version.starts_with("3.1") {
            return ImportFormat::OpenApi31;
        }
        return ImportFormat::OpenApi30;
    }
    if root.contains_key("swagger") {
        return ImportFormat::Swagger20;
    }
    if root.contains_key("item") && root.contains_key("info") {
        return ImportFormat::Postman21;
    }
    ImportFormat::Unknown
}

/// 统一入口：自动识别格式并导入，返回接口列表与格式。
pub fn import_any(text: &str) -> Result<(Vec<ImportedEndpoint>, ImportFormat), AppError> {
    let format = detect_format(text);
    let imported = match format {
        ImportFormat::OpenApi30 | ImportFormat::OpenApi31 => import_endpoints(text)?,
        ImportFormat::Swagger20 => crate::swagger2::import_swagger2(text)?,
        ImportFormat::Postman21 => crate::postman::import_postman(text)?,
        ImportFormat::Unknown => return Err(AppError::Validation(
            "无法识别的文档格式：支持 OpenAPI 3.0 / 3.1 / Swagger 2.0 / Postman 集合 v2.1（JSON/YAML）"
                .to_string(),
        )),
    };
    Ok((imported, format))
}

/// 解析 OpenAPI 文本（自动识别 JSON / YAML），校验版本为 3.0 / 3.1。
///
/// 3.1 处理：归一化为 3.0 子集后再走同一套 typed 解析（导入只消费
/// paths/operations/parameters/requestBody/examples，schema 细节无损要求低）：
/// - `openapi: 3.1.x` → `3.0.3`；`webhooks` 顶层键丢弃（给出兼容提示）；
/// - `type: [...]` 数组 → 取首个非 null 类型（nullable 语义由 `enum`/示例承载）；
/// - 数字型 `exclusiveMinimum`/`exclusiveMaximum`（3.1）→ 丢弃（3.0 为布尔型）。
pub fn parse_openapi(text: &str) -> Result<OpenAPI, AppError> {
    let mut value = parse_value(text)?;
    let version = value
        .get("openapi")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if version.starts_with("3.1") {
        normalize_openapi31(&mut value);
    }
    let spec: OpenAPI = serde_json::from_value(value).map_err(|e| {
        AppError::Validation(format!(
            "OpenAPI 文件解析失败，请检查是否为合法的 JSON/YAML：{e}"
        ))
    })?;
    if !spec.openapi.starts_with("3.0") {
        return Err(AppError::Validation(format!(
            "暂不支持 OpenAPI 版本 {}，仅支持 3.0 / 3.1（JSON/YAML）",
            spec.openapi
        )));
    }
    Ok(spec)
}

/// OpenAPI 3.1 → 3.0 子集归一化（原地改 Value）。
fn normalize_openapi31(value: &mut serde_json::Value) {
    use serde_json::Value;
    if let Some(root) = value.as_object_mut() {
        root.insert("openapi".into(), Value::String("3.0.3".into()));
        // webhooks 顶层键：3.0 无对应位置，丢弃（paths 导入不受影响）。
        root.remove("webhooks");
    }
    normalize_schema_node(value);
}

/// 递归归一化 schema 节点（3.1 → 3.0 可解析形状）。
fn normalize_schema_node(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            // type 数组 → 首个非 null 类型。
            if let Some(Value::Array(types)) = map.get("type") {
                let picked = types
                    .iter()
                    .filter_map(|t| t.as_str())
                    .find(|t| *t != "null")
                    .unwrap_or("string")
                    .to_string();
                map.insert("type".into(), Value::String(picked));
            }
            // 数字型 exclusiveMinimum/Maximum → 丢弃（3.0 为布尔型）。
            for key in ["exclusiveMinimum", "exclusiveMaximum"] {
                if map.get(key).is_some_and(|v| v.is_number()) {
                    map.remove(key);
                }
            }
            // const → enum 单元素（3.0 无 const 关键字）。
            if let Some(c) = map.remove("const") {
                map.insert("enum".into(), Value::Array(vec![c]));
            }
            for (_, v) in map.iter_mut() {
                normalize_schema_node(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_schema_node(v);
            }
        }
        _ => {}
    }
}

/// 解析并导入 OpenAPI 文档，返回待落库的接口列表。
pub fn import_endpoints(text: &str) -> Result<Vec<ImportedEndpoint>, AppError> {
    let spec = parse_openapi(text)?;
    let mut out = Vec::new();
    for (path, item) in &spec.paths.paths {
        let ReferenceOr::Item(path_item) = item else {
            continue;
        };
        for (method_str, op) in path_item.iter() {
            let method = HttpMethod::from_str(method_str)?;
            out.push(map_operation(path, method, op));
        }
    }
    Ok(out)
}

fn map_operation(path: &str, method: HttpMethod, op: &openapiv3::Operation) -> ImportedEndpoint {
    let folder_hint = op.tags.first().cloned();
    let mut params = Vec::new();
    let mut headers = Vec::new();
    let mut path_variables = Vec::new();
    for p in &op.parameters {
        let ReferenceOr::Item(Parameter::Query { parameter_data, .. }) = p else {
            continue;
        };
        params.push(kv_from_data(parameter_data));
    }
    for p in &op.parameters {
        let ReferenceOr::Item(Parameter::Header { parameter_data, .. }) = p else {
            continue;
        };
        headers.push(kv_from_data(parameter_data));
    }
    for p in &op.parameters {
        let ReferenceOr::Item(Parameter::Path { parameter_data, .. }) = p else {
            continue;
        };
        path_variables.push(kv_from_data(parameter_data));
    }

    let body = op
        .request_body
        .as_ref()
        .and_then(|rb| match rb {
            ReferenceOr::Item(rb) => Some(rb),
            ReferenceOr::Reference { .. } => None,
        })
        .map(body_from_request_body)
        .unwrap_or(BodySpec::None);

    let mut examples = Vec::new();
    for (status, resp) in &op.responses.responses {
        let ReferenceOr::Item(r) = resp else {
            continue;
        };
        let status_str = status.to_string();
        let status_num = match status_str.parse::<u16>() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let content = if r.content.is_empty() {
            let mut map = IndexMap::new();
            map.insert("application/json".into(), MediaType::default());
            map
        } else {
            r.content.clone()
        };
        let (content_type, media) = content
            .iter()
            .next()
            .map(|(ct, m)| (ct.clone(), m.clone()))
            .unwrap();
        let mut ex_headers = HashMap::new();
        for (name, h) in &r.headers {
            if let ReferenceOr::Item(h) = h {
                if let Some(v) = &h.example {
                    ex_headers.insert(name.clone(), value_to_string(v));
                }
            }
        }
        examples.push(ImportedExample {
            name: format!("{status_str} {content_type}"),
            status: status_num,
            content_type,
            headers: ex_headers,
            body: media
                .example
                .as_ref()
                .map(value_to_string)
                .unwrap_or_default(),
        });
    }

    let name = op
        .summary
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{method} {path}"));
    let description = op.description.clone().unwrap_or_default();

    ImportedEndpoint::new(
        name,
        method,
        path.to_string(),
        description,
        RequestSpec {
            params,
            headers,
            path_variables,
            auth: Default::default(),
            body,
            ..Default::default()
        },
        examples,
        folder_hint,
    )
}

fn kv_from_data(data: &openapiv3::ParameterData) -> KeyValue {
    KeyValue {
        key: data.name.clone(),
        value: data
            .example
            .as_ref()
            .map(value_to_string)
            .unwrap_or_default(),
        enabled: true,
        description: data.description.clone().unwrap_or_default(),
        field_type: Default::default(),
        required: data.required,
        example: String::new(),
    }
}

fn body_from_request_body(rb: &RequestBody) -> BodySpec {
    for (content_type, media) in &rb.content {
        if content_type.contains("json") {
            // 显式 example 优先；缺失时由 schema 合成占位体。
            // 只给 schema（无 example）是 OpenAPI 3.0 最常见的写法，
            // 旧实现一律落成 `{}`，等于把整个请求体参数丢掉。
            let synthesized = example_from_media(media);
            let raw = match synthesized {
                Some(v) => value_to_string(&v),
                None => "{}".into(),
            };
            return BodySpec::Json { raw };
        }
    }
    for (content_type, media) in &rb.content {
        if content_type.contains("x-www-form-urlencoded") {
            // 同样：fields 由 example 或 schema.properties 得来。
            let fields = urlencoded_fields(media);
            return BodySpec::UrlEncoded { fields };
        }
    }
    BodySpec::None
}

/// 合成 JSON 体：显式 `example` → 集合式 `examples` 首值 → schema 推导。
///
/// 返回 `None` 表示连 schema 都没有，调用方回退到 `{}`。
fn example_from_media(media: &MediaType) -> Option<serde_json::Value> {
    if let Some(ex) = &media.example {
        return Some(ex.clone());
    }
    // `examples` 是 `media.example` 的互斥替代（Map<String, Example|Ref>）；
    // 取第一个带 value 的，顺序即文档顺序。
    for (_, ex) in &media.examples {
        let ReferenceOr::Item(ex) = ex else { continue };
        if let Some(v) = &ex.value {
            return Some(v.clone());
        }
    }
    let schema = media.schema.as_ref()?;
    Some(example_from_schema_inner(schema, 0))
}

/// `&ReferenceOr<Box<Schema>>`（properties / items 的形状）→ 值。
fn example_from_boxed(schema: &ReferenceOr<Box<Schema>>, depth: usize) -> serde_json::Value {
    match schema {
        ReferenceOr::Item(b) => example_from_schema(b, depth),
        ReferenceOr::Reference { .. } => serde_json::Value::Object(serde_json::Map::new()),
    }
}

/// 由 schema 推导一份占位示例值（结构化字段导入的核心）。
///
/// 优先级：`example` > `default` > `enum[0]` > 按类型骨架。
/// `$ref` 不解析（本函数拿不到 components，且导入对 schema 无损要求低），
/// 走 `{}` 兜底，与旧行为一致，不会更差。
fn example_from_schema(schema: &Schema, depth: usize) -> serde_json::Value {
    if let Some(ex) = &schema.schema_data.example {
        return ex.clone();
    }
    if let Some(default) = &schema.schema_data.default {
        return default.clone();
    }
    // 深度上限：自引用 schema 防爆栈（顺带封顶嵌套对象体积）。
    if depth >= MAX_SCHEMA_DEPTH {
        return serde_json::Value::Object(serde_json::Map::new());
    }
    match &schema.schema_kind {
        SchemaKind::Type(Type::Object(obj)) => {
            let mut map = serde_json::Map::new();
            for (name, prop) in &obj.properties {
                map.insert(name.clone(), example_from_boxed(prop, depth + 1));
            }
            serde_json::Value::Object(map)
        }
        SchemaKind::Type(Type::Array(arr)) => {
            let item = arr
                .items
                .as_ref()
                .map(|i| example_from_boxed(i, depth + 1))
                .unwrap_or(serde_json::Value::Null);
            serde_json::Value::Array(vec![item])
        }
        SchemaKind::Type(Type::String(st)) => first_enum_string(&st.enumeration)
            .map(serde_json::Value::String)
            .unwrap_or_else(|| serde_json::Value::String(String::new())),
        SchemaKind::Type(Type::Integer(_)) => serde_json::Value::Number(0.into()),
        SchemaKind::Type(Type::Number(_)) => {
            serde_json::Value::Number(serde_json::Number::from_f64(0.0).unwrap_or_else(|| 0.into()))
        }
        SchemaKind::Type(Type::Boolean(_)) => serde_json::Value::Bool(false),
        // 组合类型：取分支第一个可用示例，比 {} 更接近真实请求体。
        SchemaKind::AllOf { all_of } => merge_all_of(all_of, depth),
        SchemaKind::OneOf { one_of } => one_of
            .first()
            .map(|s| example_from_schema_inner(s, depth + 1))
            .unwrap_or(serde_json::Value::Null),
        SchemaKind::AnyOf { any_of } => any_of
            .first()
            .map(|s| example_from_schema_inner(s, depth + 1))
            .unwrap_or(serde_json::Value::Null),
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

/// `allOf` 合并：所有分支同上时并入同一个对象（典型 `allOf: [Obj, {required}]`）。
fn merge_all_of(branches: &[ReferenceOr<Schema>], depth: usize) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut merged_any = false;
    for b in branches {
        if let serde_json::Value::Object(part) = example_from_schema_inner(b, depth + 1) {
            for (k, v) in part {
                map.insert(k, v);
            }
            merged_any = true;
        }
    }
    if merged_any {
        serde_json::Value::Object(map)
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    }
}

/// `allOf` 的分支是 `ReferenceOr<Schema>`（非 Box）。
fn example_from_schema_inner(schema: &ReferenceOr<Schema>, depth: usize) -> serde_json::Value {
    match schema {
        ReferenceOr::Item(s) => example_from_schema(s, depth),
        ReferenceOr::Reference { .. } => serde_json::Value::Object(serde_json::Map::new()),
    }
}

fn first_enum_string(enumeration: &[Option<String>]) -> Option<String> {
    enumeration.iter().flatten().next().cloned()
}

/// schema 深度上限：防御自引用 / 极深嵌套。
const MAX_SCHEMA_DEPTH: usize = 12;

/// x-www-form-urlencoded 字段：example 对象优先，其次 schema.properties。
fn urlencoded_fields(media: &MediaType) -> Vec<KeyValue> {
    if let Some(serde_json::Value::Object(map)) = &media.example {
        return map
            .iter()
            .map(|(k, v)| KeyValue::new(k.clone(), value_to_string(v)))
            .collect();
    }
    let Some(ReferenceOr::Item(schema)) = &media.schema else {
        return Vec::new();
    };
    let SchemaKind::Type(Type::Object(obj)) = &schema.schema_kind else {
        return Vec::new();
    };
    obj.properties
        .iter()
        .map(|(name, prop)| {
            let v = example_from_boxed(prop, 0);
            KeyValue::new(name.clone(), value_to_string(&v))
        })
        .collect()
}

/// 示例值转字符串。
pub fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_JSON: &str = r#"{
  "openapi": "3.0.3",
  "info": { "title": "Demo API", "version": "1.0.0" },
  "paths": {
    "/users": {
      "get": {
        "summary": "用户列表",
        "parameters": [
          { "name": "page", "in": "query", "schema": { "type": "integer" }, "example": 1 },
          { "name": "X-Token", "in": "header", "schema": { "type": "string" }, "example": "abc" }
        ],
        "responses": {
          "200": {
            "description": "ok",
            "content": {
              "application/json": { "example": { "items": [1, 2] } }
            }
          }
        }
      },
      "post": {
        "requestBody": {
          "content": {
            "application/json": { "example": { "name": "张三" } }
          }
        },
        "responses": {
          "201": { "description": "created" }
        }
      }
    },
    "/users/{id}": {
      "get": {
        "parameters": [
          { "name": "id", "in": "path", "required": true, "schema": { "type": "integer" }, "example": 10 }
        ],
        "responses": { "404": { "description": "not found" } }
      }
    }
  }
}"#;

    #[test]
    fn import_json_endpoints() {
        let eps = import_endpoints(SAMPLE_JSON).expect("import should succeed");
        assert_eq!(eps.len(), 3);
        let get = eps
            .iter()
            .find(|e| e.method == HttpMethod::GET && e.path == "/users")
            .unwrap();
        assert_eq!(get.name, "用户列表");
        assert_eq!(get.request.params.len(), 1);
        assert_eq!(get.request.params[0].key, "page");
        assert_eq!(get.request.params[0].value, "1");
        assert_eq!(get.request.headers[0].key, "X-Token");
        assert_eq!(get.request.headers[0].value, "abc");
        assert_eq!(get.examples.len(), 1);
        assert_eq!(get.examples[0].status, 200);
        assert!(get.examples[0].body.contains("items"));
        assert_eq!(get.examples[0].content_type, "application/json");
    }

    #[test]
    fn import_json_body_and_path_vars() {
        let eps = import_endpoints(SAMPLE_JSON).unwrap();
        let post = eps.iter().find(|e| e.method == HttpMethod::POST).unwrap();
        match &post.request.body {
            BodySpec::Json { raw } => assert!(raw.contains("张三")),
            other => panic!("expected json body, got {:?}", other),
        }
        let path_get = eps.iter().find(|e| e.path == "/users/{id}").unwrap();
        assert_eq!(path_get.request.path_variables.len(), 1);
        assert_eq!(path_get.request.path_variables[0].value, "10");
    }

    #[test]
    fn import_yaml() {
        let yaml = "openapi: 3.0.0\ninfo:\n  title: Y\n  version: 1.0.0\npaths:\n  /ping:\n    get:\n      responses:\n        '200':\n          description: pong\n";
        let eps = import_endpoints(yaml).unwrap();
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].path, "/ping");
        assert_eq!(eps[0].examples[0].status, 200);
    }

    #[test]
    fn openapi_31_normalized_and_imported() {
        // 空 paths：版本归一化后可解析。
        let doc = r#"{"openapi":"3.1.0","info":{"title":"x","version":"1"},"paths":{}}"#;
        assert!(import_endpoints(doc).unwrap().is_empty());
        assert_eq!(detect_format(doc), ImportFormat::OpenApi31);
        // 典型 3.1 形状：type 数组 + webhooks + 数字 exclusiveMinimum。
        let doc = serde_json::json!({
            "openapi": "3.1.0",
            "info": {"title": "t", "version": "1"},
            "webhooks": {"newPet": {"post": {"responses": {"200": {"description": "ok"}}}}},
            "paths": {
                "/pets": {
                    "get": {
                        "operationId": "listPets",
                        "parameters": [{
                            "name": "limit", "in": "query",
                            "schema": {"type": ["integer", "null"], "exclusiveMinimum": 0}
                        }],
                        "responses": {"200": {"description": "ok"}}
                    }
                }
            }
        });
        let eps = import_endpoints(&doc.to_string()).unwrap();
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].path, "/pets");
        assert_eq!(eps[0].name, "GET /pets");
        assert_eq!(eps[0].request.params.len(), 1);
        assert_eq!(
            import_any(&doc.to_string()).unwrap().1,
            ImportFormat::OpenApi31
        );
    }

    #[test]
    fn reject_invalid_document() {
        let doc = "not a spec at all";
        let err = import_endpoints(doc).unwrap_err();
        assert!(err.user_message().contains("解析失败"));
    }

    #[test]
    fn import_empty_paths_ok() {
        let doc = r#"{"openapi":"3.0.3","info":{"title":"x","version":"1"},"paths":{}}"#;
        let eps = import_endpoints(doc).unwrap();
        assert!(eps.is_empty());
    }

    #[test]
    fn conflict_strategy_roundtrip() {
        assert_eq!(ConflictStrategy::default(), ConflictStrategy::Skip);
        assert_eq!(
            ConflictStrategy::from_str_cn("overwrite"),
            Some(ConflictStrategy::Overwrite)
        );
        assert_eq!(ConflictStrategy::from_str_cn("nope"), None);
        assert_eq!(ConflictStrategy::Duplicate.as_str(), "duplicate");
    }

    #[test]
    fn detect_openapi3() {
        let doc = r#"{"openapi":"3.0.3","info":{"title":"t","version":"1"},"paths":{}}"#;
        assert_eq!(detect_format(doc), ImportFormat::OpenApi30);
    }

    #[test]
    fn detect_swagger2() {
        let doc = r#"{"swagger":"2.0","info":{"title":"t","version":"1"},"paths":{}}"#;
        assert_eq!(detect_format(doc), ImportFormat::Swagger20);
    }

    #[test]
    fn detect_postman() {
        let doc = r#"{"info":{"name":"c","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[]}"#;
        assert_eq!(detect_format(doc), ImportFormat::Postman21);
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(detect_format("hello world"), ImportFormat::Unknown);
        assert_eq!(detect_format(r#"{"foo":1}"#), ImportFormat::Unknown);
    }

    #[test]
    fn detect_yaml_swagger() {
        let yaml = concat!(
            "swagger: \"2.0\"\n",
            "info:\n",
            "  title: t\n",
            "  version: \"1\"\n",
            "paths: {}\n"
        );
        assert_eq!(detect_format(yaml), ImportFormat::Swagger20);
    }

    #[test]
    fn import_any_unknown_format_errors() {
        assert!(import_any("not-a-doc").is_err());
    }

    // ---------- requestBody 结构化字段导入（回归）----------
    //
    // 旧 bug：`body_from_request_body` 只读 `media.example`，而 OpenAPI 3.0
    // 最常见的写法是**只给 schema**（properties 描述字段），此时请求体一律
    // 落成 `{}`，等于把整个 requestBody 参数丢掉。

    /// 只给 schema.properties 时的 JSON 体。
    const BODY_SCHEMA_ONLY: &str = r#"{
  "openapi": "3.0.0",
  "info": { "title": "t", "version": "1" },
  "paths": {
    "/posts": {
      "post": {
        "requestBody": {
          "required": true,
          "content": {
            "application/json": {
              "schema": {
                "type": "object",
                "required": ["title", "body"],
                "properties": {
                  "title":  { "type": "string", "example": "小秦技术" },
                  "body":   { "type": "string" },
                  "userId": { "type": "integer" },
                  "tags":   { "type": "array", "items": { "type": "string" } },
                  "meta": {
                    "type": "object",
                    "properties": { "draft": { "type": "boolean" } }
                  },
                  "kind": { "type": "string", "enum": ["post", "page"] }
                }
              }
            }
          }
        },
        "responses": { "201": { "description": "ok" } }
      }
    }
  }
}"#;

    fn json_body(text: &str) -> serde_json::Value {
        let eps = import_endpoints(text).expect("import ok");
        let ep = eps.iter().find(|e| e.method == HttpMethod::POST).unwrap();
        let BodySpec::Json { raw } = &ep.request.body else {
            panic!("expected json body, got {:?}", ep.request.body);
        };
        serde_json::from_str(raw).expect("body 应是合法 JSON")
    }

    #[test]
    fn schema_only_request_body_is_not_lost() {
        let body = json_body(BODY_SCHEMA_ONLY);
        // 旧实现是 `{}`——这里断言字段真的进来了。
        assert_ne!(body, serde_json::json!({}), "requestBody 参数被丢掉了");
        assert_eq!(body["title"], "小秦技术", "字段级 example 应优先");
        assert_eq!(body["body"], "", "string 骨架");
        assert_eq!(body["userId"], 0, "integer 骨架");
        assert_eq!(body["tags"], serde_json::json!([""]), "array → 单元素骨架");
        assert_eq!(body["meta"]["draft"], false, "嵌套 object 递归");
        assert_eq!(body["kind"], "post", "enum 取首值");
    }

    #[test]
    fn explicit_media_example_still_wins() {
        let spec = r#"{
          "openapi": "3.0.0", "info": { "title": "t", "version": "1" },
          "paths": { "/a": { "post": {
            "requestBody": { "content": { "application/json": {
              "example": { "name": "张三" },
              "schema": { "type": "object", "properties": { "name": { "type": "string" } } }
            } } },
            "responses": { "200": { "description": "ok" } } } } }
        }"#;
        assert_eq!(json_body(spec), serde_json::json!({ "name": "张三" }));
    }

    #[test]
    fn media_examples_map_is_honored() {
        // `examples`（Map）与 `example` 互斥，取第一个带 value 的。
        let spec = r#"{
          "openapi": "3.0.0", "info": { "title": "t", "version": "1" },
          "paths": { "/a": { "post": {
            "requestBody": { "content": { "application/json": {
              "examples": { "first": { "value": { "id": 7 } } }
            } } },
            "responses": { "200": { "description": "ok" } } } } }
        }"#;
        assert_eq!(json_body(spec), serde_json::json!({ "id": 7 }));
    }

    #[test]
    fn schema_default_and_allof_are_synthesized() {
        let spec = r#"{
          "openapi": "3.0.0", "info": { "title": "t", "version": "1" },
          "paths": { "/a": { "post": {
            "requestBody": { "content": { "application/json": {
              "schema": { "allOf": [
                { "type": "object", "properties": { "a": { "type": "string", "default": "x" } } },
                { "type": "object", "properties": { "b": { "type": "integer" } } }
              ] }
            } } },
            "responses": { "200": { "description": "ok" } } } } }
        }"#;
        assert_eq!(json_body(spec), serde_json::json!({ "a": "x", "b": 0 }));
    }

    #[test]
    fn self_referential_schema_does_not_blow_the_stack() {
        // 深度上限兜底：即便文档里嵌套远超上限，也不能爆栈。
        // 程序化构造，避免手写括号出错。
        let mut schema = serde_json::json!({ "type": "object" });
        for _ in 0..40 {
            schema = serde_json::json!({
                "type": "object",
                "properties": { "self": schema }
            });
        }
        let spec = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "t", "version": "1" },
            "paths": { "/a": { "post": {
                "requestBody": { "content": { "application/json": { "schema": schema } } },
                "responses": { "200": { "description": "ok" } }
            } } }
        })
        .to_string();
        // 只要不 panic 即可；同时确认最外层字段存在。
        let body = json_body(&spec);
        assert!(body.get("self").is_some());
    }

    #[test]
    fn urlencoded_body_from_schema_properties() {
        let spec = r#"{
          "openapi": "3.0.0", "info": { "title": "t", "version": "1" },
          "paths": { "/login": { "post": {
            "requestBody": { "content": { "application/x-www-form-urlencoded": {
              "schema": { "type": "object", "properties": {
                "user": { "type": "string", "example": "admin" },
                "pass": { "type": "string" }
              } }
            } } },
            "responses": { "200": { "description": "ok" } } } } }
        }"#;
        let eps = import_endpoints(spec).unwrap();
        let ep = eps.iter().find(|e| e.method == HttpMethod::POST).unwrap();
        let BodySpec::UrlEncoded { fields } = &ep.request.body else {
            panic!("expected urlencoded, got {:?}", ep.request.body);
        };
        // 旧实现：没有 media.example 时 fields 为空 → 表单字段全丢。
        assert_eq!(fields.len(), 2);
        let by_key = |k: &str| fields.iter().find(|f| f.key == k).expect("字段缺失");
        assert_eq!(by_key("user").value, "admin");
        assert_eq!(by_key("pass").value, "");
    }

    #[test]
    fn openapi3_body_survives_the_full_import_any_path() {
        // 端到端：`import_any` 是 Tauri `import_document` 的后端入口，
        // 覆盖 detect → parse → map → body 的完整链路。
        let spec = r#"{
          "openapi": "3.0.0", "info": { "title": "t", "version": "1" },
          "paths": { "/posts": { "post": {
            "requestBody": { "content": { "application/json": { "schema": {
              "type": "object",
              "properties": {
                "title":  { "type": "string", "example": "小秦技术" },
                "userId": { "type": "integer", "example": 1 }
              }
            } } } },
            "responses": { "201": { "description": "ok" } } } } }
        }"#;
        let (eps, fmt) = import_any(spec).expect("import ok");
        assert_eq!(fmt, ImportFormat::OpenApi30);
        let ep = eps.iter().find(|e| e.method == HttpMethod::POST).unwrap();
        let BodySpec::Json { raw } = &ep.request.body else {
            panic!("expected json body");
        };
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(v["title"], "小秦技术");
        assert_eq!(v["userId"], 1);
    }

    #[test]
    fn request_body_reference_degrades_gracefully() {
        // `$ref` 不解析（拿不到 components），但不能 panic，且不返回字段。
        let spec = r##"{
          "openapi": "3.0.0", "info": { "title": "t", "version": "1" },
          "paths": { "/a": { "post": {
            "requestBody": { "content": { "application/json": {
              "schema": { "$ref": "#/components/schemas/Post" }
            } } },
            "responses": { "200": { "description": "ok" } } } } }
        }"##;
        assert_eq!(json_body(spec), serde_json::json!({}));
    }
}
