# 创作工作台 V2 RC 发布复查

版本：`0.2.0-rc.1`

复查日期：2026-09-01
目标分支：`v2-dev`

## 发布门槛

| 检查项 | 结论 | 可重复证据 |
| --- | --- | --- |
| V1 兼容 | 通过 | `mutation::tests::goal_01_to_12_end_to_end_acceptance` 创建 30 集《智斗游戏》，验证剧本、分镜、资产、关键帧、生成任务、历史、快照、关闭重开与稳定 ID；`app_database::tests::creates_app_database_with_all_features_off` 验证 V2 默认关闭。 |
| 数据迁移 | 通过 | project.db 当前 V9；`database::tests::migrates_old_projects_with_backup_and_versioning` 验证旧库备份并顺序迁移；app.db 迁移测试验证保留开关值并备份。 |
| Agent 权限 | 通过 | PermissionService 测试覆盖受保护字段、越界对象、一次性确认、拒绝、批量原子写入和 WriteScope 变化；Agent 没有 Mutation 工具。 |
| stale 保护 | 通过 | revision、oldValue、对象删除、WriteScope、ChangeSet 分析、生图提示词、提示词正式稿和专家团均有 stale 拒绝测试。 |
| 密钥安全 | 通过 | Provider 密钥只经 Windows Credential Manager 读写，数据库只存 `secret_ref`；tracked 文件扫描无常见 Key 模式和凭据文件；Context 会删除 `path`、`*_path`、`secret_ref`。 |
| 上下文质量 | 通过 | 字段级 Context 黄金测试验证中心字段、父链、前后邻居和正式关系，远端镜头不进入；token budget、checksum、revision 和事实优先均有测试。 |
| 记忆冲突 | 通过 | 候选不进 Context，active 冲突必须显式 `supersedesId`，长期记忆需确认，事实优先于记忆；500 条项目记忆测试通过。 |
| 生图候选 | 通过 | Mock Provider 覆盖成功、部分失败、取消、参考图、候选隔离、stale、资产/关键帧转正和媒体清理；未确认不写正式事实。 |
| 提示词编译 | 通过 | 覆盖稳定编译、多模型差异、档案/模板版本、source map、warnings、人工 override 和二次确认原子转正；不存在视频调用。 |
| 关系图与规模 | 通过 | UI 关系数据限制 1000 条、画布 200 条；发布规模测试同时装载 30 集、500 镜头、100 资产、300 图片、1000 关系、500 记忆和 100 个 Agent 任务，并在 5 秒门槛内完成 ProjectState 装载。 |
| Windows 发布 | 通过 | `npm run tauri build` 生成 x64 NSIS 与独立 `workbench.exe`；免安装 ZIP 使用 Release EXE，另记录 SHA256。 |
| 禁止能力 | 通过 | 源码扫描不存在视频 Provider、视频 Job、视频 Adapter、视频生成命令或 UI；没有剪辑、配音和发布功能。 |

## 《智斗游戏》端到端验收映射

1. 项目级讨论 30 集结构：30 集项目与项目级 Context/主 Agent 路由测试通过。
2. 创建角色线和伏笔链：StoryElement、Occurrence、时间轴和未回收伏笔测试通过。
3. 字段级摄影修改不越权：字段级 WriteScope 与 protected 字段测试通过。
4. 多镜头摄影修改遵守保护字段：前端派生权限与后端越界拒绝测试通过。
5. 分析本轮手工修改：只允许用户 ChangeSet、空 WriteScope、直接影响与 stale 测试通过。
6. 创建项目记忆并在后续任务生效：active 记忆进入 Context、候选隔离和来源审计测试通过。
7. 生成资产专业提示词：美术 Agent 路由、资产需求和 Context 测试通过。
8. Mock / 真实 Provider 返回候选资产图：Mock 完整覆盖，真实 Provider 适配器及凭据边界通过；发布验收不发送真实计费请求。
9. 选择正式资产：候选转正原子写入 AssetMedia、Requirement 关联与 ChangeSet 测试通过。
10. 生成并选择关键帧：候选转正 ready Keyframe 测试通过。
11. 编译目标模型视频提示词：多模型编译、warnings、source map 与人工覆盖测试通过。
12. 专家团经确认只读会诊：Windows 实机验证申请卡、高成本确认、三位专家独立上下文、主 Agent 共识/分歧/建议综合；项目 revision 保持不变。
13. 所有 AI 修改可撤销：AI Patch 统一经 Mutation/ChangeSet；ChangeSet 撤销、快照恢复和正式媒体历史保护测试通过。
14. 项目关闭重开后状态完整：V1 端到端和 Windows 实机重开均通过，SQLite 外键与完整性检查为 `ok`。
15. 全程没有视频生成入口：tracked 源码符号和命令注册扫描通过。

## 标准验证命令

```text
npm ci
npm test -- --run
npm run build
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --manifest-path .\src-tauri\Cargo.toml --all --no-fail-fast
cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
npm run tauri build
```

## Windows RC 构建产物

- 免安装版：`创作工作台_0.2.0-rc.1_windows_x64_免安装版.zip`
  - 大小：7,087,017 bytes
  - SHA256：`E85A3073BAF87A5C50E0251286765EE6F93BC62FB59B804643CB60DF447CBC0F`
- NSIS 安装包：`src-tauri/target/release/bundle/nsis/创作工作台_0.2.0-rc.1_x64-setup.exe`
  - 大小：5,054,309 bytes
  - SHA256：`63AA67255905E12490ABBAD6C6CC6FA7C86D4684308E6A6BEA7165A50965042D`
- Release EXE：`portable/创作工作台.exe`
  - 大小：19,131,904 bytes
  - SHA256：`318E72DF42D81CDCD0FD9992D64C043D71417C63235577A89B16EDC92EA3415D`

免安装 EXE 已在 Windows 真实窗口启动，并成功重开 revision 11 的验收项目；专家团成员状态、主 Agent 综合结果和项目事实完整保留。
