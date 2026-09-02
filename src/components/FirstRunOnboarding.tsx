import { open } from "@tauri-apps/plugin-dialog";
import { Bot, Check, ChevronLeft, ChevronRight, FolderCog, Image, PenLine } from "lucide-react";
import { useState } from "react";

interface Props {
  rootPath: string;
  disabled: boolean;
  onRootChange: (path: string) => Promise<void>;
  onOpenSettings: () => void;
  onComplete: () => void;
  onError: (error: unknown) => void;
}

export function FirstRunOnboarding({ rootPath, disabled, onRootChange, onOpenSettings, onComplete, onError }: Props) {
  const [step, setStep] = useState(0);
  const [mode, setMode] = useState<"manual" | "ai">(() => localStorage.getItem("workbench.creationMode") === "ai" ? "ai" : "manual");

  const chooseRoot = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    try { await onRootChange(selected); } catch (error) { onError(error); }
  };

  const next = () => {
    if (step === 1) localStorage.setItem("workbench.creationMode", mode);
    setStep((value) => Math.min(3, value + 1));
  };

  return (
    <div className="onboarding-backdrop">
      <section className="onboarding-card" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
        <div className="onboarding-progress" aria-label={`首次设置，第 ${step + 1} 步，共 4 步`}>{[0, 1, 2, 3].map((index) => <span className={index <= step ? "active" : ""} key={index} />)}</div>
        {step === 0 && <div className="onboarding-step"><FolderCog size={34} /><h1 id="onboarding-title">作品保存在哪里？</h1><code>{rootPath}</code><button className="secondary" disabled={disabled} onClick={() => void chooseRoot()}>选择其他目录</button></div>}
        {step === 1 && <div className="onboarding-step"><PenLine size={34} /><h1 id="onboarding-title">选择创作方式</h1><div className="onboarding-options"><button className={mode === "manual" ? "selected" : ""} onClick={() => setMode("manual")}><PenLine size={20} /><strong>手动创作</strong><small>无需配置 AI</small></button><button className={mode === "ai" ? "selected" : ""} onClick={() => setMode("ai")}><Bot size={20} /><strong>AI 辅助</strong><small>使用 Agent 共创</small></button></div></div>}
        {step === 2 && <div className="onboarding-step"><Bot size={34} /><h1 id="onboarding-title">配置服务</h1><div className="onboarding-services"><button onClick={onOpenSettings}><Bot size={19} /><span><strong>AI 模型</strong><small>连接服务并选择模型</small></span><ChevronRight size={16} /></button><button onClick={onOpenSettings}><Image size={19} /><span><strong>图片生成</strong><small>连接图片服务</small></span><ChevronRight size={16} /></button></div></div>}
        {step === 3 && <div className="onboarding-step onboarding-complete"><Check size={38} /><h1 id="onboarding-title">可以开始创作了</h1></div>}
        <footer className="onboarding-actions">{step > 0 && step < 3 ? <button className="ghost" onClick={() => setStep((value) => value - 1)}><ChevronLeft size={15} />上一步</button> : <span />}{step < 3 ? <button className="primary" onClick={next}>继续<ChevronRight size={15} /></button> : <button className="primary" onClick={onComplete}>进入工作台</button>}</footer>
      </section>
    </div>
  );
}
