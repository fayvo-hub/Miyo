-- 设计页结构化 Body 编辑器：响应示例字段注释（JSON Pointer → FieldDoc）。
-- 旧库缺列时补齐（默认空对象）。
ALTER TABLE response_examples ADD COLUMN docs_json TEXT NOT NULL DEFAULT '{}';
