//! Agent 监听端口文件：`{data_dir}/agent-port`。
//!
//! 控制面启动成功后写入**实际绑定**的端口；`miyo-mcp` 发现时优先读此文件，
//! 用户改端口或默认端口被占用自动顺延后仍能连上。停止服务时删除，避免误导。

use std::path::{Path, PathBuf};

/// 端口文件名（位于数据目录下）。
pub const PORT_FILE: &str = "agent-port";

/// 端口文件完整路径。
pub fn port_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PORT_FILE)
}

/// 写入实际监听端口（十进制文本）。
pub fn write_port(data_dir: &Path, port: u16) -> std::io::Result<()> {
    std::fs::write(port_path(data_dir), port.to_string())
}

/// 读取端口（文件缺失 / 内容非法时返回 None）。
pub fn read_port(data_dir: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(port_path(data_dir)).ok()?;
    text.trim().parse().ok()
}

/// 删除端口文件（文件不存在时静默）。
pub fn clear_port(data_dir: &Path) {
    let _ = std::fs::remove_file(port_path(data_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_roundtrip_and_clear() {
        let dir = std::env::temp_dir().join(format!("miyo-agent-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(read_port(&dir), None, "无文件应为 None");
        write_port(&dir, 4111).expect("写入");
        assert_eq!(read_port(&dir), Some(4111));

        clear_port(&dir);
        assert_eq!(read_port(&dir), None, "清除后应为 None");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_content_is_none() {
        let dir = std::env::temp_dir().join(format!("miyo-agent-port-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(PORT_FILE), "not-a-port").unwrap();
        assert_eq!(read_port(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
