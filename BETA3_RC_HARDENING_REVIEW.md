# Beta 3 RC 前加固验收

版本：`0.2.0-beta.3`
日期：2026-09-01
结论：Pro 复查指出的本地代码缺陷已修复；真实外部模型验收等待用户配置 Provider API Key，因此当前仍为 Beta，不标记 RC。

## 修复矩阵

| 项目 | 结果与证据 |
|---|---|
| API Key 持久化 | 使用现有 `keyring` 依赖写入 Windows Credential Manager；`app.db` 仅保存 Provider ID。应用重启后在模型目录、Doctor、主 Agent 和专家团启动前恢复到 Pi Runtime。Windows 系统密钥库真实写入/读取/删除测试通过。 |
| 单专业模型覆盖 | `call_expert` 按项目覆盖、应用级专业设置、主模型回退解析 Provider/Model，并传递专业 thinking level。Rust 测试验证摄影 Agent 使用项目模型、应用级 `high` thinking。 |
| 专家团综合隔离 | 综合改为独立短生命周期 Pi Session，`allowedTools=[]`、`allowCallExpert=false`；Rust Gateway 再按 `task_type + agent_type` 强制白名单，测试验证读取工具和 `call_expert` 均被拒绝。 |
| 结构化结果 | 新增 `submit_agent_result`、`submit_expert_result`、`submit_team_result`，参数由 TypeBox 校验。真实 Pi SDK 测试验证最终自由文本不是 JSON 时，Patch 仍通过独立 `structured_result` 完整返回。自由文本解析仅保留为旧 Session 兼容回退。 |
| 通用 stale | 主任务、单专业任务、专家团成员与综合结果均比较 context/base revision 和当前 revision；过期编辑提案不会进入 PatchProposal。 |
| 焦点视觉附件 | 主 Agent 默认不再自动携带选区图片；单专家根据 `focusRefs` 由 Rust 重新解析正式媒体，且只在目标模型支持视觉时发送。 |
| 模型切换语义 | UI 明示模型和 thinking level 从新建讨论开始生效，避免现有 Pi Session 继续旧模型造成误解。 |
| Runtime 任务内存 | 终态任务释放项目路径、app data 路径与 event sink，仅保留最多 256 项轻量状态。 |

## 验证边界

- Rust 80 项、前端 Vitest 17 项、Agent Host 真实 Pi SDK 13 项通过；`cargo fmt --check`、Release Check、Clippy `-D warnings`、TypeScript 与 Vite 生产构建通过。
- 安装版：`src-tauri/target/release/bundle/nsis/创作工作台_0.2.0-beta.3_x64-setup.exe`，45,954,086 bytes，SHA-256 `549C1C33B0AB353E524C249FC6FAA53B9FF3CC19F48CD7415246F7B8B3A5BEEC`。
- 免安装版：`创作工作台_0.2.0-beta.3_windows_x64_免安装版.zip`，96,797,543 bytes，SHA-256 `1FFAED743B3FEE2D99CDF4EA3755BE6ACF85EE0A0754341EC6748FCBEE34303C`；ZIP 共 22,951 项。
- 在 `PATH` 仅保留 `C:\Windows\System32` 时，直接从免安装包启动私有 Host，Doctor 返回 Pi SDK `0.84.4`、40 个 Provider、1290 个模型，ModelRuntime 与 Tool Gateway 健康。
- 自动化使用真实 Pi SDK、真实 AgentSession、真实 Tool Loop 与官方 faux Provider。
- 本机 Pi `auth.json`、Agent 凭据索引以及常见模型环境变量均未配置，无法在不获得用户密钥的情况下执行真实 OpenAI / Anthropic / Gemini 文本和视觉模型验收。
- 不读取、复制或迁移 jewelflow 或其他项目的 API Key。
