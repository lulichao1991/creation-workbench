# 创作工作台 V2 beta.1 构建复查

版本：`0.2.0-beta.1`

> 本文是 Goal25 历史发布审计；Goal26–34 / Beta 2 的最新证据见 `BETA2_GOAL34_REVIEW.md`。

复查日期：2026-09-01
目标分支：`v2-dev`

## 发布门槛

| 检查项 | 结论 | 可重复证据 |
| --- | --- | --- |
| V1 兼容 | 通过 | `mutation::tests::goal_01_to_12_end_to_end_acceptance` 创建 30 集《智斗游戏》，验证剧本、分镜、资产、关键帧、生成任务、历史、快照、关闭重开与稳定 ID；`app_database::tests::creates_app_database_with_all_features_off` 验证 V2 默认关闭。 |
| 数据迁移 | 通过 | project.db 当前 V10、app.db 当前 V5；迁移测试验证旧库备份、顺序迁移、列幂等检查、保留开关值及完整性。 |
| Agent 权限 | 通过 | PermissionService 测试覆盖受保护字段、越界对象、一次性确认、拒绝、批量原子写入和 WriteScope 变化；Agent 没有 Mutation 工具。 |
| stale 保护 | 通过 | revision、oldValue、对象删除、WriteScope、ChangeSet 分析、生图提示词、提示词正式稿和专家团均有 stale 拒绝测试。 |
| 密钥安全 | 通过 | Provider 密钥只经 Windows Credential Manager 读写，数据库只存 `secret_ref`；tracked 文件扫描无常见 Key 模式和凭据文件；Context 会删除 `path`、`*_path`、`secret_ref`。 |
| 上下文质量 | 通过 | ContextPolicy v3 覆盖中心完整事实、专业邻居、场正文、项目后代、StoryElement Occurrence；token budget、checksum、revision 和事实优先均有测试。 |
| 记忆冲突 | 通过 | 只有同范围、同分类且同 `memoryKey` 的 active 记忆互斥；普通同类偏好允许并存，长期记忆仍需确认。 |
| 生图候选 | 通过 | 参考图真实进入 `images/edits` multipart；本机 HTTP fixture 验证 endpoint、image part、文件名和提示词；异步请求可在取消时中止。 |
| 提示词编译 | 通过 | 正文包含资产视觉定义且不含本地路径；正式资产/关键帧通过独立参考图清单返回，并保留版本、source map、warnings 和人工 override。 |
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

## Windows beta.1 构建产物

- 免安装版：`创作工作台_0.2.0-beta.1_windows_x64_免安装版.zip`
  - 大小：7,163,508 bytes
  - SHA256：`2A10305559C5E1E97B5CAE8582BFFA2EC632CD92EB55D103DDB19D288166D09B`
- NSIS 安装包：`src-tauri/target/release/bundle/nsis/创作工作台_0.2.0-beta.1_x64-setup.exe`
  - 大小：5,105,682 bytes
  - SHA256：`EDE86752D02057E356EFF023367382773EC960ABC7301DE6E46C8DD6E8CDA2D8`
- Release EXE：`portable/创作工作台.exe`
  - 大小：19,380,224 bytes
  - SHA256：`49BFFE229BD391F5708ED2F3D69667F481D605F7E702E30640F2F13A9FDDA52B`

beta.1 已完成 TypeScript 生产构建、63 项 Rust 测试、17 项前端测试、Clippy `-D warnings` 与 Windows x64 Release/NSIS 构建。当前机器未安装 Pi，真实 Pi + 真实模型仍须通过 Agent 面板 Runtime 检测后执行端到端验收，未满足前不定义为 RC。
