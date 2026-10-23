//! miyo-grpc 单元测试：protox 编译（含 import）、服务/方法解析、
//! protobuf-JSON 往返、metadata 构造与缓存——不依赖真实 gRPC 服务器（CI 可跑）。

use std::sync::Arc;

use miyo_grpc::{acquire_pool, compile_files, resolve_method, GrpcSource, ProtoFileInput};

/// 内置 fixture：user/v1/user.proto import 基础类型文件，含 unary 与服务端流方法。
fn fixture_files() -> Vec<ProtoFileInput> {
    vec![
        ProtoFileInput {
            id: "common".into(),
            name: "common/v1/common.proto".into(),
            content: r#"syntax = "proto3";
package common.v1;

message Page {
  int32 page = 1;
  int32 limit = 2;
}
"#
            .into(),
        },
        ProtoFileInput {
            id: "user".into(),
            name: "user/v1/user.proto".into(),
            content: r#"syntax = "proto3";
package user.v1;

import "common/v1/common.proto";

message GetUserRequest {
  string id = 1;
}

message User {
  string id = 1;
  string name = 2;
  common.v1.Page page_info = 3;
}

message ListEventsRequest {
  string id = 1;
}

message Event {
  string text = 1;
}

service UserService {
  rpc GetUser(GetUserRequest) returns (User);
  rpc ListEvents(ListEventsRequest) returns (stream Event);
}
"#
            .into(),
        },
    ]
}

#[test]
fn compile_files_resolves_cross_file_import() {
    let pool = compile_files(&fixture_files()).expect("编译应成功（import 从文件集解析）");
    let svc = pool
        .get_service_by_name("user.v1.UserService")
        .expect("服务应存在");
    let methods: Vec<String> = svc.methods().map(|m| m.name().to_string()).collect();
    assert_eq!(methods, vec!["GetUser", "ListEvents"]);
}

#[test]
fn compile_files_empty_set_errors() {
    let err = compile_files(&[]).unwrap_err();
    assert!(err.user_message().contains("未提供 proto 文件"));
}

#[test]
fn compile_files_syntax_error_reports_file_name() {
    let files = vec![ProtoFileInput {
        id: "bad".into(),
        name: "bad/bad.proto".into(),
        content: "syntax = \"proto3\"; this is not valid".into(),
    }];
    let err = compile_files(&files).unwrap_err();
    assert!(err.user_message().contains("bad/bad.proto"), "{err}");
}

#[test]
fn compile_files_missing_import_errors() {
    let files = vec![ProtoFileInput {
        id: "a".into(),
        name: "a.proto".into(),
        content: "syntax = \"proto3\";\nimport \"missing.proto\";".into(),
    }];
    assert!(compile_files(&files).is_err());
}

#[test]
fn resolve_method_reports_streaming_flags() {
    let pool = compile_files(&fixture_files()).unwrap();

    let (_input, _output, client_streaming, server_streaming) =
        resolve_method(&pool, "user.v1.UserService", "GetUser").expect("unary 方法");
    assert!(!client_streaming && !server_streaming);

    let (_input, _output, client_streaming, server_streaming) =
        resolve_method(&pool, "user.v1.UserService", "listevents").expect("服务端流方法");
    assert!(!client_streaming && server_streaming);

    // 大小写不敏感匹配
    let (input, output, _, _) = resolve_method(&pool, "user.v1.UserService", "getuser").unwrap();
    assert_eq!(input.full_name(), "user.v1.GetUserRequest");
    assert_eq!(output.full_name(), "user.v1.User");
}

#[test]
fn resolve_method_errors_on_missing() {
    let pool = compile_files(&fixture_files()).unwrap();
    let err = resolve_method(&pool, "no.SuchService", "M").unwrap_err();
    assert!(err.user_message().contains("服务不存在"));
    let err = resolve_method(&pool, "user.v1.UserService", "Nope").unwrap_err();
    assert!(err.user_message().contains("方法不存在"));
}

/// DynamicMessage 的 protobuf-JSON 反序列化 → 序列化往返（含嵌套消息）。
#[test]
fn dynamic_message_json_roundtrip() {
    use prost_reflect::DynamicMessage;

    let pool = compile_files(&fixture_files()).unwrap();
    let (input, _output, _, _) = resolve_method(&pool, "user.v1.UserService", "GetUser").unwrap();

    let json = serde_json::json!({ "id": "u-1" });
    let msg = DynamicMessage::deserialize(input.clone(), json).expect("反序列化");
    assert_eq!(msg.get_field_by_name("id").unwrap().as_str(), Some("u-1"));

    let out = serde_json::to_value(&msg).expect("序列化");
    assert_eq!(out["id"], "u-1");
}

#[test]
fn proto_file_input_serde_shape() {
    // ProtoFileInput 序列化形状锁定（命令层与前端传递约定）
    let f = ProtoFileInput {
        id: "id-1".into(),
        name: "user/v1/user.proto".into(),
        content: "syntax = \"proto3\";".into(),
    };
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["id"], "id-1");
    assert_eq!(v["name"], "user/v1/user.proto");
    assert_eq!(v["content"], "syntax = \"proto3\";");
}

/// 描述符池缓存：同内容命中缓存（Arc 指针一致），force_reload 重新编译。
#[tokio::test]
async fn pool_cache_hits_and_force_reload() {
    let files = fixture_files();
    let a = acquire_pool(GrpcSource::Files(files.clone()), "x:1", false, &[], false)
        .await
        .expect("首次编译");
    let b = acquire_pool(GrpcSource::Files(files.clone()), "x:1", false, &[], false)
        .await
        .expect("二次获取");
    assert!(Arc::ptr_eq(&a, &b), "同内容应命中缓存");

    let c = acquire_pool(GrpcSource::Files(files), "x:1", false, &[], true)
        .await
        .expect("强制重编译");
    assert!(!Arc::ptr_eq(&a, &c), "force_reload 应重新编译");
}

/// 取消令牌先于调用触发 → 即使地址网络不可达（connect 会挂）也必须立即
/// 返回 Cancelled。回归锁定：取消令牌覆盖整个调用周期（含 connect），
/// 而非只包住最终的 unary 调用——否则「点取消仍在请求中」。
#[tokio::test]
async fn invoke_unary_cancelled_before_connect_returns_immediately() {
    use miyo_grpc::{invoke_unary, GrpcInvokeArgs};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    let args = GrpcInvokeArgs {
        // 不可路由地址：TCP connect 无限挂起（若取消未覆盖 connect 阶段，
        // 本测试会卡死直到外层超时——测试失败可见）
        address: "10.255.255.1:65535".into(),
        service: "user.v1.UserService".into(),
        method: "GetUser".into(),
        message_json: "{}".into(),
        metadata: vec![],
        use_tls: false,
        timeout_ms: None,
        pool: Arc::new(compile_files(&fixture_files()).expect("fixture 编译")),
    };
    let token = CancellationToken::new();
    token.cancel();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        invoke_unary(&args, Some(&token)),
    )
    .await
    .expect("取消后 5 秒内必须返回（不得卡在 connect）");
    let err = result.expect_err("已取消的调用必须返回错误");
    assert!(err.user_message().contains("取消"), "错误应为取消类：{err}");
}

/// connect 硬超时：网络不可达时 connect 最多挂 10 秒后报连接错误，
/// 不允许无限挂起（无取消令牌的裸调用也有上界）。
#[tokio::test]
async fn connect_unreachable_bounded_by_timeout() {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        miyo_grpc::connect("10.255.255.1:65535", false),
    )
    .await
    .expect("connect 须在 15 秒内返回（硬超时 10 秒兜底）");
    assert!(result.is_err(), "不可达地址必须连接失败");
}
