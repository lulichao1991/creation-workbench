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
