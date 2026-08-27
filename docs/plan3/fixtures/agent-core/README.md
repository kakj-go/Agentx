# Agent Core 参考行为 Fixture

本目录保存绑定 Pi 固定参考版本的规范化黑盒行为样本。它是 Agentx Rust Core 的行为输入，不是 Pi 内部类型、JSONL Session 格式或 npm Runtime 的复制品。

固定来源：

- Repository：`https://github.com/earendil-works/pi`
- Commit：`a69bef789bc95abf0acee16f7b4660b70b650bb9`
- Package：`@earendil-works/pi-agent-core@0.84.2`
- Schema：`fixture.schema.json`，版本 `1.0`

规范化移除时间戳、随机 ID、绝对路径、Provider 私有字段和流式 Token delta；保留事件顺序、角色、工具名、稳定操作 ID、压缩类型/边界和终止原因。

更新流程：

1. 运行 `uv run --frozen python scripts/plan3/verify_pi_reference.py --run-upstream-tests` 验证 Commit、Package、License、关键源码 Blob及跨平台上游行为；有可用 `/bin/bash` 时追加 `--run-bash-tests`。
2. 对照 `docs/plan3/p3-00/reference-audit.md` 中列出的上游测试重放脚本化 Model/Tool 输入。
3. 只把规范化结果写入 `cases.jsonl`；每行必须通过 `fixture.schema.json`。
4. 运行 `cargo test -p agentx-agent-core`。Commit、Package 或预期行为变化必须同时更新审计报告、Schema 版本或差异说明。

Fixture 可离线被 Rust 测试消费。Pi 源码或 Node Runtime不进入 Agentx 生产构建和镜像。
