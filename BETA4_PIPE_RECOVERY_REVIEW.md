# Beta4 Windows Agent Host 管道恢复验收

版本：`0.2.0-beta.4`  
日期：2026-09-01

## 结论

已在用户实际运行的 Beta3 免安装版复现 `管道正在被关闭。 (os error 232)`。主程序仍在运行，但 Pi SDK Agent Host 子进程已经退出；Rust Adapter 仍持有旧 `ChildStdin`，后续 Runtime Doctor 或 Agent 请求继续向已关闭的 Windows 管道写入。

Beta4 在共享 `HostProcess` 入口统一检查子进程状态。旧 Host 已退出时，Adapter 会回收旧进程、清空只属于旧进程的 Session 映射并启动新 Host；写入竞争仍发生时，不再直接暴露系统错误码，而是返回可执行的重试提示。

## 回归证据

- 新增回归测试：先启动真实 mock Host，强制终止子进程，再执行 Doctor；Adapter 自动启动新 Host 并返回 `mock-sdk`。
- Rust：81 项通过。
- 前端：17 项通过。
- Agent Host：13 项通过。
- `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings` 通过。
- Windows Release、NSIS 与免安装包构建通过。
- 免安装包内应用、私有 Node、Host、Pi SDK 和 README 五项完整性检查通过；Host Doctor 返回 Pi SDK `0.84.4`、40 个 Provider、1290 个模型，Tool Gateway 正常。

## Windows 产物

- 安装版：`src-tauri/target/release/bundle/nsis/创作工作台_0.2.0-beta.4_x64-setup.exe`，45,951,334 bytes，SHA-256 `123FC7EA090289675DCD0A6AD3CCFBF669E236F4E48919A63350415FDBEAF19D`。
- 免安装版：`创作工作台_0.2.0-beta.4_windows_x64_免安装版.zip`，96,799,057 bytes，SHA-256 `E4EFC26B53F6223927B47B2C721DCE3310BE76FFF2B5EB658FA60A8CD340BEC0`；ZIP 共 22,951 项。

真实外部文本/视觉模型验收仍需用户先在应用内配置 Provider API Key。
