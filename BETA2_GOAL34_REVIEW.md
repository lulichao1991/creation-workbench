# Beta 2 Goal34 Pi 原生 Agent 完整验收

版本：`0.2.0-beta.2`
验收日期：2026-09-01
验收项目：《智斗游戏》
结论：20 项通过。

本轮把“真实 Pi”定义为真实 `@earendil-works/pi-coding-agent` 的 `ModelRuntime`、`AgentSession`、Session 持久化与 Tool Loop。自动化使用 SDK 官方 faux Provider，避免产生外部模型费用；Provider、模型和 API Key 的真实配置入口已由 Goal32 在应用内提供。

## 20 项验收矩阵

| # | 验收项 | 结论与证据 |
|---:|---|---|
| 1 | 项目级主 Agent 持续讨论 30 集结构 | 通过。`mutation::tests::goal_01_to_12_end_to_end_acceptance` 创建 30 集《智斗游戏》；Agent Host 测试 `Goal34 main Agent reads 30-episode structure and continues episode-10 discussion with memory` 在同一真实 Pi Session 连续两轮运行。 |
| 2 | 主 Agent 自行调用 `read_story_structure` | 通过。上述 Goal34 Host 测试由模型发起该工具；`agent_gateway::tests::executes_all_read_tools_with_current_revision_and_audit` 验证 Rust 当前 revision 与审计。 |
| 3 | “把第10集提前一点”沿用当前讨论 | 通过。第二轮模型上下文断言包含第一轮 30 集判断、工具结果和新请求，不依赖 Rust 手工工作记忆。 |
| 4 | 选中镜头04构图后主 Agent 读取完整镜头 | 通过。`runs multiple Workbench tools inside the real Pi Tool Loop` 与 Gateway 测试跑通 `get_selection → read_shot_context(shot04)`；结果含场、镜头03/04/05及正式关系。 |
| 5 | 主 Agent 调用摄影 Agent | 通过。`main AgentSession calls an independent cinematography AgentSession and synthesizes its result` 跑通真实 `call_expert(cinematography)`。 |
| 6 | 摄影 Agent 读取前后镜头与正式资产 | 通过。摄影 Session 白名单含 `read_shot_context`、`read_asset`、`read_neighbors`；Rust Gateway 测试验证前后镜头和正式资产元数据。 |
| 7 | 摄影 Agent 可看正式角色图 | 通过。`expert_team::tests::pi_team_prepares_independent_tool_driven_professional_sessions` 创建正式 PNG，断言摄影 Session 收到视觉附件，非视觉角色不接收。 |
| 8 | 返回修改提案 | 通过。`agent_application::tests::prepares_single_expert_context_and_materializes_patch_proposal` 将结构化 Patch 转为持久化提案，模型不能直接写库。 |
| 9 | 越权字段产生权限申请 | 通过。`permission::tests::classifies_scope_and_builds_diff_with_permission_card` 与 `refuses_unconfirmed_out_of_scope_write_without_changing_facts` 覆盖。 |
| 10 | 用户确认后写入 | 通过。`permission::tests::applies_approved_batch_once_and_never_applies_protected_field` 只应用明确批准项，保护字段仍拒绝。 |
| 11 | 修改进入 ChangeSet | 通过。权限应用在同一事务建立 `source_type=agent` ChangeSet、Changes、revision 与提案状态。 |
| 12 | 可以撤销 | 通过。`mutation::tests::mutation_records_history_and_undoes_patch` 与 `undoing_atomic_reorder_restores_unique_orders` 验证历史撤销。 |
| 13 | 用户手工修改后主动分析本轮 | 通过。`agent_application::tests::analyzes_change_set_only_on_explicit_readonly_task_and_marks_stale` 只接受用户 ChangeSet，空 WriteScope，且 revision 变化后 stale。 |
| 14 | 主 Agent 发现跨专业问题后建议专家团 | 通过。主 Agent Prompt 只允许返回受约束的 `expertTeamSuggestion`；`main_agent_accepts_only_bounded_cross_discipline_team_suggestions` 拒绝重复、未知或少于两名的成员；UI 将有效建议带入申请和成本确认，不自动启动。 |
| 15 | 用户确认后启动三个专业 Pi Session | 通过。Host 测试 `runs three independent professional AgentSessions in parallel with distinct tool loops` 与 Rust `confirmed_pi_team_closes_member_sessions_and_main_synthesizes` 验证确认硬边界、编剧/导演/摄影三 Session 并行。 |
| 16 | 主 Agent 汇总专家意见 | 通过。全部成员终态后才恢复原 MainAgentSession，综合共识、分歧、建议、问题和风险；Patch 与扩权请求被丢弃。 |
| 17 | 项目记忆按需读取 | 通过。Goal34 Host 测试由模型在读取结构后调用 `read_active_memories`；Gateway 按当前作用域、状态和事实优先级返回。 |
| 18 | 静态生图仍由用户显式触发 | 通过。UI 保持“确认参数并生成候选 → 再次点击确认”；后端测试覆盖候选隔离、取消、失败、显式转正与 stale，Agent 工具表无生图入口。 |
| 19 | Prompt Agent 调用编译预览 | 通过。新增真实 Pi 测试 `prompt AgentSession calls deterministic compile_prompt_preview without video tools`；Rust `prompt_agent_compiles_readonly_preview_without_persisting_or_exposing_paths` 验证确定性预览、不落编译历史、不写正式提示词、不暴露媒体路径。 |
| 20 | 全程不存在视频生成 | 通过。Prompt Agent 仅得到 `compile_prompt_preview`；测试断言 Tool Loop 无 `video_generate`、`generate_video` 或 `video_job`，Tauri 命令、Rust Gateway 和前端均无视频生成入口。 |

## 质量与发布证据

- 前端：6 个文件、17 项 Vitest 通过。
- Agent Host：12 项真实 Pi SDK 测试通过。
- Rust：75 项测试通过。
- `cargo fmt --check`、Clippy `-D warnings`、TypeScript、Vite 生产构建通过。
- Beta 2 Windows NSIS 与免安装 ZIP 均已成功打包；在 PATH 仅保留 `C:\Windows\System32` 时，直接从免安装包启动私有 Runtime，返回 Pi SDK `0.84.4`、40 个 Provider、1290 个模型，ModelRuntime 与 Tool Gateway 健康。
- 安装版：`src-tauri/target/release/bundle/nsis/创作工作台_0.2.0-beta.2_x64-setup.exe`（45,926,815 bytes）。
- 免安装版：`创作工作台_0.2.0-beta.2_windows_x64_免安装版.zip`（96,774,128 bytes）；ZIP 共 22,951 项，包含主程序、私有 `node.exe`、Agent Host、Pi SDK `0.84.4` 与使用说明。
- Agent 凭据只由包内 Pi ModelRuntime 管理；正式项目事实仍只来自 `project.db`、`app.db` 与项目媒体文件。

## 明确边界

- 静态生图仍需用户显式触发和确认。
- 提示词预览不会自动设为正式稿。
- 专家团不会由主 Agent 自动启动。
- 不包含视频 Provider、视频 Job、视频生成、剪辑、配音或发布链路。
