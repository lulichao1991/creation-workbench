# Goal 01–18 实施状态

验证日期：2026-08-31（0.2.0-alpha.6，V2 Goal18 Agent 右侧工作区）

## Goal 对照

| Goal | 状态 | 主要实现与证据 |
| --- | --- | --- |
| 01 桌面应用与项目骨架 | 完成 | Tauri 桌面壳；项目新建、打开、复制、删除；一项目一目录与 `project.db`；`commands::project_lifecycle_creates_required_directories` |
| 02 内容单元树 | 完成 | `content_units`；season / episode / short / act / custom；父子关系、排序、跨父级移动、循环关系校验；批量建立剧集 |
| 03 剧本系统 | 完成 | `scripts`、`scenes`；场 CRUD、排序、标题、地点、时间、摘要和正文编辑 |
| 04 镜头系统 | 完成 | `shots` 完整字段；CRUD、复制、拖动排序、多选；稳定 UUID 与动态显示编号分离 |
| 05 SelectionStore | 完成 | Zustand 全局 Store；项目、内容单元、工作区、对象、字段、多选、模式；`selectionScope` 与 `writeScope` 分离；前端单元测试覆盖 |
| 06 统一写入与历史 | 完成 | 所有业务修改经 `apply_mutation`；revision、Change、ChangeSet、历史面板、指定变更集撤销与 Ctrl+Z |
| 07 资产系统 | 完成 | 角色 / 场景 / 道具；AssetMedia、AssetRequirement、作用范围、提示词草稿、图片导入、多图与主图 |
| 08 关键帧系统 | 完成 | planned / ready；single / start / middle / end；描述、提示词草稿、手动图片导入 |
| 09 生成任务 | 完成 | 镜头多选、任务创建、镜头顺序、总时长、目标模型与提示词编辑 |
| 10 作品结构 | 完成 | 项目树、季剧集列表、基础时间轴、带语义 Relation 清单；高级关系网留待 V2 |
| 11 快照 | 完成 | JSON 快照创建、内容统计查看和完整恢复；`snapshot_restores_business_state` |
| 12 整体 Review | 完成 | Rust、TypeScript、Vitest、Clippy、桌面可视化与 NSIS 发布构建全部通过 |
| 13 V2 工程基础 | 完成 | `app.db` V1 与备份迁移；8 个默认关闭的 feature flags；`project.db` V4 Agent 核心表；项目复制隔离 Agent 数据；Agent / Context / Memory / Permission 契约目录；开始拆分 Workbench；V1 端到端回归通过 |
| 14 Pi Runtime 接入 | 完成 | Rust `AgentRuntime` 与 `PiRuntimeAdapter`；TypeScript Runtime 契约；Pi 官方 RPC JSONL；流式文本和工具事件；追加输入、状态与取消；MockRuntime；Windows `.cmd`、中文空格路径和子进程回收验证 |
| 15 上下文系统 | 完成 | SelectionSnapshot / ContextPolicy / ContextPackage；字段级中心事实；父链、邻居、Relation 与镜头资产结构查询；FTS5；token budget；稳定 checksum；revision 校验和事务快照；V4 持久化 |
| 16 权限、修改提案与 AI 卡片 | 完成 | WriteScope / ProtectedScope 权限判断；Patch old/new 差异；权限卡；revision、旧值、对象与权限变化 stale 校验；一次性批准；同事务批量 Mutation 与 Agent ChangeSet；9 个权限安全测试 |
| 17 主 Agent 与单专业 Agent | 完成 | MainAgent 应用服务；IntentResolver；ExpertRegistry / ExpertRouter；六类专家配置；会话/任务持久化；Context/Pi/Patch 串联；统一结构化输出；固定路由测试与含糊澄清 |
| 18 Agent 右侧工作区 | 完成 | 全工作区共用会话 UI；当前选区、revision 与写入/保护范围；流式输出、专家状态和停止；讨论/建议/编辑模式；AI Card 与 Patch 差异；应用/拒绝/讨论；只读模式后端兜底；Windows 实机验收 |

## 自动化验收

Rust 端到端测试 `mutation::tests::goal_01_to_12_end_to_end_acceptance` 完整验证《智斗游戏》场景：

- 创建项目、第一季和 EP01–EP30；
- EP01 创建 3 个剧本场与 10 个镜头；
- 修改并重排镜头04，同时验证稳定 ID；
- 创建奶牛猫、大黄狗、暹罗猫、游戏大厅、广播屏；
- 建立资产需求、提示词草稿并导入资产图；
- 为镜头04建立关键帧需求、提示词并导入图片；
- 用镜头01–05建立生成任务并保存视频提示词；
- 建立内容语义关系；
- 创建快照、继续修改、恢复快照；
- 重新打开数据库，验证全部数量、关系和文件引用。

## 通过的命令

```text
npm test
npm run build
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
npm run tauri build
```

## UI 参考适配

已按 `C:\Users\LU\Documents\Codex\2026-08-29\wo\outputs\video-workbench` 的视觉语言完成桌面端重构：深炭黑分层表面、酸性荧光绿强调色、Lucide 线性图标、紧凑型工作区标签、32px 制图网格背景，以及可折叠的左右侧栏。完整设计系统、设计风格和视觉效果参数记录在 `design-dna-reference.json`；同时提供 `prefers-reduced-motion` 降级。

已在真实 Tauri 窗口中完成首页、三栏工作区与双侧栏折叠的视觉验收，并重新生成 NSIS 安装包和 Windows x64 免安装压缩包。

## V1 发布基线边界

V1 `0.1.2` 不包含 Pi Agent、主 Agent、专业 Agent、专家团、记忆、权限申请或真实生图 Provider。当前 `v2-dev` 已完成 Goal13–18，但专家团、记忆和真实生图仍未实现；真实生成按钮保持禁用。

## Pro 审查问题修复

已修复审查中列出的数据正确性问题：跨场镜头排序、生成任务外键删除、任务总时长失步、多行操作非原子、旧项目无迁移。新增批量事务命令、迁移前备份以及相应回归测试。

产品闭环同步补齐：短片扩展为系列、内容类型工作区能力、跨工作区选区清理、活动变更集、生成任务重新组合、镜头资产关系、需求多来源、图片满足需求关系、关键帧资产继承显示、孤立媒体清理和项目副本新身份。

自动化覆盖增加至前端派生逻辑测试，以及后端迁移、批量回滚、排序撤销、生成任务时长与删除、媒体清理、项目复制和 30 集 / 500 镜头规模测试；仓库加入 Windows GitHub Actions CI。

0.1.2 复查收尾进一步修复：资产需求父资产跳转、关键帧父镜头跳转；媒体清理保护快照及可撤销历史；镜头/资产领域删除完整记录并恢复关联；数据库 V3 回填旧来源并增加唯一约束；新增独立 Windows Tauri 发布构建工作流。

## V2 Goal13 说明

`app.db` 与 `project.db` 都使用顺序版本号、迁移前 WAL checkpoint、文件备份、事务、外键检查和完整性检查。新建项目直接创建 V4；旧项目从 V1–V3 打开时自动备份并升级。项目副本保留正式业务事实，但清理 Agent 会话、消息、任务、上下文包、Patch、AI Card 与专家覆盖配置。

本阶段只冻结并实现工程边界，没有提前实现 Goal14 之后的 Agent 服务或占位业务逻辑。8 个 V2 功能开关默认关闭，因此 V1 界面和写入链路保持不变。

## V2 Goal14 说明

Pi Runtime 采用官方 headless RPC 模式，通过 stdin/stdout 严格 JSONL 通信。适配器每个任务使用独立进程，启动参数固定为 `--mode rpc --no-session --no-tools`，因此当前只消费显式传入的只读上下文，不暴露文件写入工具。支持 `prompt`、流式 `message_update/text_delta`、工具事件、`abort` 和 `agent_end`，并将其转换为工作台统一事件。

开发环境从 PATH 查找 `pi`，或读取 `PI_AGENT_CLI`。Windows 会解析 npm 安装产生的 `pi.cmd`，测试固定覆盖中文和空格路径；取消 500ms 后仍未退出会强制回收，Runtime Drop 也会清理全部子进程。当前本机未安装真实 Pi、未配置 Provider/API Key，验收使用遵循同一官方协议的 sidecar fixture 与 MockRuntime，不把任何密钥或未知 Pi API 写入业务层。自动化现为前端 6 项、Rust 19 项。

## V2 Goal15 说明

ContextService 只接受带项目 ID 和 revision 的 SelectionSnapshot。字段级选择只保留中心对象身份字段和目标字段，再按优先级加入明确多选、父链、前后邻居、正式 Relation 与 `shot_assets` 关联资产；绝对文件路径和 `secret_ref` 会在进入 ContextPackage 前移除。构建和持久化处于同一 SQLite 事务快照，revision 不匹配直接拒绝。

上下文采用固定 `context-v1` 策略版本、偏保守的中英文 token 估算和硬 budget；非中心候选超限会进入 `omittedSummary`，中心超限会明确标记截断。checksum 绑定 revision、策略、任务意图、专家、中心引用、实际条目和遗漏列表；同一输入生成相同 checksum。

FTS5 只在用户或上层服务显式调用搜索接口时建立临时索引，普通字段任务不会触发全文扫描或自动加入搜索结果。测试项目包含 33 个镜头，修改镜头04构图的 ContextPackage 只含目标字段、镜头03/05、父链和正式资产关系，明确验证远端镜头文本未进入上下文。自动化现为前端 6 项、Rust 21 项。

## V2 Goal16 说明

PermissionService 从 `agent_tasks.write_scope_json` 读取当前写入范围，按“保护范围优先、明确对象/字段授权、其余需要确认”分类 PatchItem。Agent 只能创建提案和卡片，没有直接写入工具；`patch_apply` 是唯一将提案转为项目事实的入口，调用已有 MutationService 生成 `source_type=agent` 的单一 ChangeSet。

应用提案会在同一 SQLite 事务中校验 base revision、每个字段 old value、对象存在性和最新权限状态，再处理一次性批准/拒绝、批量写入、权限卡 resolution 与提案状态。任何 revision、旧值、对象或相关写入范围变化都会将提案和待处理项持久化为 stale；保护字段即使被列入批准项也会拒绝。权限卡不能通过普通 `card_resolve` 绕过该事务。自动化现为前端 6 项、Rust 31 项。

## V2 Goal17 说明

MainAgent 通过 `agent_create_session`、`agent_send_message` 和 `agent_get_task` 管理持久化会话与任务。IntentResolver 使用当前对象、字段、工作区和请求关键词确定唯一专家；信号不足或并列时创建结构化澄清结果并停留在 `waiting_for_user`，不会同时调用全部专家。ExpertRegistry 固定六类专家的职责、上下文、写入边界和禁止项，项目覆盖表可替换 Provider/模型或禁用某专家。

专业任务先写入 `context_building`，ContextPackage 成功后进入 `queued`，Runtime 事件持久化 `running/completed/waiting_for_user/cancelled/failed`。专家提示只包含有预算的 ContextPackage、WriteScope 和统一 JSON 输出契约；结果被归一化后写入 assistant message 与 task result。含修改项的结果调用 Goal16 服务生成 PatchProposal，不直接调用 MutationService。路由验收逐条覆盖编剧、导演/分镜、摄影、美术、关键帧和提示词六个固定案例，并验证含糊请求先澄清。自动化现为前端 6 项、Rust 34 项。

## V2 Goal18 说明

AgentPanel 嵌入现有三栏 Workbench 的右侧区域，不新增独立 AI 页面。面板从 SelectionStore 构建 SelectionSnapshot；无显式对象时回退到当前内容单元或项目，因此作品结构、剧本、分镜、资产、关键帧、生成任务和历史工作区都能围绕当前上下文使用 Agent。多镜头编辑只授权摄影字段并显式保护叙事字段；讨论和建议模式写入范围为空。

会话启动后加载持久化消息，并从数据库刷新最新 Patch 状态；Runtime 事件按任务 ID 绑定，避免快速完成任务在 React 状态切换期间丢失终态。任务结果中的 AI Card、权限卡和 Patch 差异可在面板内处理，应用仍调用 Goal16 的原子 `patch_apply`。后端完成入口按 task type 强制执行只读模式，模型在讨论/建议模式违规返回的 Patch 会被丢弃并记录风险。自动化现为前端 8 项、Rust 35 项；Windows 实机验证了禁用/启用、模式切换、写入范围、折叠和跨工作区保留。
