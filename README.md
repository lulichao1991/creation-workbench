# 创作工作台 V2 开发线

本地优先的 AI 视频创作工作台。`v2-dev` 当前处于 `0.2.0-alpha.4` 权限与修改提案阶段；所有 V2 功能开关默认关闭，未配置时不调用 AI 或真实生图服务，V1 工作流保持完整可用：

`作品结构 → 剧本 → 分镜 → 资产 → 关键帧 → 生成任务 / 提示词 → 历史与快照`

## 技术栈

- Tauri 2
- React 19 + TypeScript
- Rust + SQLite（rusqlite）
- Zustand
- Vitest

## 开发

```bash
npm install
npm run tauri dev
```

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

图像生成按钮在第一阶段保持禁用；资产图和关键帧图通过手动导入进入正式事实层。

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

- 业务层通过统一 `AgentRuntime` 契约使用 Runtime；Rust 端提供 `PiRuntimeAdapter`，前端冻结对应 TypeScript 类型；
- Pi 以独立进程运行，使用 `pi --mode rpc --no-session --no-tools` 和严格 LF 分隔 JSONL 通信；
- 支持文本增量事件、工具事件映射、追加输入、查询状态和 `abort` 取消；应用退出或取消超时会回收子进程；
- Windows 测试覆盖 npm 常见 `.cmd` 入口、中文与空格路径、流式输出、取消和无孤儿进程；
- 开发期从 PATH 查找 `pi`，也可通过 `PI_AGENT_CLI` 指定路径。当前 `agent_core` 开关仍默认关闭；模型配置、密钥读取和 Agent UI 属于后续 Goal。

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
- 主 Agent、专业 Agent 路由和右侧 Agent UI 仍属于 Goal17–18。
