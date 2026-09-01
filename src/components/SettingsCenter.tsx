import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { Activity, Bot, CheckCircle2, CircleDashed, Clipboard, Database, Image, Info, RefreshCw, Settings2, X, XCircle } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../api";
import { toUserErrorMessage } from "../domain/userError";
import type { RuntimeDiagnostics } from "../features/agent/runtime";
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
  const [diagnostics, setDiagnostics] = useState<RuntimeDiagnostics | null>(null);
  const [diagnosticError, setDiagnosticError] = useState<string | null>(null);
  const [diagnosing, setDiagnosing] = useState(false);
  const [showDiagnosticDetails, setShowDiagnosticDetails] = useState(false);
  const [copied, setCopied] = useState(false);

  const runDiagnostics = async (showDetails: boolean) => {
    if (showDetails) setShowDiagnosticDetails(true);
    setDiagnosing(true);
    setDiagnosticError(null);
    try {
      setDiagnostics(await api.agentRuntimeDoctor());
    } catch (error) {
      setDiagnostics(null);
      setDiagnosticError(toUserErrorMessage(error));
    } finally {
      setDiagnosing(false);
    }
  };

  useEffect(() => { void getVersion().then(setVersion).catch(() => setVersion("未知")); }, []);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);
  useEffect(() => {
    if (tab === "about" && !diagnostics && !diagnosticError && !diagnosing) void runDiagnostics(false);
  }, [tab]);

  const chooseRoot = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    try { await onRootChange(selected); } catch (error) { onError(error); }
  };

  const copyDiagnostics = async () => {
    if (!diagnostics) return;
    const configuredProviders = diagnostics.providerAuth.filter((provider) => provider.configured).length;
    const overallHealthy = diagnostics.localDatabaseHealthy && diagnostics.agentHostHealthy && diagnostics.modelRuntimeHealthy && diagnostics.toolGatewayHealthy;
    const report = [
      `创作工作台 ${version}`,
      `Pi Agent Runtime / SDK ${diagnostics.sdkVersion ?? "未知"}`,
      `Agent Host: ${diagnostics.agentHostHealthy ? "正常" : "异常"}`,
      `模型目录: ${diagnostics.modelRuntimeHealthy ? `正常（${diagnostics.modelCount} 个模型）` : `异常（${diagnostics.modelRuntimeError ?? "未知错误"}）`}`,
      `AI 服务: ${configuredProviders}/${diagnostics.providerCount} 已连接`,
      `工具连接: ${diagnostics.toolGatewayHealthy ? "正常" : "异常"}`,
      `本地数据库: ${diagnostics.localDatabaseHealthy ? "正常" : "异常"}`,
      `活跃讨论: ${diagnostics.sessionHealth.active}（忙碌 ${diagnostics.sessionHealth.busy}）`,
      `总体状态: ${overallHealthy ? "正常" : "异常"}`,
      ...(diagnostics.error ? [`错误: ${diagnostics.error}`] : []),
    ].join("\n");
    try {
      await navigator.clipboard.writeText(report);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch (error) {
      onError(error);
    }
  };

  const diagnosticRows: Array<[string, boolean | null]> = [
    ["AI 服务", diagnostics?.modelRuntimeHealthy ?? null],
    ["Agent 服务", diagnostics ? diagnostics.agentHostHealthy && diagnostics.toolGatewayHealthy : null],
    ["本地数据库", diagnostics?.localDatabaseHealthy ?? null],
  ];

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
            {tab === "images" && <><div className="settings-intro"><span className="label">图片服务</span><h2>图片生成</h2><p>连接图片服务后，可在角色、场景、分镜和关键帧中直接使用。</p></div><ImageProviderSettingsPanel disabled={disabled} onError={onError} /></>}
            {tab === "data" && <><div className="settings-intro"><span className="label">本地优先</span><h2>数据与存储</h2><p>项目内容和素材保存在你选择的本地目录。</p></div><div className="settings-stack"><section className="settings-block"><div className="settings-heading"><div><h3>项目目录</h3><p>首页扫描和新建项目都会使用此目录。</p></div></div><label className="settings-field">当前目录<code className="settings-path">{rootPath}</code></label><div className="settings-actions"><button className="secondary" disabled={disabled} onClick={() => void chooseRoot()}>更改项目目录</button></div></section></div></>}
            {tab === "about" && <><div className="settings-intro"><span className="label">软件信息</span><h2>关于与诊断</h2><p>日常只显示服务状态，需要排查时再展开技术信息。</p></div><div className="settings-stack">
              <section className="settings-block">
                <div className="settings-heading"><div><h3>系统状态</h3><p>{diagnosing ? "正在检查工作台服务…" : diagnostics ? "工作台服务已检查。" : diagnosticError ? "检查未完成。" : "等待检查。"}</p></div><Activity size={18} /></div>
                <div className="diagnostic-summary" aria-live="polite">
                  {diagnosticRows.map(([label, healthy]) => {
                    const state = diagnosticError ? "error" : healthy === null ? "idle" : healthy ? "success" : "error";
                    return <div className={state} key={label}>{state === "success" ? <CheckCircle2 size={17} /> : state === "error" ? <XCircle size={17} /> : <CircleDashed size={17} />}<span>{label}</span><strong>{diagnosing ? "检查中" : diagnosticError ? "检查失败" : healthy === null ? "尚未检查" : healthy ? "正常" : "异常"}</strong></div>;
                  })}
                </div>
                {diagnosticError && <p className="settings-error" role="alert">{diagnosticError}</p>}
                <div className="settings-actions"><button className="secondary" disabled={diagnosing} onClick={() => void runDiagnostics(true)}><RefreshCw className={diagnosing ? "spin" : ""} size={15} />{diagnosing ? "正在诊断…" : "运行完整诊断"}</button></div>
                {showDiagnosticDetails && diagnostics && <div className="diagnostic-details">
                  <div><span>Pi Agent Runtime</span><strong>SDK {diagnostics.sdkVersion ?? "未知"}</strong></div>
                  <div><span>Agent Host</span><strong>{diagnostics.agentHostHealthy ? "运行中" : "异常"}</strong></div>
                  <div><span>模型目录</span><strong>{diagnostics.modelRuntimeHealthy ? `加载正常 · ${diagnostics.modelCount} 个模型` : "加载异常"}</strong></div>
                  <div><span>工具连接</span><strong>{diagnostics.toolGatewayHealthy ? "正常" : "异常"}</strong></div>
                  <div><span>活跃讨论</span><strong>{diagnostics.sessionHealth.active}</strong></div>
                  <button className="ghost" onClick={() => void copyDiagnostics()}><Clipboard size={14} />{copied ? "已复制" : "复制诊断信息"}</button>
                </div>}
              </section>
              <section className="settings-block about-grid"><div><span>应用</span><strong>创作工作台</strong></div><div><span>版本</span><strong>{version}</strong></div><div><span>数据策略</span><strong>项目内容本地保存</strong></div><div><span>凭据策略</span><strong>系统密钥保护</strong></div><div className="settings-actions"><button className="secondary" onClick={onRestartOnboarding}>重新运行首次使用引导</button></div></section>
            </div></>}
          </main>
        </div>
      </section>
    </div>
  );
}
