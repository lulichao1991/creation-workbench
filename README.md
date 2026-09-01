# 创作工作台 V2 开发线

本地优先的 AI 视频创作工作台。`v2-dev` 当前处于 `0.2.0-beta.1` 核心闭环修复阶段；所有 V2 功能开关默认关闭，未配置时不调用 AI、真实生图或视频生成服务，V1 工作流保持完整可用：

`作品结构 → 剧本 → 分镜 → 资产 → 关键帧 → 生成任务 / 提示词 → 历史与快照`

## 技术栈

- Tauri 2
- React 19 + TypeScript
- Rust + SQLite（rusqlite）
- Zustand
- Vitest
- Pi SDK AgentSession（内置 Agent Host）

## 开发

```bash
npm install
npm run tauri dev
```

`npm install` 会安装隔离在 `agent-host/` 下并固定为 `0.84.4` 的 Pi SDK；开发与正式版都只使用内置 Agent Host，不加载用户 `~/.pi`、项目扩展、Skills 或 `AGENTS.md`。

## 验证

```bash
npm test
npm run build
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

## 发布构建

```bash
npm run tauri build
```

项目默认保存在用户“文档/AI视频工作台”目录，也可以在项目列表页更换根目录。每个项目拥有独立的 `project.db`、资产、关键帧、参考资料、导入、导出与缓存目录。

资产图和关键帧图既可手动导入，也可在用户显式启用、配置 Provider、确认本次参数与成本后生成候选；候选不会自动进入正式事实层。

## V1 发布候选基线（0.1.2）

- 整集镜头严格按“场顺序 → 场内镜头顺序”排列；
- 多对象排序、设置主图、短片扩展、生成任务操作均在单事务与单变更集中完成；
- 生成任务支持增删、替换、重排和删除，总时长由关联镜头自动重算；
- 数据库使用 `PRAGMA user_version` 顺序迁移，迁移前自动备份到项目 `backups/`；V3 会回填旧资产需求来源并阻止重复关系；
- 已建立镜头—资产、资产需求—多来源、资产图片—已满足需求三类正式关系；
- 季作为结构容器会禁用剧本、分镜、关键帧和生成任务工作区；
- 项目副本生成新的项目 ID 并清理原项目历史与快照；
- 资产需求和关键帧编辑始终保持正确的父资产与父镜头导航；
- 镜头和资产删除会显式记录全部依赖关系，撤销可恢复关系链及任务时长；
- 历史页可以清理孤立媒体，同时保护快照和可撤销历史仍引用的文件。

GitHub Actions 会在 Windows 上持续执行前端测试、TypeScript 构建、Rust 格式检查、测试与 Clippy；标签或手动触发的发布工作流会额外验证 Tauri/NSIS 构建并上传构建产物。

## V2 Goal13 工程基础（0.2.0-alpha.1）

- 应用数据目录新增独立 `app.db`，迁移前自动备份，并保存全局设置与功能开关；
- `agent_core`、专家 Agent、变更分析、故事图、记忆、生图、提示词编译和专家团共 8 个开关默认全部关闭；
- 项目库升级到 V4，新增 Agent 会话、消息、任务、上下文包、Patch、AI Card 与项目专家覆盖表；
- 项目复制会清理 Agent 运行数据，避免把原项目会话和临时结果带入副本；
- 建立 Agent / Context / Memory / Permission 前端契约目录，并开始拆分 Workbench；
- 尚未实现 Agent 调度、模型调用、记忆召回或 Patch 应用，这些属于后续 Goal。

## V2 Goal14 Pi Runtime（0.2.0-alpha.2）

- 该阶段建立了统一 `AgentRuntime`、流式事件、追加输入、状态查询和取消契约；
- 早期外部命令行原型已在 Beta 2 Goal33 完整删除，现由内置 Pi SDK Agent Host 实现同一契约；
- 当前实现不读取系统 PATH 中的 Pi、Node 或 npm，也不暴露内置文件和 Shell 工具。

## V2 Goal15 上下文系统（0.2.0-alpha.3）

- 冻结 `SelectionSnapshot`、`ObjectRef`、`ContextPolicy`、`ContextItem` 与 `ContextPackage` 前后端契约；
- 字段级上下文按中心字段、明确多选、父链、相邻对象、正式 Relation 与镜头资产关系的顺序装载；
- 构建过程使用同一个 SQLite 事务快照，校验项目 revision，并将结果写入 V4 `context_packages`；
- token budget 使用偏保守的中英文估算，超限时保留中心内容并明确记录遗漏项；相同 revision、策略和输入生成稳定 checksum；
- 显式全文搜索使用 SQLite FTS5 临时索引，与字段级上下文路径隔离，不把全文搜索或整个项目自动加入普通任务；
- 记忆注入、语义检索、上下文缓存和 Agent 自动路由仍属于后续 Goal。

## V2 Goal16 权限、修改提案与 AI 卡片（0.2.0-alpha.4）

- 新增 `PermissionService`、`PatchProposal` / `PatchItem` 与 `AICard` 的 Rust / TypeScript 契约和 Tauri 命令；创建接口用 `requestId` 作为稳定 ID，重复调用不会生成重复提案或卡片；
- 提案从 Agent 任务的 `write_scope_json` 推导字段权限，保护范围优先于普通授权；范围外字段进入显式权限卡，保护字段始终拒绝；
- `patch_get` 返回结构化 old/new 差异预览；`patch_apply` 逐项校验项目 revision、当前旧值、对象存在性和最新写入范围；过期提案持久化为 `stale` 且不能写入；
- 用户批准项、拒绝项、权限卡解决、Agent 来源 ChangeSet、批量业务修改与提案状态在同一个 SQLite 事务完成，成功修改仍可由 V1 历史系统撤销；
- 权限卡不能经普通 `card_resolve` 绕过应用事务；未确认的字段级越权、多选未选对象、保护字段、已删除对象、变更后旧值和写入范围变化都有拒绝回归测试；
- 主 Agent、专业 Agent 路由和右侧 Agent UI 已分别在 Goal17–18 完成。

## V2 Goal17 主 Agent 与单专业 Agent（0.2.0-alpha.5）

- 新增 MainAgent 应用服务，负责幂等创建会话/消息/任务、解析意图、建立 ContextPackage、调度单一专业 Agent、持久化状态和归一化结果；
- ExpertRegistry 固定注册编剧、导演/分镜、摄影、美术、关键帧和提示词六类专家，分别声明职责、默认读取、默认写入、禁止项与系统指令，并支持项目级 Provider/模型覆盖和禁用；
- IntentResolver 综合当前对象、字段、工作区和用户关键词；文档规定的六条路由样例均有固定测试，信号不足或并列时返回澄清问题，不启动多个专家；
- 专家只接收有预算和 checksum 的 ContextPackage 与当前 WriteScope，Pi 仍以无工具模式运行；输出统一收敛为 summary、findings、patchProposal、relatedImpacts、permissionRequests、questions、risks；
- 有修改的结构化结果会再次经过 Goal16 的 old-value 与权限检查并持久化为 PatchProposal，Agent 不获得 SQL、文件系统、MutationService、视频生成或正式图片选择入口；
- 流式会话、选区与写入范围展示、卡片和差异交互已在 Goal18 完成。

## V2 Goal18 Agent 右侧工作区（0.2.0-alpha.6）

- 所有现有工作区共用右侧主 Agent 面板；上下文条持续显示当前对象、模式、revision、写入范围和保护范围；
- 会话历史和任务结果持久化，支持流式文本、专业 Agent 状态、任务停止，以及讨论、建议、编辑三种模式；讨论和建议模式在后端强制只读，即使模型违规返回 Patch 也不会落库；
- AI Card、权限申请和 Patch 差异在对话内展示，可逐项选择、应用全部、拒绝或继续讨论；应用仍统一经过 Goal16 的权限、旧值和 stale 校验；
- 未启用时说明 Pi SDK Agent Host 已内置；用户显式启用后才打开 Agent 功能开关，不会在启用前发送模型请求；
- 面板支持收起并释放中央空间，已在真实 Windows Tauri 窗口中验证作品结构和资产工作区。

## V2 Goal19 分析本轮修改（0.2.0-alpha.7）

- 手工编辑只持续记录活动 ChangeSet，不会自动运行 Agent；右侧面板的“分析本轮修改”是唯一触发入口；
- ChangeSet 上下文包含字段 old/new 差异、受影响对象、父级、相邻对象和直接正式关系，并受 token budget 与同剧集传播边界约束；
- 主 Agent 以强制只读任务生成问题卡、建议卡、受影响对象和建议复查范围，不得自动修改 `sync_status` 或创建 Patch；
- 卡片支持逐项讨论、忽略、标记受影响和请求专业 Agent 复查；标记只记录 Card 决策，不改变项目事实；
- 分析结果绑定 base revision，分析期间或读取结果时发现项目 revision 变化会持久化为 stale，禁止继续按旧结论操作。

## V2 Goal20 高级作品结构与关系图（0.2.0-alpha.8）

- 项目库升级到 V5，新增 StoryElement、StoryElementOccurrence 与 GraphLayout；故事事实继续走 Mutation / ChangeSet，布局与筛选单独持久化且不增加项目 revision；
- 作品结构页提供按当前项目、季或剧集范围加载的故事时间轴、关系图和剧集表，支持主线、人物弧光、伏笔、事件、主题与自定义元素；
- 时间轴节点使用明确的主线、人物和伏笔语义；新建故事元素与首个节点可在同一事务完成，剧集表可直接维护一句话剧情、三类进度、成熟度和同步状态；
- 默认展示计划层与剧本/场/分镜事实层的缺失、偏离以及未回收伏笔问题，只提示人工复核，不自动改写任何项目事实；
- 支持人物线、伏笔、未回收伏笔、受影响内容和计划/事实不一致等聚焦模式；当前筛选可保存或重置，关系最多按需装载 1000 条、画布最多绘制 200 条；
- 选中 StoryElement 后 Agent 仍受该选择的 WriteScope 约束，只允许修改其已有 Occurrence，其他故事线继续要求显式扩权。

## V2 Goal21 记忆系统（0.2.0-alpha.9）

- 项目库升级到 V6，保存项目级和内容单元级记忆及来源；`app.db` 升级到 V2，跨项目长期记忆与项目文件完全分离；
- 记忆分为候选、生效、已替代和已失效四种状态；同范围同分类冲突不会静默覆盖，激活或替代必须由用户明确确认；
- 单次对话不会自动写入长期记忆；跨项目长期记忆只有在用户明确选择并确认后才能生效，项目记忆随项目复制，长期记忆不会进入项目副本或导出；
- ContextPackage 只召回当前内容单元、祖先和项目范围内少量生效记忆，并在项目事实之后加入；Agent 提示强制“事实始终优先于记忆”，候选、已替代和已失效记忆不会进入任务；
- 右侧 Agent 区域加入可折叠记忆面板，支持全文搜索、创建、范围设置、来源与使用任务查看、编辑、激活、明确替代和失效；
- 自动化覆盖 500 条项目记忆、当前单元优先级、长期记忆确认、冲突替代、幂等创建、事实优先和非生效记忆隔离。

## V2 Goal22 真实静态生图（0.2.0-alpha.10）

- 项目库升级到 V7，保存静态生图 Job、Result、候选状态、Provider 元数据、用量与错误；`app.db` 升级到 V3，保存全局 Provider 配置和系统密钥引用；
- 提供 OpenAI Compatible HTTP Adapter 与不联网的 Mock Provider；真实 API Key 只写入 Windows 凭据管理器，项目库、应用库明文、日志和导出均不保存密钥；
- 资产需求和关键帧共用静态生图面板，生成前明确展示 Provider、模型、尺寸、质量、数量和费用提示，并要求界面内二次确认；没有自动生成、后台连续生成或视频调用；
- 生成结果写入 `candidates/images/<job-id>/`，支持成功、部分成功、失败、超时、取消与幂等查询；未删除候选受媒体清理保护；
- 候选默认保持非正式状态，只有用户二次确认“选为正式”后，才会复制到正式资产/关键帧目录，并在同一事务中写入 Mutation、ChangeSet、需求关联和项目 revision；提示词变化会触发 stale 拒绝；
- 自动化覆盖候选隔离、资产与关键帧转正、部分失败/取消、幂等、参考图边界、候选状态与媒体清理；Windows 实机用 Mock Provider 完成启用、生成、候选展示和转正闭环。

## V2 Goal23 提示词编译器（0.2.0-alpha.11）

- `app.db` 升级到 V4，保存可版本化的 ModelProfile 与 PromptTemplate；`project.db` 升级到 V8，保存不可静默覆盖的 PromptCompilation 历史、模型/模板版本、来源 revision、warnings 与 source map；
- 编译器只读取生成任务、用户排序的镜头、正式资产媒体、正式关键帧和生效的项目视觉规则，按目标模型能力生成稳定文本；不调用 Provider、视频模型或任何视频生成入口；
- 对建议镜头数、时长、缺少正式关键帧、资产缺少正式媒体、起止帧能力、镜头复杂度和禁止模式给出结构化警告，不自动改写故事或补造事实；
- 编译结果、人工 override 与当前正式提示词分离。重新编译只新增历史记录，不覆盖人工稿；用户可编辑候选、对比正式稿，并通过界面内二次确认原子写入 Mutation、ChangeSet 与项目 revision；
- 同一任务可按不同模型档案和模板得到不同提示词；自动化覆盖稳定输出、版本追踪、source map、能力警告、正式稿隔离、人工覆盖保留和原子转正。

## V2 Goal24 专家团（0.2.0-alpha.12）

- 项目库升级到 V9，持久化专家团申请、成员任务、确认/运行/综合/完成/取消/失败/stale 状态；申请阶段只创建等待用户的主任务与“专家团申请”“高成本确认”两张卡；
- 未携带显式确认时后端拒绝启动，确认前不存在成员 AgentTask、ContextPackage 或 Runtime 调用；通用 `card_resolve` 不能绕过专家团和成本确认边界；
- 确认后每位专家获得独立 ContextPackage 和空 WriteScope，只读提示明确禁止参考其他专家意见；模型违规返回的 Patch 或扩权请求会被丢弃，绝不调用 MutationService；
- 全部成员完成后才创建独立的主 Agent 综合任务，结构化展示共识、分歧、建议、问题与风险；项目 revision 变化会把结果标记为 stale，任何修改必须另行建立 PatchProposal；
- 右侧 Agent 面板提供成员选择、申请卡、高成本二次确认、成员状态、取消和综合结果，不新增 AI 页面；自动化覆盖确认硬边界、独立上下文、默认只读、主 Agent 综合、取消与 stale；Windows 实机已用 Mock Pi 验证完整会诊闭环。

## V2 Goal25 发布候选（0.2.0-rc.1）

- 完整复查 V1 兼容、V9 顺序迁移、Agent 权限、stale、防明文密钥、上下文、记忆冲突、生图候选、提示词编译、关系图边界和 Windows 构建；
- 发布规模测试覆盖 30 集、500 镜头、100 项资产、300 张正式图片、1000 条关系、500 条项目记忆和 100 次 Agent 任务，并继续限制关系数据 1000 条、画布连线 200 条；
- 《智斗游戏》验收逐项映射到自动化与 Windows 实机证据，关闭重开后项目、会诊、历史和正式创作事实保持完整；
- RC 仍不提供视频 Provider、视频 Job、视频生成命令、剪辑、配音或发布入口。详细证据见 `V2_RELEASE_REVIEW.md`。

## V2 beta.1 核心闭环修复

- 主 Agent 新增有界 `SessionWorkingMemory`，连续三轮可引用上一轮方案；任务意图与讨论、建议、编辑模式已分离。
- `ContextPolicy v3` 提供完整中心事实、专业相邻事实、场正文、项目后代结构和 StoryElement 出现链。
- 美术、摄影、关键帧和提示词 Agent 可接收正式资产与当前关键帧图片；Pi 会先检查模型 `image` 能力，不支持时明确拒绝。
- 静态生图参考图真实进入 OpenAI Compatible `images/edits` multipart 请求；未明确允许上传时不会静默退化成纯文本生成。
- 新增 Pi Runtime 检测、终态资源回收、多季完整路径排序、`memoryKey` 冲突键，以及提示词编译参考图清单。
- 本机尚未安装 Pi，因此真实 Pi + 真实模型端到端验收仍是进入 RC 前的外部门槛；可在 Agent 面板运行 Runtime 检测。

## V2 Beta 2 Goal26 Pi SDK Agent Host

- 新增独立 TypeScript `agent-host`，直接使用 Pi `ModelRuntime` 与真实 `AgentSession`，通过严格 LF-JSONL 与 Rust 通信，不开放本地 HTTP 端口。
- Agent Host 使用独立系统数据目录和显式空资源加载器，不读取用户 Pi 环境或项目文件；默认禁用全部内置文件与 Shell 工具。
- Rust 新增 `PiSdkRuntimeAdapter` 并复用现有 Runtime 事件契约；Goal33 已将其升级为唯一正式 Runtime。
- 自动化覆盖真实 Pi SDK Session 创建、同一 Session 两轮上下文、Host Doctor、中文空格路径、流式事件以及前后端原有回归测试。

## V2 Beta 2 Goal27 MainAgentSession

- Pi SDK Session 由 SDK 原生 JSONL 持久化；应用或 Agent Host 重启后按 `runtime_session_id` 恢复完整上下文。
- `project.db` 记录主会话生命周期和 Pi Session 映射；运行中任务不能关闭，结束后的讨论可显式恢复。
- Agent 面板支持新建、切换、结束和恢复讨论，不再使用项目级硬编码会话 ID。
- Pi SDK 模式由 `AgentSession` 自己维护多轮上下文；旧 `SessionWorkingMemory` 仅在 Goal 33 前作为 legacy Runtime 回退。
- Runtime 桥接同时支持 `steer` 和 `follow_up`，自动化覆盖 Host 重启恢复、数据库映射、生命周期与忙碌会话保护。

## V2 Beta 2 Goal28 Custom Tool Gateway

- 真实 Pi Tool Loop 已注册 `get_selection`、对象/父子/相邻读取、场/镜头/资产/生成任务、故事结构、搜索、记忆和 ChangeSet 共 13 个只读工具。
- Agent Host 只传递工具名和结构化参数；项目路径、SQLite 与应用数据目录始终留在 Rust 进程，且没有文件系统、Shell、PowerShell 或数据库直连工具。
- Rust 对任务、会话、项目对象、参数白名单、当前 revision 和 64 KiB 结果上限逐次校验；资产图片仅返回不含本地路径的媒体元数据。
- 每次调用写入 `agent_tool_calls` 并更新任务 `tool_call_count`；审计摘要会截断并清除 Key、Token、Secret、路径和图片字段。
- 自动化覆盖真实 Pi 多工具循环、Rust↔Host 双向 IPC、全部首批读取工具、SQL 注入字符串、任意文件尝试、结果限额、revision 更新和审计。

## V2 Beta 2 Goal29 ContextSystem 工具化

- Pi SDK 普通对话只发送用户请求、选区引用与 WriteScope，不再预先构建或注入完整 ContextPackage；专项“分析本轮修改”和 legacy 回退仍保留可审计的批量上下文包。
- `ContextPolicy v3` 直接约束 `read_shot_context` 与 `read_story_structure` 等组合读取，Agent 可在同一 Tool Loop 中按需读取并再次读取当前项目事实。
- 工具网关同时限制约 12K token 和 64 KiB 结果，返回当前 policy、revision 与 token 估算；超限调用进入失败审计，不会把大结果送入模型。
- 相同任务、项目 revision、工具与参数的结果使用有界内存缓存；revision 变化自动产生新缓存键，确保后续读取取得新事实。

## V2 Beta 2 Goal30 Professional AgentSession

- Pi SDK 普通对话统一由真实 `MainAgentSession` 启动，Rust 关键词解析只保留给 legacy 回退和显式解析接口，不再预先决定 Pi 主流程的专业角色。
- Main Agent 新增 `call_expert`，可自主创建 Writer、Director、Cinematography、Art、Keyframe、Prompt 六类短生命周期 Pi `AgentSession`；每次调用都持久化父子 Session、专业 Task、Pi Session ID、结果和 Tool 审计。
- 每类专业 Session 使用独立系统指令和最小只读工具白名单，不能递归调用其他专家；Provider/模型沿用项目专业覆盖，摄影、美术、关键帧和提示词角色可接收当前正式视觉附件。
- 自动化使用真实 Pi SDK 跑通 `MainAgent → call_expert(cinematography) → read_shot_context → 专业意见 → MainAgent 综合`，并覆盖六类配置、专业工具审计、失败收口及专业 Session 禁止递归。

## V2 Beta 2 Goal31 专家团 Pi 化

- 保留既有“专家团申请 + 高成本确认”双卡硬边界；确认前仍不会创建成员 Task、Pi Session、ContextPackage 或 Runtime 调用。
- 确认后每位成员获得独立数据库子 Session、Pi `AgentSession`、专业系统指令和最小只读工具白名单；Pi 路径不再预打包项目 ContextPackage，而是在各自 Tool Loop 中按需读取事实，且不能调用其他专家。
- 编剧、导演和摄影 Session 可并行运行、互不读取彼此输出并产生角色化意见；摄影等视觉角色会直接收到当前选区关联的正式图片，所有成员 WriteScope 始终为空。
- 全部成员结束后由原 MainAgentSession 综合共识、分歧、建议、问题和风险；成员 Session 生命周期、Pi Session ID、结果、失败、取消、stale 与只读约束均持久化，任何 Patch 或扩权请求都会被丢弃。
- 自动化覆盖真实 Pi SDK 三 Session 并行三种 Tool Loop、不同专业输出，以及 Rust 侧确认边界、无预打包上下文、正式视觉附件、成员 Session 关闭和 Main 综合。

## V2 Beta 2 Goal32 Pi ModelRuntime 与模型设置

- Agent 面板新增“AI 模型设置”，直接列出 Pi `ModelRuntime` 内的 Provider 与模型，不要求用户进入 Pi CLI；模型条目展示视觉附件、推理能力和上下文窗口。
- 用户可在工作台内保存或注销 Provider API Key；Agent Host 的 `auth.json` 是 Agent 模型凭据的唯一权威源，`app.db` 只保存非敏感的模型选择与 thinking level，不复制密钥。
- 主 Agent 可设置默认 Provider、模型和 thinking level；编剧、导演、摄影、美术、关键帧和提示词六类专业 Agent 均可选择独立模型覆盖，未覆盖时沿用主 Agent。
- 普通 MainAgentSession、单专业调用和专家团成员启动时均会解析应用设置；项目级专业覆盖仍保持最高优先级，任务记录实际使用的 Provider 与模型。
- 自动化覆盖 ModelRuntime 模型/视觉能力枚举、API Key 登录与注销、非敏感设置持久化、专业覆盖解析，以及前端生产构建和完整回归测试。

## V2 Beta 2 Goal33 移除外部 Runtime 依赖

- 删除早期外部 Pi 命令行适配器、PATH 探测、运行参数和双 Runtime 环境开关；生产入口始终创建 `PiSdkRuntimeAdapter`，不存在回退到全局 Pi 的路径。
- 正式 Windows 包携带私有 Node Runtime、Agent Host 构建产物、固定 Pi SDK 与全部生产依赖；目标电脑不需要安装 Pi、Node 或 npm。
- Runtime 检测替换为 Agent Host Doctor，统一显示 Pi SDK 版本、ModelRuntime 状态、Provider 登录数、Session 健康和 Tool Gateway 健康。
- 应用启动时从 Tauri `resource_dir` 解析私有 Runtime 与 Host 脚本；开发测试仍可显式指定 Host fixture，但正式包不会执行系统 Shell 或 PATH 查找。
- 自动化覆盖私有 Runtime 路径解析、Agent Host Doctor、Provider/模型状态、Session 状态、Tool Gateway 状态及完整前后端回归测试。
