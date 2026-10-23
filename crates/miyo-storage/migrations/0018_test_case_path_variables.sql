-- 测试用例补存路径变量（URL path 传参，如 /users/{id}）。
--
-- 旧表只存 url_path + params + headers + body，路径模板里的 {id} 无处安放；
-- 用例单跑/集合跑时 buildCaseRequest 只能喂空 path_variables，
-- 于是把字面量 `/users/{id}` 直接发出去。
--
-- 默认 '[]' 让存量行保持原样（无路径变量）。
ALTER TABLE test_cases ADD COLUMN path_variables TEXT NOT NULL DEFAULT '[]';