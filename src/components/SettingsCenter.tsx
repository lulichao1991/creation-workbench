import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { Bot, Database, Image, Info, Settings2, X } from "lucide-react";
import { useEffect, useState } from "react";

import { AgentModelSettingsPanel } from "./AgentModelSettingsPanel";
import { ImageProviderSettingsPanel } from "./ImageProviderSettingsPanel";

type SettingsTab = "agent" | "images" | "data" | "about";

interface Props {
  rootPath: string;
  disabled: boolean;
  onRootChange: (path: string) => Promise<void>;
  onClose: () => void;
  onError: (error: unknown) => void;
  onRestartOnboarding: () => void;
}

const tabs: Array<[SettingsTab, string, typeof Bot]> = [
  ["agent", "AI 模型", Bot],
  ["images", "图片生成", Image],
  ["data", "数据与存储", Database],
  ["about", "关于与诊断", Info],
];

export function SettingsCenter({ rootPath, disabled, onRootChange, onClose, onError, onRestartOnboarding }: Props) {
  const [tab, setTab] = useState<SettingsTab>("agent");
  const [version, setVersion] = useState("读取中…");

  useEffect(() => { void getVersion().then(setVersion).catch(() => setVersion("未知")); }, []);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const chooseRoot = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    try { await onRootChange(selected); } catch (error) { onError(error); }
  };

  return (
    <div className="settings-backdrop">
      <section className="settings-center" role="dialog" aria-modal="true" aria-labelledby="settings-title">
        <header className="settings-header">
          <div><Settings2 size={19} /><div><strong id="settings-title">全局设置</strong><small>服务、存储与软件状态</small></div></div>
          <button className="icon-button" aria-label="关闭设置" onClick={onClose}><X size={18} /></button>
        </header>
        <div className="settings-layout">
          <nav className="settings-nav" aria-label="设置分类">
            {tabs.map(([value, label, Icon]) => <button className={tab === value ? "active" : ""} onClick={() => setTab(value)} key={value}><Icon size={16} />{label}</button>)}
          </nav>
          <main className="settings-content">
            {tab === "agent" && <><div className="settings-intro"><span className="label">AI 服务</span><h2>AI 模型</h2><p>连接你使用的 AI 服务，并为新讨论选择默认模型。</p></div><AgentModelSettingsPanel disabled={disabled} onError={onError} /></>}
            {tab === "images" && <><div className="settings-intro"><span className="label">核心效率工具</span><h2>图片生成</h2><p>图片生成会贯穿角色、场景、道具、分镜和关键帧；服务配置全局复用。</p></div><ImageProviderSettingsPanel disabled={disabled} onError={onError} /></>}
            {tab === "data" && <><div className="settings-intro"><span className="label">本地优先</span><h2>数据与存储</h2><p>项目数据库、正式素材和历史记录都保存在项目目录中。</p></div><section className="settings-section"><label>项目根目录<code className="settings-path">{rootPath}</code></label><button className="secondary" disabled={disabled} onClick={() => void chooseRoot()}>更改项目目录</button><p className="settings-note">更改目录不会移动已有项目，只会改变首页默认扫描和新建项目的位置。</p></section></>}
            {tab === "about" && <><div className="settings-intro"><span className="label">软件信息</span><h2>关于与诊断</h2></div><section className="settings-section about-grid"><div><span>应用</span><strong>创作工作台</strong></div><div><span>版本</span><strong>{version}</strong></div><div><span>数据策略</span><strong>项目事实本地保存</strong></div><div><span>凭据策略</span><strong>Windows 系统密钥库</strong></div><button className="secondary" onClick={onRestartOnboarding}>重新运行首次使用引导</button></section></>}
          </main>
        </div>
      </section>
    </div>
  );
}
