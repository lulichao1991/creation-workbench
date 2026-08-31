# Goal 01–12 实施状态

验证日期：2026-08-31（0.1.2 V1 发布候选收尾）

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

## 第一阶段边界

未实现第二阶段的 Pi Agent、主 Agent、专业 Agent、专家团、记忆、权限申请或真实生图 Provider。界面已保留 Agent 区域，代码已保留 `ImageGenerationProvider` 与 `ImageGenerationSystem` 抽象；真实生成按钮保持禁用。

## Pro 审查问题修复

已修复审查中列出的数据正确性问题：跨场镜头排序、生成任务外键删除、任务总时长失步、多行操作非原子、旧项目无迁移。新增批量事务命令、迁移前备份以及相应回归测试。

产品闭环同步补齐：短片扩展为系列、内容类型工作区能力、跨工作区选区清理、活动变更集、生成任务重新组合、镜头资产关系、需求多来源、图片满足需求关系、关键帧资产继承显示、孤立媒体清理和项目副本新身份。

自动化覆盖增加至前端派生逻辑测试，以及后端迁移、批量回滚、排序撤销、生成任务时长与删除、媒体清理、项目复制和 30 集 / 500 镜头规模测试；仓库加入 Windows GitHub Actions CI。

0.1.2 复查收尾进一步修复：资产需求父资产跳转、关键帧父镜头跳转；媒体清理保护快照及可撤销历史；镜头/资产领域删除完整记录并恢复关联；数据库 V3 回填旧来源并增加唯一约束；新增独立 Windows Tauri 发布构建工作流。
