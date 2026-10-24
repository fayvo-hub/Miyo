//! JUnit XML 输出：GitHub Actions / GitLab CI 等流水线的通用测试报告格式。
//!
//! 手写转义不引依赖：单 testsuite，每个接口一个 testcase，
//! 失败断言合并进单个 `<failure>`，请求级错误输出 `<error>`，跳过输出 `<skipped>`。

use crate::CaseKind;

/// 转义 XML 文本节点 / 属性中的五个保留字符。
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn secs(duration_ms: Option<f64>) -> String {
    format!("{:.3}", duration_ms.unwrap_or(0.0) / 1000.0)
}

/// 把运行报告渲染为 JUnit XML 字符串。
pub fn junit_xml(report: &crate::RunReport) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuites name=\"Miyo {}\" tests=\"{}\" failures=\"{}\" errors=\"0\" time=\"{}\">\n",
        escape_xml(&report.project),
        report.cases.len(),
        report.failed,
        secs(Some(report.total_duration_ms)),
    ));
    out.push_str(&format!(
        "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{}\">\n",
        escape_xml(&report.project),
        report.cases.len(),
        report.failed,
        report.skipped,
        secs(Some(report.total_duration_ms)),
    ));

    for case in &report.cases {
        let name = format!("{} {}", case.method, case.path);
        let classname = if case.group.is_empty() {
            case.name.clone()
        } else {
            format!("{} / {}", case.group, case.name)
        };
        match &case.kind {
            CaseKind::Passed {
                status,
                duration_ms,
                assertions: _,
            } => {
                out.push_str(&format!(
                    "    <testcase name=\"{}\" classname=\"{}\" time=\"{}\"",
                    escape_xml(&name),
                    escape_xml(&classname),
                    secs(*duration_ms),
                ));
                // 状态码进 properties：CI 日志里可直接看到 HTTP 结果
                if let Some(code) = status {
                    out.push_str(&format!(">\n      <properties>\n        <property name=\"http_status\" value=\"{code}\"/>\n      </properties>\n    </testcase>\n"));
                } else {
                    out.push_str("/>\n");
                }
            }
            CaseKind::Failed {
                status,
                duration_ms,
                failures,
                request_error,
            } => {
                out.push_str(&format!(
                    "    <testcase name=\"{}\" classname=\"{}\" time=\"{}\">\n",
                    escape_xml(&name),
                    escape_xml(&classname),
                    secs(*duration_ms),
                ));
                if let Some(err) = request_error {
                    out.push_str(&format!(
                        "      <error message=\"{}\">{}</error>\n",
                        escape_xml(err),
                        escape_xml(err),
                    ));
                }
                for (desc, reason) in failures {
                    let detail = reason.clone().unwrap_or_default();
                    out.push_str(&format!(
                        "      <failure message=\"{}\" type=\"assertion\">{}</failure>\n",
                        escape_xml(desc),
                        escape_xml(&detail),
                    ));
                }
                if let Some(code) = status {
                    out.push_str(&format!("      <system-out>HTTP {code}</system-out>\n"));
                }
                out.push_str("    </testcase>\n");
            }
            CaseKind::Skipped { reason } => {
                out.push_str(&format!(
                    "    <testcase name=\"{}\" classname=\"{}\">\n      <skipped message=\"{}\"/>\n    </testcase>\n",
                    escape_xml(&name),
                    escape_xml(&classname),
                    escape_xml(reason),
                ));
            }
        }
    }

    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

/// 把 JUnit XML 写入文件（父目录不存在时自动创建）。
pub fn write_junit(report: &crate::RunReport, path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建报告目录失败：{e}"))?;
    }
    std::fs::write(path, junit_xml(report)).map_err(|e| format!("写入报告失败：{e}"))
}
