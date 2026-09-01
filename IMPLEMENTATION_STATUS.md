# Goal 01–25 与 V2 beta.1 实施状态

验证日期：2026-09-01（0.2.0-beta.4，Pi 原生 Agent RC 加固）

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
| 14 Pi Runtime 接入 | 完成 | Rust `AgentRuntime` 与 TypeScript Runtime 契约；流式文本、工具、追加输入、状态与取消；早期外部命令行原型已由 Goal33 的内置 Pi SDK Agent Host 取代 |
| 15 上下文系统 | 完成 | SelectionSnapshot / ContextPolicy / ContextPackage；字段级中心事实；父链、邻居、Relation 与镜头资产结构查询；FTS5；token budget；稳定 checksum；revision 校验和事务快照；V4 持久化 |
| 16 权限、修改提案与 AI 卡片 | 完成 | WriteScope / ProtectedScope 权限判断；Patch old/new 差异；权限卡；revision、旧值、对象与权限变化 stale 校验；一次性批准；同事务批量 Mutation 与 Agent ChangeSet；9 个权限安全测试 |
| 17 主 Agent 与单专业 Agent | 完成 | MainAgent 应用服务；IntentResolver；ExpertRegistry / ExpertRouter；六类专家配置；会话/任务持久化；Context/Pi/Patch 串联；统一结构化输出；固定路由测试与含糊澄清 |
| 18 Agent 右侧工作区 | 完成 | 全工作区共用会话 UI；当前选区、revision 与写入/保护范围；流式输出、专家状态和停止；讨论/建议/编辑模式；AI Card 与 Patch 差异；应用/拒绝/讨论；只读模式后端兜底；Windows 实机验收 |
| 19 分析本轮修改 | 完成 | 用户显式触发的 ChangeSet 只读任务；old/new 差异、受影响对象、父级、相邻对象与直接关系上下文；问题/建议卡；影响与复查范围；跨剧集确认边界；revision stale 保护 |
| 20 高级作品结构与关系图 | 完成 | V5 StoryElement / Occurrence / GraphLayout；时间轴、关系图、剧集表；故事语义与聚焦模式；计划/事实问题提示；布局不改变 revision；30 集与 1000 关系测试 |
| 21 记忆系统 | 完成 | project.db V6 与 app.db V2；项目/内容单元/长期记忆；来源、状态与使用任务审计；FTS 搜索；显式冲突替代；事实优先的 Context v2；500 条容量测试；右侧记忆面板 |
| 22 真实静态生图 | 完成 | project.db V7 Job/Result 与候选目录；app.db V3 Provider 配置；Windows Credential Manager 密钥；OpenAI Compatible/Mock Adapter；成本二次确认；资产/关键帧生图；显式转正与 stale 保护；Windows 实机闭环 |
| 23 提示词编译器 | 完成 | project.db V8 编译历史；app.db V4 模型档案/模板；稳定结构化编译；能力警告与来源映射；人工 override 保留；显式原子设为正式稿；无视频调用 |

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

V1 `0.1.2` 不包含 Pi Agent、主 Agent、专业 Agent、专家团、记忆、权限申请或真实生图 Provider。当前 `v2-dev` 已完成 Goal13–23；专家团仍按后续 Goal 保持关闭，提示词编译器默认关闭且不含任何视频生成调用，静态生图不会自动调用或自动转正。

## Pro 审查问题修复

已修复审查中列出的数据正确性问题：跨场镜头排序、生成任务外键删除、任务总时长失步、多行操作非原子、旧项目无迁移。新增批量事务命令、迁移前备份以及相应回归测试。

产品闭环同步补齐：短片扩展为系列、内容类型工作区能力、跨工作区选区清理、活动变更集、生成任务重新组合、镜头资产关系、需求多来源、图片满足需求关系、关键帧资产继承显示、孤立媒体清理和项目副本新身份。

自动化覆盖增加至前端派生逻辑测试，以及后端迁移、批量回滚、排序撤销、生成任务时长与删除、媒体清理、项目复制和 30 集 / 500 镜头规模测试；仓库加入 Windows GitHub Actions CI。

0.1.2 复查收尾进一步修复：资产需求父资产跳转、关键帧父镜头跳转；媒体清理保护快照及可撤销历史；镜头/资产领域删除完整记录并恢复关联；数据库 V3 回填旧来源并增加唯一约束；新增独立 Windows Tauri 发布构建工作流。

## V2 Goal13 说明

`app.db` 与 `project.db` 都使用顺序版本号、迁移前 WAL checkpoint、文件备份、事务、外键检查和完整性检查。新建项目直接创建 V4；旧项目从 V1–V3 打开时自动备份并升级。项目副本保留正式业务事实，但清理 Agent 会话、消息、任务、上下文包、Patch、AI Card 与专家覆盖配置。

本阶段只冻结并实现工程边界，没有提前实现 Goal14 之后的 Agent 服务或占位业务逻辑。8 个 V2 功能开关默认关闭，因此 V1 界面和写入链路保持不变。

## V2 Goal14 说明

Goal14 建立的 Runtime 契约现由 Beta 2 内置 Pi SDK Agent Host 实现。Host 通过 stdin/stdout 严格 LF-JSONL 与 Rust 通信，支持流式文本、工具事件、追加输入、取消、Session 恢复和终态回收；默认不注册文件写入或 Shell 工具。

Goal33 已删除早期外部命令行实现和系统 PATH 依赖。正式 Windows 包携带私有 Node Runtime、固定 Pi SDK、Agent Host 与生产依赖；模型凭据统一由工作台 UI 和 Pi ModelRuntime 管理。

## V2 Goal15 说明

ContextService 只接受带项目 ID 和 revision 的 SelectionSnapshot。字段级选择只保留中心对象身份字段和目标字段，再按优先级加入明确多选、父链、前后邻居、正式 Relation 与 `shot_assets` 关联资产；绝对文件路径和 `secret_ref` 会在进入 ContextPackage 前移除。构建和持久化处于同一 SQLite 事务快照，revision 不匹配直接拒绝。

上下文最初采用固定 `context-v1` 策略版本；Goal21 在不改变事实装载顺序的前提下升级到 `context-v2`，仅在事实之后加入少量已生效记忆。系统继续使用偏保守的中英文 token 估算和硬 budget；非中心候选超限会进入 `omittedSummary`，中心超限会明确标记截断。checksum 绑定 revision、策略、任务意图、专家、中心引用、实际条目、记忆 ID 和遗漏列表；同一输入生成相同 checksum。

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

## V2 Goal19 说明

右侧面板只在存在活动 ChangeSet 时显示“分析本轮修改”，普通手工编辑仍只记录 ChangeSet，不启动 Agent。用户点击后启用 `change_analysis` 开关并创建强制空 WriteScope 的 `change_analysis` 任务；后端拒绝非用户 ChangeSet、空变更集、错误中心对象或任何写入授权。

ContextPackage 以 ChangeSet 为中心，结构化解析每条 Change 的 old/new 值，并在预算内加入仍存在的受影响对象、父链、相邻对象和直接 Relation；删除对象仍由 ChangeSet 中的旧值保留证据。提示明确限定默认传播深度为直接关系和同剧集，跨剧集深挖必须返回确认要求，且禁止自动更新 `sync_status`。

结构化结果落地为问题卡、建议卡、受影响对象列表和建议复查范围；用户可以讨论、忽略、标记受影响或发起专业 Agent 复查，标记动作只解决 Card，不修改项目事实。任务完成时会比较 base/current revision；分析期间或后续读取发现项目事实变化时，任务持久化为 stale。自动化现为前端 9 项、Rust 37 项。

## V2 Goal20 说明

项目库 V5 把 StoryElement 与 StoryElementOccurrence 作为正式创作事实纳入 Mutation、ChangeSet、撤销和快照；GraphLayout 只保存当前范围、视图、筛选与布局偏好，独立命令明确验证不会增加项目 revision。新建故事元素和首个节点可由同一个批量 Mutation 原子提交。

作品结构工作区在现有 Overview 内增加时间轴矩阵、轻量 SVG 关系图和可编辑剧集表，不引入图数据库或第三方图库。所有视图限定到当前项目/季/剧集层级；关系模型最多装载 1000 条，画布最多绘制 200 条，并支持人物线、伏笔、未回收伏笔、受影响内容和计划/事实不一致聚焦。

计划层与事实层检查只产生问题提示：检测已有计划但未落剧本、已有事实但缺一句话剧情、文字明显偏离，以及已埋下但未回收的伏笔，绝不自动改写。StoryElement 与 Occurrence 已进入 ContextService；选中故事元素时，Agent 只能修改属于该元素的已有节点，其他元素仍需用户确认扩权。自动化现为前端 14 项、Rust 40 项。

## V2 Goal21 说明

项目记忆保存在 `project.db` V6，按项目或内容单元范围管理；跨项目长期记忆保存在 `app.db` V2，不随项目复制、导出或快照移动。每条记录保存分类、来源、置信度、优先级、状态和显式替代链，来源表与 ContextPackage 的 `memory_ids_json` 提供“从哪里来、被哪些任务使用”的审计路径。

候选记忆只供人工复核；只有 active 记录会进入 Context。激活同范围同分类记忆时，如果已有 active 记录，服务会返回 `MEMORY_CONFLICT`，必须携带明确的 `supersedesId` 才能在同一事务中替代旧记录。长期记忆 active 状态还需要显式 `confirmed=true`，因此单次对话或后台流程不能自动形成跨项目长期记忆。

ContextService `context-v2` 先装载并裁剪项目事实，再按当前内容单元、祖先、项目范围和优先级加入最多 8 条项目记忆以及少量已确认长期记忆；预算不足时记忆先被舍弃。Agent 提示再次强制事实优先，并要求在冲突时明确指出而不是用记忆覆盖事实。右侧面板支持搜索、创建、范围设置、来源/使用任务查看、编辑、激活、显式替代和失效；中文 FTS 无词元命中时使用参数化子串检索回退。自动化现为前端 14 项、Rust 46 项。

## V2 Goal22 说明

ImageGenerationService 只接受资产需求或关键帧目标，先校验目标、提示词、候选数量、参考图来源和项目 revision，再持久化 Job。真实 Adapter 使用配置的 HTTPS Base URL、超时与默认模型调用 OpenAI Compatible 图片生成接口；API Key 通过 Windows Credential Manager 读取，数据库只保存 `secret_ref`，前端只显示是否已配置，不回显任何密钥。

Provider 配置、模型、尺寸、质量、数量和成本提示在生成前可见，生成需要界面内二次确认。Job 结果只写入 `candidates/images/<job-id>/`；失败、取消和未选择候选不会生成 AssetMedia、Keyframe 事实或项目 ChangeSet。候选拒绝、归档和删除由独立状态管理，媒体清理会保护仍存在的候选。

转正时重新核对当前提示词，stale 候选直接拒绝。通过校验后才把候选复制到正式目录，并在同一个 SQLite 事务中写入 AssetMedia/Requirement 关联或 Keyframe、`source_type=image_generation` ChangeSet、结果 selected 状态和项目 revision；事务失败会补偿删除已复制文件。自动化现为前端 14 项、Rust 52 项；真实 Windows Tauri 窗口已用 Mock Provider 验证默认关闭、显式启用、费用提示、候选隔离、二次确认和正式图片落库。

## V2 Goal23 说明

PromptCompiler 以 GenerationTask 为编译中心，严格按 `generation_task_shots.sort_order` 读取镜头，并只引用已有的正式 AssetMedia、ready Keyframe 与 active 项目视觉记忆。ModelProfile 记录能力上限、参考图规则、建议约束和禁止模式，PromptTemplate 使用 `header / visual_rules / shots / constraints` 四类结构化 token；编译输出绑定模型档案版本、模板版本和来源 revision。

每次编译只新增 PromptCompilation，不修改 `generation_tasks.prompt`。用户可以保留编译稿、编辑独立 override、查看 warnings/source map 和对比当前正式稿；只有界面内二次确认后，后端才校验 expected revision，并在同一事务中通过 Mutation 写入当前正式提示词、目标模型、ChangeSet、编译记录 current 状态和项目 revision。重新编译不会覆盖已保存的人工 override。

本阶段没有视频 Provider、视频 Job、视频 HTTP Adapter 或视频生成命令。自动化现为前端 15 项、Rust 55 项，覆盖同一任务的多模型差异、稳定编译、版本与来源追踪、能力警告、编译/正式隔离及人工覆盖保留。

## V2 Goal24 说明

项目库 V9 新增专家团申请与成员记录。申请阶段只持久化 `waiting_for_user` 主任务，以及 `expert_team` 申请卡和 `cost` 高成本确认卡；成员 AgentTask、ContextPackage 和 Runtime 调用全部延迟到 `expert_team_confirm` 收到 `confirmed=true` 之后。通用 Card 解决命令不能处理专家团、成本或权限卡，因此无法绕过专用确认事务。

确认后每位成员使用不同 task ID 构建独立 ContextPackage，任务类型固定为 `expert_team_member`，WriteScope 强制为空，提示要求专家互不查看意见且只读分析。模型返回的 PatchProposal 或 permissionRequests 会被丢弃并记录风险。成员全部终止后，主 Agent 才以 `expert_team_synthesis` 任务整合共识、分歧、建议、问题和风险；综合结果仍为只读，若项目 revision 变化则持久化为 stale，修改只能另行走 Goal16 Patch 流程。

右侧 Agent 面板现可选择 2–6 位专家、生成申请卡、确认高成本、查看成员状态、取消任务并阅读结构化综合结果。自动化现为前端 16 项、Rust 58 项；Windows Tauri 实机使用三位专家验证了确认前零成员任务、确认后 3 个独立上下文、主 Agent 综合、零写入范围、零 PatchProposal 和 revision 不变。全程没有视频生成命令或入口。

## V2 Goal25 说明

`0.2.0-beta.3` 在 Goal26–34 基础上完成 RC 前本地加固：Agent API Key 由 Windows Credential Manager 持久化；单专家使用专业模型、thinking level 和焦点视觉附件；专家团综合使用独立无工具 Session，并由 Rust Gateway 强制任务级白名单；主 Agent、专业 Agent 和专家团均使用 TypeBox Result Tool；所有 Agent 结果带通用 stale 语义；终态 Runtime 任务缓存有界。`0.2.0-beta.4` 修复 Windows 下 Agent Host 意外退出后继续复用已关闭管道的问题：下一次请求会检测旧子进程、清理旧会话映射并自动启动新 Host。真实外部文本/视觉模型验收仍需用户配置 Provider API Key。Goal34 基线证据见 `BETA2_GOAL34_REVIEW.md`，Beta3 证据见 `BETA3_RC_HARDENING_REVIEW.md`。

新增发布规模回归同时装载 30 集、500 镜头、100 项资产、300 张正式图片、1000 条关系、500 条项目记忆和 100 个 Agent 任务，ProjectState 在 5 秒门槛内完成并通过完整性检查。自动化现为前端 16 项、Rust 59 项；tracked 文件扫描未发现常见明文密钥或凭据文件，源代码没有视频生成命令、视频 Job、视频 Adapter 或对应 UI。完整证据矩阵与《智斗游戏》15 项验收映射记录在 `V2_RELEASE_REVIEW.md`。
