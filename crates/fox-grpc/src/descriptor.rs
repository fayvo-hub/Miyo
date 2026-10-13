//! proto 描述符获取：项目内 .proto 文件运行时编译（protox）与服务端反射
//! （tonic-reflection v1），统一产出 [`DescriptorPool`] 并按内容缓存。
//!
//! 两条路径都免代码生成：编译产物直接给 prost-reflect 动态消息使用。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use fox_core::{AppError, Result};
use prost::Message;
use prost_reflect::DescriptorPool;

/// 项目级 proto 文件（id 供端点 GrpcSpec.proto_ids 引用）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProtoFileInput {
    pub id: String,
    pub name: String,
    pub content: String,
}

/// 描述符来源：服务端反射（免 proto 文件）或项目内 proto 文件集。
#[derive(Debug, Clone)]
pub enum GrpcSource {
    Reflection,
    Files(Vec<ProtoFileInput>),
}

/// 单个方法信息（前端服务/方法选择器数据源）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GrpcMethodInfo {
    pub method: String,
    /// 输入消息全名（前端 JSON 占位提示用）。
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

/// 单个服务信息。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GrpcServiceInfo {
    /// 全限定服务名，如 `user.v1.UserService`。
    pub service: String,
    pub methods: Vec<GrpcMethodInfo>,
}

/// 服务目录（grpc_list_services 命令返回体）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ServiceCatalog {
    pub services: Vec<GrpcServiceInfo>,
    /// 目录来源（reflection / proto），前端展示用。
    pub source: String,
}

impl ServiceCatalog {
    /// 从描述符池提取服务/方法目录。
    pub fn from_pool(pool: &DescriptorPool, source: &str) -> Self {
        let mut services: Vec<GrpcServiceInfo> = pool
            .services()
            .map(|svc| GrpcServiceInfo {
                service: svc.full_name().to_string(),
                methods: svc
                    .methods()
                    .map(|m| GrpcMethodInfo {
                        method: m.name().to_string(),
                        input_type: m.input().full_name().to_string(),
                        output_type: m.output().full_name().to_string(),
                        client_streaming: m.is_client_streaming(),
                        server_streaming: m.is_server_streaming(),
                    })
                    .collect(),
            })
            .collect();
        services.sort_by(|a, b| a.service.cmp(&b.service));
        Self {
            services,
            source: source.to_string(),
        }
    }
}

/// 描述符池缓存：键为来源内容摘要（proto 内容哈希 / 反射地址），
/// 同一 proto 集或同一服务的反射结果进程内只取一次。
/// `order` 记录插入序（HashMap 无序），供 [`POOL_CACHE_CAP`] 淘汰最早项。
struct PoolCache {
    map: HashMap<String, Arc<DescriptorPool>>,
    order: std::collections::VecDeque<String>,
}

static POOL_CACHE: OnceLock<Mutex<PoolCache>> = OnceLock::new();

/// 缓存上限：反射按地址成键、proto 按内容哈希成键，长期使用会无限累积
///（每次改 proto / 换地址都新增一项，池可达数 MB）。超限淘汰最早插入项。
const POOL_CACHE_CAP: usize = 32;

fn pool_cache() -> &'static Mutex<PoolCache> {
    POOL_CACHE.get_or_init(|| {
        Mutex::new(PoolCache {
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
        })
    })
}

impl PoolCache {
    fn get(&self, key: &str) -> Option<Arc<DescriptorPool>> {
        self.map.get(key).map(Arc::clone)
    }

    fn insert(&mut self, key: String, pool: Arc<DescriptorPool>) {
        if self.map.insert(key.clone(), pool).is_none() {
            self.order.push_back(key);
        }
        while self.map.len() > POOL_CACHE_CAP {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            } else {
                break;
            }
        }
    }
}

fn cache_key(source: &GrpcSource, address: &str, use_tls: bool) -> String {
    use std::hash::{Hash, Hasher};
    match source {
        // 反射池内容由服务端决定：键必须含地址（含 TLS 模式），否则跨服务端
        // 串池——对服务 A 反射后，服务 B 会直接命中 A 的描述符（方法解析全错）。
        GrpcSource::Reflection => format!("reflection:{use_tls}@{address}"),
        GrpcSource::Files(files) => {
            // proto 文件集内容自包含，池与地址无关（同内容跨端点复用命中）。
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for f in files {
                (&f.name, &f.content).hash(&mut hasher);
            }
            format!("files:{:x}", hasher.finish())
        }
    }
}

/// 获取描述符池（命中缓存直接返回；反射结果按地址缓存，proto 按内容哈希缓存）。
pub async fn acquire_pool(
    source: GrpcSource,
    address: &str,
    use_tls: bool,
    metadata: &[(String, String)],
    force_reload: bool,
) -> Result<Arc<DescriptorPool>> {
    let key = cache_key(&source, address, use_tls);
    if !force_reload {
        if let Some(hit) = pool_cache().lock().expect("pool cache").get(&key) {
            return Ok(hit);
        }
    }
    let pool = match source {
        GrpcSource::Files(files) => compile_files(&files)?,
        GrpcSource::Reflection => reflect_pool(address, use_tls, metadata)
            .await
            .map_err(|e| AppError::Grpc(format!("服务反射失败：{e}")))?,
    };
    let pool = Arc::new(pool);
    pool_cache()
        .lock()
        .expect("pool cache")
        .insert(key, Arc::clone(&pool));
    Ok(pool)
}

/// protox 运行时编译内存 proto 文件集（import 直接从文件集内解析）。
pub fn compile_files(files: &[ProtoFileInput]) -> Result<DescriptorPool> {
    use protox::file::{File as ProtoxFile, FileResolver};

    if files.is_empty() {
        return Err(AppError::Grpc(
            "未提供 proto 文件：请先在端点中导入 .proto，或改用服务端反射".into(),
        ));
    }

    struct MapResolver {
        files: Vec<ProtoFileInput>,
    }
    impl FileResolver for MapResolver {
        fn open_file(&self, name: &str) -> std::result::Result<ProtoxFile, protox::Error> {
            let found = self
                .files
                .iter()
                .find(|f| f.name == name)
                .ok_or_else(|| protox::Error::file_not_found(name))?;
            ProtoxFile::from_source(&found.name, &found.content)
        }
    }

    // Compiler 要求 resolver: 'static，克隆一份所有权进去
    let mut compiler = protox::Compiler::with_file_resolver(MapResolver {
        files: files.to_vec(),
    });
    compiler.include_imports(true);
    for f in files {
        compiler
            .open_file(&f.name)
            .map_err(|e| AppError::Grpc(format!("proto 编译失败（{}）：{e}", f.name)))?;
    }
    Ok(compiler.descriptor_pool())
}

/// 服务端反射：list_services + 逐服务 file_containing_symbol，
/// 汇总 FileDescriptorProto 去重后构建描述符池。
///
/// 版本策略与 grpcurl 一致：先试 v1（Unimplemented 即服务未实现 v1），
/// 自动回退 v1alpha（大量现存服务仍只挂 v1alpha）。
///
/// 不用 tonic-reflection 的生成客户端：其流式方法无法附加 metadata
/// （认证头会丢），改走低层 `Grpc::streaming` + 自带 ProstCodec。
async fn reflect_pool(
    address: &str,
    use_tls: bool,
    metadata: &[(String, String)],
) -> Result<DescriptorPool> {
    match run_reflection::<V1Reflection>(address, use_tls, metadata).await {
        Ok(pool) => Ok(pool),
        Err(ReflectError::Unimplemented) => match run_reflection::<V1AlphaReflection>(
            address, use_tls, metadata,
        )
        .await
        {
            Ok(pool) => Ok(pool),
            Err(ReflectError::Unimplemented) => Err(AppError::Grpc(
                "服务端未启用 gRPC 反射（v1 / v1alpha 均不可用），请导入 proto 文件后用本地编译"
                    .into(),
            )),
            Err(ReflectError::Fatal(e)) => Err(e),
        },
        Err(ReflectError::Fatal(e)) => Err(e),
    }
}

/// 反射流程错误：Unimplemented 表示该版本反射服务不存在（可回退重试）。
enum ReflectError {
    Unimplemented,
    Fatal(AppError),
}

impl From<AppError> for ReflectError {
    fn from(e: AppError) -> Self {
        ReflectError::Fatal(e)
    }
}

/// 反射响应的解包结果（服务列表 / 描述符字节 / 其他应答）。
enum ReflectionReply {
    Services(Vec<String>),
    FileDescriptors(Vec<Vec<u8>>),
    Other,
}

/// 反射协议适配：v1 与 v1alpha 消息结构一致，仅包名与路径不同。
trait ReflectionProto: Send + Sync + 'static {
    type Request: prost::Message + Default + Send + Sync + 'static;
    type Response: prost::Message + Default + Send + Sync + 'static;
    const PATH: &'static str;
    fn list_services() -> Self::Request;
    fn file_containing_symbol(symbol: String) -> Self::Request;
    fn unpack(response: Self::Response) -> ReflectionReply;
}

struct V1Reflection;
struct V1AlphaReflection;

macro_rules! impl_reflection {
    ($ty:ident, $pb:ident, $path:literal) => {
        impl ReflectionProto for $ty {
            type Request = tonic_reflection::pb::$pb::ServerReflectionRequest;
            type Response = tonic_reflection::pb::$pb::ServerReflectionResponse;

            const PATH: &'static str = $path;

            fn list_services() -> Self::Request {
                Self::Request {
                    host: String::new(),
                    message_request: Some(tonic_reflection::pb::$pb::server_reflection_request::MessageRequest::ListServices(
                        String::new(),
                    )),
                }
            }

            fn file_containing_symbol(symbol: String) -> Self::Request {
                Self::Request {
                    host: String::new(),
                    message_request: Some(
                        tonic_reflection::pb::$pb::server_reflection_request::MessageRequest::FileContainingSymbol(symbol),
                    ),
                }
            }

            fn unpack(response: Self::Response) -> ReflectionReply {
                use tonic_reflection::pb::$pb::server_reflection_response::MessageResponse;
                match response.message_response {
                    Some(MessageResponse::ListServicesResponse(list)) => {
                        ReflectionReply::Services(
                            list.service.into_iter().map(|s| s.name).collect(),
                        )
                    }
                    Some(MessageResponse::FileDescriptorResponse(files)) => {
                        ReflectionReply::FileDescriptors(files.file_descriptor_proto)
                    }
                    _ => ReflectionReply::Other,
                }
            }
        }
    };
}

impl_reflection!(
    V1Reflection,
    v1,
    "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo"
);
impl_reflection!(
    V1AlphaReflection,
    v1alpha,
    "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo"
);

/// 单版本反射流程：list_services + 逐服务 file_containing_symbol。
///
/// 兼容性关键：每次请求走独立 bidi 流，发送单条请求后立即半关发送侧
/// （drop 请求端）。grpcurl 同款策略——部分服务端实现「读到 EOF 才处理」，
/// 常驻流会永久挂起；对交互式 bidi 服务端同样正确（先答后见 EOF）。
async fn run_reflection<P: ReflectionProto>(
    address: &str,
    use_tls: bool,
    metadata: &[(String, String)],
) -> std::result::Result<DescriptorPool, ReflectError> {
    let channel = super::client::connect(address, use_tls).await?;

    let services = match reflection_round_trip::<P>(&channel, metadata, P::list_services()).await? {
        ReflectionReply::Services(services) => services,
        _ => {
            return Err(ReflectError::Fatal(AppError::Grpc(
                "反射响应类型不符（期望服务列表）".into(),
            )))
        }
    };

    let mut files: HashMap<String, prost_types::FileDescriptorProto> = HashMap::new();
    for symbol in &services {
        if let ReflectionReply::FileDescriptors(bytes_list) = reflection_round_trip::<P>(
            &channel,
            metadata,
            P::file_containing_symbol(symbol.clone()),
        )
        .await?
        {
            for bytes in bytes_list {
                let fdp = prost_types::FileDescriptorProto::decode(bytes.as_slice())
                    .map_err(|e| AppError::Grpc(format!("描述符解码失败：{e}")))?;
                files
                    .entry(fdp.name.clone().unwrap_or_default())
                    .or_insert(fdp);
            }
        }
    }

    let set = prost_types::FileDescriptorSet {
        file: files.into_values().collect(),
    };
    DescriptorPool::from_file_descriptor_set(set).map_err(|e| {
        ReflectError::Fatal(AppError::Grpc(format!(
            "反射描述符不完整（缺 import？）：{e}"
        )))
    })
}

/// 单次反射请求：建流 → 发送 → 半关发送侧 → 读单条响应。
async fn reflection_round_trip<P: ReflectionProto>(
    channel: &tonic::transport::Channel,
    metadata: &[(String, String)],
    request: P::Request,
) -> std::result::Result<ReflectionReply, ReflectError> {
    let mut grpc = tonic::client::Grpc::new(channel.clone());
    grpc.ready()
        .await
        .map_err(|e| AppError::Connection(format!("gRPC 通道未就绪：{e}")))?;
    let path = P::PATH
        .parse()
        .map_err(|e| AppError::Validation(format!("反射路径构造失败：{e}")))?;

    let (req_tx, req_rx) = tokio::sync::mpsc::channel::<P::Request>(1);
    let request_stream = futures::stream::unfold(req_rx, |mut rx| async move {
        rx.recv().await.map(|msg| (msg, rx))
    });
    let mut outbound = tonic::Request::new(request_stream);
    for (k, v) in metadata {
        if k.trim().is_empty() {
            continue;
        }
        let parsed_key = k
            .trim()
            .parse::<tonic::metadata::AsciiMetadataKey>()
            .map_err(|e| AppError::Grpc(format!("反射 metadata 键非法（{k}）：{e}")))?;
        let value = tonic::metadata::MetadataValue::try_from(v.as_str())
            .map_err(|_| AppError::Grpc(format!("反射 metadata 值非法：{k}")))?;
        outbound.metadata_mut().insert(parsed_key, value);
    }

    // 请求先入缓冲、随即半关发送端：DATA 帧与 END_STREAM 在等待响应头期间
    // 就能发出。若「先 await streaming 再发请求」，服务端要等首条消息才回
    // 响应头、客户端要等响应头才发消息，会互等死锁（grpcbin 实测复现）。
    req_tx
        .send(request)
        .await
        .map_err(|_| AppError::Grpc("反射请求发送失败".into()))?;
    drop(req_tx);

    let response: tonic::Response<tonic::Streaming<P::Response>> = grpc
        .streaming(
            outbound,
            path,
            super::client::ProstCodec::<P::Request, P::Response>::default(),
        )
        .await
        .map_err(to_reflect_error)?;
    let mut inbound = response.into_inner();

    let response = inbound
        .message()
        .await
        .map_err(to_reflect_error)?
        .ok_or_else(|| ReflectError::Fatal(AppError::Grpc("反射流提前结束".into())))?;
    Ok(P::unpack(response))
}

/// Unimplemented（v1 不存在）→ 可回退；其余按致命错误处理。
fn to_reflect_error(status: tonic::Status) -> ReflectError {
    if status.code() == tonic::Code::Unimplemented {
        return ReflectError::Unimplemented;
    }
    ReflectError::Fatal(AppError::Grpc(format!("服务反射失败：{status}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_files() -> Vec<ProtoFileInput> {
        vec![ProtoFileInput {
            id: "p1".into(),
            name: "a.proto".into(),
            content: "syntax = \"proto3\"; message A { string x = 1; }".into(),
        }]
    }

    /// 回归锁定：反射池按地址成键——此前固定 "reflection" 键导致跨服务端串池
    /// （对服务 A 反射后，服务 B 直接命中 A 的描述符，方法解析全部错位）。
    #[test]
    fn reflection_key_isolated_per_address() {
        let src = GrpcSource::Reflection;
        let a = cache_key(&src, "a:443", true);
        let b = cache_key(&src, "b:443", true);
        let a_plain = cache_key(&src, "a:443", false);
        assert_ne!(a, b, "不同地址不得共享缓存键");
        assert_ne!(a, a_plain, "同地址 TLS / 明文不得共享缓存键");
        assert_eq!(a, cache_key(&src, "a:443", true), "同参数键应稳定");
    }

    /// proto 文件集内容自包含：与地址无关，同内容必须命中同一键。
    #[test]
    fn files_key_ignores_address() {
        let a = cache_key(&GrpcSource::Files(fixture_files()), "a:443", true);
        let b = cache_key(&GrpcSource::Files(fixture_files()), "b:1", false);
        assert_eq!(a, b, "proto 内容哈希键不应掺入地址");
    }

    /// 缓存超上限淘汰最早插入项，且已有项不被误删。
    #[test]
    fn pool_cache_evicts_oldest_at_cap() {
        let pool = DescriptorPool::from_file_descriptor_set(prost_types::FileDescriptorSet {
            file: vec![],
        })
        .expect("空池");
        let pool = Arc::new(pool);
        let mut cache = PoolCache {
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
        };
        for i in 0..POOL_CACHE_CAP {
            cache.insert(format!("k{i}"), Arc::clone(&pool));
        }
        assert_eq!(cache.map.len(), POOL_CACHE_CAP);
        // 淘汰线：再插一项应踢掉 k0，保留其余。
        cache.insert("new".into(), Arc::clone(&pool));
        assert_eq!(cache.map.len(), POOL_CACHE_CAP);
        assert!(cache.get("k0").is_none(), "最旧项应被淘汰");
        assert!(cache.get("k1").is_some(), "未超龄项应保留");
        assert!(cache.get("new").is_some(), "新项应入缓存");
    }
}
