# 创作工作台 V1

本地优先的 AI 视频创作工作台第一阶段实现。当前版本不调用 AI 或真实生图服务，可独立完成：

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

## V1 数据正确性基线（0.1.1）

- 整集镜头严格按“场顺序 → 场内镜头顺序”排列；
- 多对象排序、设置主图、短片扩展、生成任务操作均在单事务与单变更集中完成；
- 生成任务支持增删、替换、重排和删除，总时长由关联镜头自动重算；
- 数据库使用 `PRAGMA user_version` 顺序迁移，迁移前自动备份到项目 `backups/`；
- 已建立镜头—资产、资产需求—多来源、资产图片—已满足需求三类正式关系；
- 季作为结构容器会禁用剧本、分镜、关键帧和生成任务工作区；
- 项目副本生成新的项目 ID 并清理原项目历史与快照；
- 历史页可以显式清理未被数据库引用的媒体文件。

GitHub Actions 会在 Windows 上持续执行前端测试、TypeScript 构建、Rust 格式检查、测试与 Clippy。
