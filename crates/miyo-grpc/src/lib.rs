pub mod client;
pub mod descriptor;

pub use client::{
    connect, invoke_unary, resolve_method, spawn_server_stream, GrpcEvent, GrpcInvokeArgs,
    GrpcMessage, GrpcResponse,
};
pub use descriptor::{
    acquire_pool, compile_files, GrpcMethodInfo, GrpcServiceInfo, GrpcSource, ProtoFileInput,
    ServiceCatalog,
};
pub use prost_reflect::DescriptorPool;
