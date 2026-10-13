//! 开发环境种子数据（仅 debug 构建参与编译）。
//!
//! `npm run tauri dev` 每次启动：`db::reset_dev_database()` 先删库文件，
//! 迁移重建后由本模块写入一批开箱可测的项目 / 接口 / 环境 / Mock 规则，
//! 保证每次重启都是干净一致的测试数据集。release 构建不编译本模块，
//! 正式数据完全不受影响。
//!
//! 数据集以「小奏技术」为演示品牌，项目概览：
//! - 项目 1「小奏技术 · 用户服务」：账号 / 鉴权 REST 接口 + 配套 Mock 规则
//!   （启动 Mock 后基址 http://127.0.0.1:4010 可直接调试）；
//! - 项目 2「小奏技术 · 开放演示」：公网真实 API（JSONPlaceholder），无需 Mock 即可直接发送；
//! - 项目 3「小奏技术 · GraphQL 网关」：公共 GraphQL 服务，测试 GraphQL 工作台；
//! - 项目 2 内含 gRPC 端点：公共反射演示服务（grpcb.in），测试 gRPC 调试；
//! - 环境：每个项目各自的「开发环境 / 测试环境」（单一 Base URL + 变量）、
//!   全局参数、激活项 settings。

use std::collections::HashMap;

use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use fox_core::model::{
    BodySpec, Endpoint, EndpointStatus, Environment, EnvironmentVariable, Folder, GlobalParam,
    GlobalParamLocation, GraphQLSpec, GrpcSpec, HttpMethod, KeyValue, MockMatchItem, MockRule,
    Project, RequestExample, RequestSpec, TestCase, TestCaseStatus,
};
use fox_core::Result;

use crate::repository as repo;

/// 与 fox-tauri/src/state.rs 的 KEY_ACTIVE_PROJECT 一致；
/// state.rs 未导出常量，此处保持字面量同步。
/// 激活环境按项目记忆：键为 `active_environment_id:{project_id}`。
const KEY_ACTIVE_PROJECT: &str = "active_project_id";
const KEY_ACTIVE_ENVIRONMENT_PREFIX: &str = "active_environment_id:";

/// 开发启动时清库后的种子写入入口。
pub async fn seed_dev_data(db: &SqlitePool) -> Result<()> {
    // ---- 团队（默认团队由迁移 0015 创建，这里补两个演示团队） ----
    let team_platform = repo::create_team(db, "小奏技术 · 平台组").await?;
    let team_business = repo::create_team(db, "小奏技术 · 业务组").await?;

    // ---- 项目 ----
    let users = project(
        "小奏技术 · 用户服务",
        "小奏技术内部用户服务演示：账号与鉴权接口。启动 Mock 服务后，\
         开发环境下基址 http://127.0.0.1:4010 的接口可直接调试。",
        Some(team_platform.id),
    );
    let open_demo = project(
        "小奏技术 · 开放演示",
        "小奏技术开放 API 演示：后端指向公网 JSONPlaceholder（jsonplaceholder.typicode.com），\
         无需 Mock 即可直接发送体验。",
        Some(team_business.id),
    );
    let graphql = project(
        "小奏技术 · GraphQL 网关",
        "小奏技术 GraphQL 网关演示：接入公共 GraphQL 服务（countries.trevorblades.com），用于测试 GraphQL 工作台。",
        None, // 默认团队
    );
    for p in [&users, &open_demo, &graphql] {
        repo::save_project(db, p).await?;
    }

    // ---- 轻量演示项目（每团队补几个，凑足分组展示的数据量） ----
    // 只给 2-3 个真实公网端点，不铺 Mock / 测试用例，保持种子数据精简。
    for (name, desc, team_id, endpoints) in [
        (
            "小奏技术 · 网关服务",
            "API 网关配置演示：路由与限流接口（指向 JSONPlaceholder 占位）。",
            Some(team_platform.id),
            vec![
                ("GET", "/routes", "网关路由列表"),
                ("POST", "/routes", "新增路由"),
            ],
        ),
        (
            "小奏技术 · 监控告警",
            "监控告警演示：告警规则 CRUD（指向 httpbin.org 公网调试服务）。",
            Some(team_platform.id),
            vec![
                ("GET", "/alerts", "告警规则列表"),
                ("PUT", "/alerts/{id}", "更新告警规则"),
            ],
        ),
        (
            "小奏技术 · 支付中心",
            "支付中心演示：下单 / 退款接口（指向 jsonplaceholder 占位）。",
            Some(team_business.id),
            vec![
                ("POST", "/orders", "创建支付订单"),
                ("POST", "/refunds", "发起退款"),
            ],
        ),
        (
            "小奏技术 · 营销活动",
            "营销活动演示：活动投放接口（指向 httpbin.org 公网调试服务）。",
            Some(team_business.id),
            vec![
                ("GET", "/campaigns", "活动列表"),
                ("POST", "/campaigns", "创建活动"),
            ],
        ),
        (
            "小奏技术 · 数据大屏",
            "数据大屏演示：指标查询接口（指向 JSONPlaceholder 占位）。",
            None, // 默认团队
            vec![
                ("GET", "/metrics", "大屏指标聚合"),
                ("GET", "/metrics/trend", "趋势数据"),
            ],
        ),
        (
            "小奏技术 · 文档中心",
            "文档中心演示：文档发布接口（指向 httpbin.org 公网调试服务）。",
            None, // 默认团队
            vec![("GET", "/docs", "文档列表"), ("POST", "/docs", "发布文档")],
        ),
    ] {
        let p = project(name, desc, team_id);
        repo::save_project(db, &p).await?;
        for (i, (method, path, ep_desc)) in endpoints.into_iter().enumerate() {
            let method = match method {
                "POST" => HttpMethod::POST,
                "PUT" => HttpMethod::PUT,
                _ => HttpMethod::GET,
            };
            repo::save_endpoint(
                db,
                &endpoint(
                    &p,
                    None,
                    ep_desc,
                    method,
                    path,
                    ep_desc,
                    EndpointStatus::Released,
                    i as i64,
                    RequestSpec::default(),
                ),
            )
            .await?;
        }
    }

    // ---- 用户服务：文件夹 + 接口 + 响应示例 + Mock 规则 ----
    let account_folder = folder(&users, None, "账号管理");
    let auth_folder = folder(&users, None, "鉴权");
    for f in [&account_folder, &auth_folder] {
        repo::save_folder(db, f).await?;
    }

    let list_users = endpoint(
        &users,
        Some(&account_folder),
        "用户列表",
        HttpMethod::GET,
        "/users",
        "分页查询小奏技术账号，Query 参数 page / limit",
        EndpointStatus::Released,
        0,
        RequestSpec {
            params: vec![kv("page", "1"), kv("limit", "10")],
            ..RequestSpec::default()
        },
    );
    let get_user = endpoint(
        &users,
        Some(&account_folder),
        "用户详情",
        HttpMethod::GET,
        "/users/{id}",
        "路径变量 {id}；Mock 规则内置 /users/1 示例",
        EndpointStatus::Released,
        1,
        RequestSpec {
            path_variables: vec![kv("id", "1")],
            ..RequestSpec::default()
        },
    );
    let create_user = endpoint(
        &users,
        Some(&account_folder),
        "创建用户",
        HttpMethod::POST,
        "/users",
        "JSON Body：新增小奏技术账号",
        EndpointStatus::Testing,
        2,
        json_body(r#"{"name": "奏小新", "email": "xinxin@xiaozou.tech", "dept": "设计部"}"#),
    );
    let update_user = endpoint(
        &users,
        Some(&account_folder),
        "更新用户",
        HttpMethod::PUT,
        "/users/{id}",
        "JSON Body + 路径变量",
        EndpointStatus::Developing,
        3,
        json_body(r#"{"name": "奏小雪", "dept": "架构组"}"#),
    );
    let delete_user = endpoint(
        &users,
        Some(&account_folder),
        "删除用户",
        HttpMethod::DELETE,
        "/users/{id}",
        "路径变量 {id}",
        EndpointStatus::Developing,
        4,
        RequestSpec {
            path_variables: vec![kv("id", "1")],
            ..RequestSpec::default()
        },
    );
    let login = endpoint(
        &users,
        Some(&auth_folder),
        "登录",
        HttpMethod::POST,
        "/auth/login",
        "表单（urlencoded）提交 username / password，返回小奏技术 token",
        EndpointStatus::Testing,
        5,
        RequestSpec {
            body: BodySpec::UrlEncoded {
                fields: vec![kv("username", "demo"), kv("password", "xiaozou123")],
            },
            ..RequestSpec::default()
        },
    );
    let me = endpoint(
        &users,
        Some(&auth_folder),
        "当前用户",
        HttpMethod::GET,
        "/auth/me",
        "Bearer {{token}} 认证示例（token 来自环境变量）",
        EndpointStatus::Developing,
        6,
        RequestSpec {
            auth: fox_core::model::AuthSpec::Bearer {
                token: "{{token}}".to_string(),
            },
            ..RequestSpec::default()
        },
    );
    for e in [
        &list_users,
        &get_user,
        &create_user,
        &update_user,
        &delete_user,
        &login,
        &me,
    ] {
        repo::save_endpoint(db, e).await?;
    }

    // 响应示例（文档页展示 + Mock 快速填充的素材）
    for example in [
        response_example(
            &list_users,
            "200 成功",
            200,
            r#"[{"id": 1, "name": "奏小雪", "email": "xuexue@xiaozou.tech", "dept": "研发部"}, {"id": 2, "name": "奏小风", "email": "xiaofeng@xiaozou.tech", "dept": "产品部"}]"#,
        ),
        response_example(
            &get_user,
            "200 成功",
            200,
            r#"{"id": 1, "name": "奏小雪", "email": "xuexue@xiaozou.tech", "dept": "研发部", "company": "小奏技术"}"#,
        ),
        response_example(
            &create_user,
            "201 已创建",
            201,
            r#"{"id": 1001, "name": "奏小新", "email": "xinxin@xiaozou.tech", "dept": "设计部", "company": "小奏技术"}"#,
        ),
        response_example(&delete_user, "204 无内容", 204, ""),
        response_example(
            &login,
            "200 成功",
            200,
            r#"{"token": "xz-mock-token-666", "expires_in": 3600, "company": "小奏技术"}"#,
        ),
    ] {
        repo::save_response_example(db, &example).await?;
    }

    // Mock 规则（启动 Mock 服务后，http://127.0.0.1:4010 直接命中）
    for rule in [
        mock_rule(
            &users,
            Some(&list_users),
            "用户列表",
            HttpMethod::GET,
            "/users",
            200,
            r#"[{"id": 1, "name": "奏小雪", "email": "xuexue@xiaozou.tech", "dept": "研发部"}, {"id": 2, "name": "奏小风", "email": "xiaofeng@xiaozou.tech", "dept": "产品部"}]"#,
            100,
        ),
        mock_rule(
            &users,
            Some(&get_user),
            "用户详情",
            HttpMethod::GET,
            "/users/1",
            200,
            r#"{"id": 1, "name": "奏小雪", "email": "xuexue@xiaozou.tech", "dept": "研发部", "company": "小奏技术"}"#,
            0,
        ),
        mock_rule(
            &users,
            Some(&create_user),
            "创建用户",
            HttpMethod::POST,
            "/users",
            201,
            r#"{"id": 1001, "name": "奏小新", "email": "xinxin@xiaozou.tech", "dept": "设计部", "company": "小奏技术"}"#,
            0,
        ),
        mock_rule(
            &users,
            Some(&login),
            "登录",
            HttpMethod::POST,
            "/auth/login",
            200,
            r#"{"token": "xz-mock-token-666", "expires_in": 3600, "company": "小奏技术"}"#,
            200,
        ),
        mock_rule(
            &users,
            Some(&delete_user),
            "删除用户",
            HttpMethod::DELETE,
            "/users/1",
            204,
            "",
            0,
        ),
    ] {
        repo::save_mock_rule(db, &rule).await?;
    }

    // 高级 Mock 规则：query 精确匹配 / 故障注入 / 当前用户（演示匹配优先级与故障注入）
    for rule in [
        mock_rule_ex(
            &users,
            Some(&list_users),
            "小奏技术·page=7 精确匹配",
            HttpMethod::GET,
            "/users",
            200,
            r#"{"page": 7, "list": [{"id": 7, "name": "奏小柒", "email": "qiqi@xiaozou.tech", "dept": "测试部", "company": "小奏技术"}]}"#,
            0,
            vec![MockMatchItem {
                key: "page".into(),
                value: "7".into(),
            }],
            20,
            0,
        ),
        mock_rule_ex(
            &users,
            Some(&create_user),
            "小奏技术·创建用户故障注入（30% 5xx）",
            HttpMethod::POST,
            "/users",
            201,
            r#"{"id": 1001, "name": "奏小新", "email": "xinxin@xiaozou.tech", "dept": "设计部", "company": "小奏技术"}"#,
            0,
            vec![],
            20,
            30,
        ),
        mock_rule_ex(
            &users,
            Some(&me),
            "小奏技术·当前用户",
            HttpMethod::GET,
            "/auth/me",
            200,
            r#"{"id": 1, "name": "奏小雪", "company": "小奏技术", "role": "admin", "issuedBy": "RustFox Mock"}"#,
            0,
            vec![],
            0,
            0,
        ),
    ] {
        repo::save_mock_rule(db, &rule).await?;
    }

    // ---- 开放演示：真实公网接口（JSONPlaceholder） ----
    // 收集已保存端点（名称 → 端点），供测试用例 / 请求用例挂载引用
    let mut open_endpoints: HashMap<String, Endpoint> = HashMap::new();
    for (i, (name, method, path, desc, status, request)) in [
        (
            "文章列表",
            HttpMethod::GET,
            "/posts",
            "Query 参数 _limit / _page",
            EndpointStatus::Released,
            RequestSpec {
                params: vec![kv("_limit", "5"), kv("_page", "1")],
                ..RequestSpec::default()
            },
        ),
        (
            "文章详情",
            HttpMethod::GET,
            "/posts/1",
            "按 id 查询",
            EndpointStatus::Released,
            RequestSpec::default(),
        ),
        (
            "发布文章",
            HttpMethod::POST,
            "/posts",
            "JSON Body（服务端返回 201 模拟创建）",
            EndpointStatus::Testing,
            json_body(
                r#"{"title": "小奏技术", "body": "由小奏技术调试工具 RustFox 发送", "userId": 1}"#,
            ),
        ),
        (
            "更新文章",
            HttpMethod::PUT,
            "/posts/1",
            "JSON Body 整体更新",
            EndpointStatus::Developing,
            json_body(
                r#"{"id": 1, "title": "小奏技术周报", "body": "本周小奏技术动态", "userId": 1}"#,
            ),
        ),
        (
            "删除文章",
            HttpMethod::DELETE,
            "/posts/1",
            "服务端返回 200 + 空对象（模拟删除）",
            EndpointStatus::Developing,
            RequestSpec::default(),
        ),
        (
            "用户详情",
            HttpMethod::GET,
            "/users/1",
            "嵌套资源示例",
            EndpointStatus::Released,
            RequestSpec::default(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let ep = endpoint(
            &open_demo, None, name, method, path, desc, status, i as i64, request,
        );
        repo::save_endpoint(db, &ep).await?;
        open_endpoints.insert(name.to_string(), ep);
    }

    // ---- GraphQL：公共接口 ----
    let mut graphql_endpoints: HashMap<String, Endpoint> = HashMap::new();
    for (i, (name, desc, query, variables)) in [
        (
            "国家列表",
            "POST { countries { code name emoji capital } }",
            "query { countries { code name emoji capital } }",
            "",
        ),
        (
            "国家详情（带变量）",
            "带 $code 变量的查询，variables 里改 code 试试",
            "query Country($code: ID!) { country(code: $code) { code name emoji capital currency phone } }",
            r#"{"code": "US"}"#,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let ep = endpoint(
            &graphql,
            None,
            name,
            HttpMethod::POST,
            "/graphql",
            desc,
            EndpointStatus::Released,
            i as i64,
            RequestSpec {
                body: BodySpec::GraphQL {
                    spec: GraphQLSpec {
                        query: query.to_string(),
                        variables: variables.to_string(),
                        operation_name: String::new(),
                    },
                },
                ..RequestSpec::default()
            },
        );
        repo::save_endpoint(db, &ep).await?;
        graphql_endpoints.insert(name.to_string(), ep);
    }

    // ---- gRPC：公共反射演示服务（grpcb.in：免 proto 文件，走服务端反射） ----
    for (i, (name, desc, address, spec)) in [
        (
            "gRPC 回显调用（unary）",
            "grpcbin.GRPCBin/DummyUnary：服务端反射免 proto 文件，回显请求消息；元数据（请求头）会被服务端记录",
            "grpcb.in:9000",
            GrpcSpec {
                service: "grpcbin.GRPCBin".into(),
                method: "DummyUnary".into(),
                message: r#"{"f_string": "小奏技术"}"#.into(),
                use_tls: false,
                proto_ids: vec![],
            },
        ),
        (
            "gRPC 服务端流",
            "hello.HelloService/LotsOfReplies：一次请求，服务端流回十条问候消息",
            "grpcb.in:9000",
            GrpcSpec {
                service: "hello.HelloService".into(),
                method: "LotsOfReplies".into(),
                message: r#"{"greeting": "小奏技术"}"#.into(),
                use_tls: false,
                proto_ids: vec![],
            },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        repo::save_endpoint(
            db,
            &endpoint(
                &open_demo,
                None,
                name,
                HttpMethod::Grpc,
                address,
                desc,
                EndpointStatus::Released,
                100 + i as i64,
                RequestSpec {
                    headers: vec![kv("x-demo", "rustfox")],
                    body: BodySpec::Grpc { spec },
                    ..RequestSpec::default()
                },
            ),
        )
        .await?;
    }

    // ---- 测试用例（Apifox 风格用例管理；分类为存库数据值，展示经 caseCategoryLabel 翻译） ----
    for case in [
        // 用户服务（启动 Mock 后可整组运行）
        test_case(
            &list_users,
            "正向-小奏技术默认分页",
            "正向",
            HttpMethod::GET,
            "/users",
            vec![kv("page", "1"), kv("limit", "10")],
            vec![],
            "none",
            "",
        ),
        test_case(
            &list_users,
            "边界值-limit 上限探测",
            "边界值",
            HttpMethod::GET,
            "/users",
            vec![kv("page", "1"), kv("limit", "1000")],
            vec![],
            "none",
            "",
        ),
        test_case(
            &get_user,
            "正向-查询奏小雪",
            "正向",
            HttpMethod::GET,
            "/users/1",
            vec![],
            vec![],
            "none",
            "",
        ),
        test_case(
            &get_user,
            "负向-不存在的用户",
            "负向",
            HttpMethod::GET,
            "/users/99999",
            vec![],
            vec![],
            "none",
            "",
        ),
        test_case(
            &login,
            "正向-小奏管理员登录",
            "正向",
            HttpMethod::POST,
            "/auth/login",
            vec![],
            vec![],
            "urlencoded",
            r#"{"username": "xiaozou_admin", "password": "xz@2024"}"#,
        ),
        test_case(
            &login,
            "负向-空密码登录",
            "负向",
            HttpMethod::POST,
            "/auth/login",
            vec![],
            vec![],
            "urlencoded",
            r#"{"username": "xiaozou_admin", "password": ""}"#,
        ),
        test_case(
            &create_user,
            "正向-新增小奏技术成员",
            "正向",
            HttpMethod::POST,
            "/users",
            vec![],
            vec![],
            "json",
            r#"{"name": "奏小新", "email": "xinxin@xiaozou.tech", "dept": "研发部"}"#,
        ),
        test_case(
            &me,
            "安全性-伪造 Token 访问",
            "安全性",
            HttpMethod::GET,
            "/auth/me",
            vec![],
            vec![kv("Authorization", "Bearer xz-forged-token")],
            "none",
            "",
        ),
        // 开放演示（公网 JSONPlaceholder，直接可跑）
        test_case(
            open_endpoints.get("文章列表").expect("seed 文章列表端点"),
            "正向-小奏技术文章分页",
            "正向",
            HttpMethod::GET,
            "/posts",
            vec![kv("_limit", "5"), kv("_page", "2")],
            vec![],
            "none",
            "",
        ),
        test_case(
            open_endpoints.get("发布文章").expect("seed 发布文章端点"),
            "正向-发布小奏技术周报",
            "正向",
            HttpMethod::POST,
            "/posts",
            vec![],
            vec![],
            "json",
            r#"{"title": "小奏技术周报 #42", "body": "本周小奏技术动态：gRPC 调试上线", "userId": 1}"#,
        ),
        // GraphQL 网关（body_type=graphql，body_content 为查询文本）
        test_case(
            graphql_endpoints
                .get("国家详情（带变量）")
                .expect("seed 国家详情端点"),
            "正向-查询小奏技术所在国家",
            "正向",
            HttpMethod::POST,
            "/graphql",
            vec![],
            vec![],
            "graphql",
            r#"query { country(code: "CN") { name emoji capital } }"#,
        ),
    ] {
        repo::create_test_case(db, &case).await?;
    }

    // ---- 请求用例（请求快照，一键回填调试页） ----
    for example in [
        request_example_snap(&list_users, "小奏技术·第一页", RequestSpec {
            params: vec![kv("page", "1"), kv("limit", "10")],
            ..RequestSpec::default()
        }),
        request_example_snap(&list_users, "小奏技术·每页 50 条", RequestSpec {
            params: vec![kv("page", "1"), kv("limit", "50")],
            ..RequestSpec::default()
        }),
        request_example_snap(&login, "小奏管理员账号", RequestSpec {
            body: BodySpec::UrlEncoded {
                fields: vec![kv("username", "xiaozou_admin"), kv("password", "xz@2024")],
            },
            ..RequestSpec::default()
        }),
        request_example_snap(&create_user, "小奏技术新成员（研发部）", RequestSpec {
            body: BodySpec::Json {
                raw: r#"{"name": "奏小新", "email": "xinxin@xiaozou.tech", "dept": "研发部"}"#.into(),
            },
            ..RequestSpec::default()
        }),
        request_example_snap(
            open_endpoints.get("更新文章").expect("seed 更新文章端点"),
            "小奏技术周报样例",
            RequestSpec {
                body: BodySpec::Json {
                    raw: r#"{"id": 1, "title": "小奏技术周报", "body": "本周小奏技术动态", "userId": 1}"#.into(),
                },
                ..RequestSpec::default()
            },
        ),
    ] {
        repo::create_request_example(db, &example).await?;
    }

    // ---- 环境（项目维度：每个项目各自的开发 / 测试环境 + 单一 Base URL） ----
    let users_dev = environment(
        &users,
        "开发环境",
        "http://127.0.0.1:4010",
        vec![
            env_var(
                "token",
                "xz-dev-token-123",
                "小奏技术登录接口返回的 Bearer Token 示例",
            ),
            env_var("env_name", "development", "当前环境标识"),
            env_var("trace_id", "xz-trace-dev-001", "链路追踪 ID 示例"),
        ],
    );
    let users_test = environment(
        &users,
        "测试环境",
        "http://127.0.0.1:4010",
        vec![
            env_var(
                "token",
                "xz-test-token-456",
                "小奏技术登录接口返回的 Bearer Token 示例",
            ),
            env_var("env_name", "staging", "当前环境标识"),
            env_var("trace_id", "xz-trace-stg-001", "链路追踪 ID 示例"),
        ],
    );
    let open_dev = environment(
        &open_demo,
        "开发环境",
        "https://jsonplaceholder.typicode.com",
        vec![
            env_var(
                "token",
                "xz-dev-token-123",
                "小奏技术登录接口返回的 Bearer Token 示例",
            ),
            env_var("env_name", "development", "当前环境标识"),
            env_var("trace_id", "xz-trace-dev-001", "链路追踪 ID 示例"),
        ],
    );
    let open_test = environment(
        &open_demo,
        "测试环境",
        "https://jsonplaceholder.typicode.com",
        vec![
            env_var(
                "token",
                "xz-test-token-456",
                "小奏技术登录接口返回的 Bearer Token 示例",
            ),
            env_var("env_name", "staging", "当前环境标识"),
            env_var("trace_id", "xz-trace-stg-001", "链路追踪 ID 示例"),
        ],
    );
    let gql_dev = environment(
        &graphql,
        "开发环境",
        "https://countries.trevorblades.com",
        vec![
            env_var(
                "token",
                "xz-dev-token-123",
                "小奏技术登录接口返回的 Bearer Token 示例",
            ),
            env_var("env_name", "development", "当前环境标识"),
            env_var("trace_id", "xz-trace-dev-001", "链路追踪 ID 示例"),
        ],
    );
    let gql_test = environment(
        &graphql,
        "测试环境",
        "https://countries.trevorblades.com",
        vec![
            env_var(
                "token",
                "xz-test-token-456",
                "小奏技术登录接口返回的 Bearer Token 示例",
            ),
            env_var("env_name", "staging", "当前环境标识"),
            env_var("trace_id", "xz-trace-stg-001", "链路追踪 ID 示例"),
        ],
    );
    for env in [
        &users_dev,
        &users_test,
        &open_dev,
        &open_test,
        &gql_dev,
        &gql_test,
    ] {
        repo::save_environment(db, env).await?;
    }

    // ---- 全局参数（注入制演示） ----
    repo::save_global_params(
        db,
        &[
            GlobalParam {
                key: "X-Client".to_string(),
                value: "XiaoZouTech-RustFox".to_string(),
                enabled: true,
                location: GlobalParamLocation::Header,
            },
            GlobalParam {
                key: "trace_id".to_string(),
                value: "xz-seed-trace-001".to_string(),
                enabled: false,
                location: GlobalParamLocation::Query,
            },
        ],
    )
    .await?;

    // ---- 激活项：启动即落在「小奏技术 · 开放演示」（jsonplaceholder 公网接口）+ 其开发环境 ----
    // 默认激活公网项目而非本地 Mock（127.0.0.1:4010），保证 dev 开箱即可直接发送请求；
    // 需要本地 Mock 演示时切到「小奏技术 · 用户服务」并启动 Mock 服务即可。
    repo::set_setting(db, KEY_ACTIVE_PROJECT, &setting_id(&open_demo.id)).await?;
    repo::set_setting(
        db,
        &format!("{KEY_ACTIVE_ENVIRONMENT_PREFIX}{}", open_demo.id),
        &setting_id(&open_dev.id),
    )
    .await?;

    Ok(())
}

/* ---------- 构造辅助 ---------- */

fn project(name: &str, description: &str, team_id: Option<Uuid>) -> Project {
    let now = Utc::now();
    Project {
        id: Uuid::new_v4(),
        name: name.to_string(),
        description: description.to_string(),
        variables: HashMap::new(),
        team_id: team_id.or(Some(fox_core::model::DEFAULT_TEAM_ID)),
        created_at: now,
        updated_at: now,
    }
}

fn folder(project: &Project, parent: Option<&Folder>, name: &str) -> Folder {
    let now = Utc::now();
    Folder {
        id: Uuid::new_v4(),
        project_id: project.id,
        parent_id: parent.map(|f| f.id),
        name: name.to_string(),
        sort_order: 0,
        created_at: now,
        updated_at: now,
    }
}

#[allow(clippy::too_many_arguments)]
fn endpoint(
    project: &Project,
    folder: Option<&Folder>,
    name: &str,
    method: HttpMethod,
    path: &str,
    description: &str,
    status: EndpointStatus,
    sort_order: i64,
    request: RequestSpec,
) -> Endpoint {
    let now = Utc::now();
    Endpoint {
        id: Uuid::new_v4(),
        project_id: project.id,
        folder_id: folder.map(|f| f.id),
        name: name.to_string(),
        method,
        path: path.to_string(),
        description: description.to_string(),
        status,
        sort_order,
        request,
        created_at: now,
        updated_at: now,
    }
}

fn response_example(
    endpoint: &Endpoint,
    name: &str,
    status: u16,
    body: &str,
) -> fox_core::model::ResponseExample {
    let now = Utc::now();
    fox_core::model::ResponseExample {
        id: Uuid::new_v4(),
        endpoint_id: endpoint.id,
        name: name.to_string(),
        status,
        headers: HashMap::new(),
        body: body.to_string(),
        content_type: "application/json".to_string(),
        docs: HashMap::new(),
        created_at: now,
        updated_at: now,
    }
}

#[allow(clippy::too_many_arguments)]
fn mock_rule(
    project: &Project,
    endpoint: Option<&Endpoint>,
    name: &str,
    method: HttpMethod,
    path: &str,
    status: u16,
    body: &str,
    delay_ms: u64,
) -> MockRule {
    let now = Utc::now();
    MockRule {
        id: Uuid::new_v4(),
        project_id: project.id,
        endpoint_id: endpoint.map(|e| e.id),
        name: name.to_string(),
        method,
        path: path.to_string(),
        match_query: Vec::<MockMatchItem>::new(),
        match_headers: Vec::<MockMatchItem>::new(),
        response_status: status,
        response_headers: HashMap::new(),
        response_body_template: body.to_string(),
        delay_ms,
        fault_rate_pct: 0,
        fault_status: 500,
        enabled: true,
        priority: 0,
        created_at: now,
        updated_at: now,
    }
}

fn environment(
    project: &Project,
    name: &str,
    base_url: &str,
    variables: Vec<EnvironmentVariable>,
) -> Environment {
    let now = Utc::now();
    Environment {
        id: Uuid::new_v4(),
        project_id: project.id,
        name: name.to_string(),
        base_url: base_url.to_string(),
        variables,
        created_at: now,
        updated_at: now,
    }
}

fn env_var(key: &str, value: &str, description: &str) -> EnvironmentVariable {
    EnvironmentVariable {
        key: key.to_string(),
        remote_value: value.to_string(),
        local_value: String::new(),
        enabled: true,
        description: Some(description.to_string()),
    }
}

fn kv(key: &str, value: &str) -> KeyValue {
    KeyValue::new(key, value)
}

fn json_body(raw: &str) -> RequestSpec {
    RequestSpec {
        body: BodySpec::Json {
            raw: raw.to_string(),
        },
        ..RequestSpec::default()
    }
}

/// 与 fox-tauri state.rs `setting_value` 一致：JSON 字符串 `"uuid"`。
fn setting_id(id: &Uuid) -> String {
    serde_json::to_string(&id.to_string()).unwrap_or_else(|_| "null".into())
}

/// 测试用例（挂到主接口；分类为存库数据值：正向/负向/边界值/安全性）。
#[allow(clippy::too_many_arguments)]
fn test_case(
    endpoint: &Endpoint,
    name: &str,
    category: &str,
    method: HttpMethod,
    url_path: &str,
    params: Vec<KeyValue>,
    headers: Vec<KeyValue>,
    body_type: &str,
    body_content: &str,
) -> TestCase {
    TestCase {
        id: Uuid::new_v4(),
        request_id: endpoint.id,
        name: name.to_string(),
        category: category.to_string(),
        method,
        url_path: url_path.to_string(),
        params,
        headers,
        body_type: body_type.to_string(),
        body_content: body_content.to_string(),
        last_run_status: TestCaseStatus::Untested,
        created_at: Utc::now(),
    }
}

/// 请求用例（当前请求快照，可一键回填调试页）。
fn request_example_snap(endpoint: &Endpoint, name: &str, request: RequestSpec) -> RequestExample {
    let now = Utc::now();
    RequestExample {
        id: Uuid::new_v4(),
        endpoint_id: endpoint.id,
        name: name.to_string(),
        request,
        created_at: now,
        updated_at: now,
    }
}

/// 高级 Mock 规则：支持 query 精确匹配 / 优先级 / 故障注入（基础版 mock_rule 的超集）。
#[allow(clippy::too_many_arguments)]
fn mock_rule_ex(
    project: &Project,
    endpoint: Option<&Endpoint>,
    name: &str,
    method: HttpMethod,
    path: &str,
    status: u16,
    body: &str,
    delay_ms: u64,
    match_query: Vec<MockMatchItem>,
    priority: i64,
    fault_rate_pct: u8,
) -> MockRule {
    MockRule {
        match_query,
        priority,
        fault_rate_pct,
        ..mock_rule(
            project, endpoint, name, method, path, status, body, delay_ms,
        )
    }
}
