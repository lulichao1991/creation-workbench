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
        {step === 0 && <div className="onboarding-step"><FolderCog size={34} /><span className="label">第一步 · 项目目录</span><h1 id="onboarding-title">作品保存在哪里？</h1><p>每个项目都有独立数据库和素材目录。以后可以在全局设置中修改默认目录。</p><code>{rootPath}</code><button className="secondary" disabled={disabled} onClick={() => void chooseRoot()}>选择其他目录</button></div>}
        {step === 1 && <div className="onboarding-step"><PenLine size={34} /><span className="label">第二步 · 创作方式</span><h1 id="onboarding-title">选择你的起步方式</h1><p>两种方式使用同一套项目数据，可以随时切换。</p><div className="onboarding-options"><button className={mode === "manual" ? "selected" : ""} onClick={() => setMode("manual")}><PenLine size={20} /><strong>手动创作</strong><small>不配置任何 AI 也能完成全部编辑与导出。</small></button><button className={mode === "ai" ? "selected" : ""} onClick={() => setMode("ai")}><Bot size={20} /><strong>AI 辅助</strong><small>使用 Agent 讨论、分析并提出可确认的修改建议。</small></button></div></div>}
        {step === 2 && <div className="onboarding-step"><Bot size={34} /><span className="label">第三步 · 可选效率服务</span><h1 id="onboarding-title">现在配置，或稍后再说</h1><p>Agent 和图片生成都不是进入软件的前置条件，但配置后可以显著缩短创作链路。</p><div className="onboarding-services"><button onClick={onOpenSettings}><Bot size={19} /><span><strong>Agent 与模型</strong><small>配置账号、默认模型并检测运行环境</small></span><ChevronRight size={16} /></button><button onClick={onOpenSettings}><Image size={19} /><span><strong>图片生成</strong><small>为资产、分镜和关键帧直接生成图片</small></span><ChevronRight size={16} /></button></div></div>}
        {step === 3 && <div className="onboarding-step onboarding-complete"><Check size={38} /><span className="label">设置完成</span><h1 id="onboarding-title">可以开始创作了</h1><p>先在首页创建项目。每个空工作区都会告诉你当前可以做什么以及推荐的下一步。</p></div>}
        <footer className="onboarding-actions">{step > 0 && step < 3 ? <button className="ghost" onClick={() => setStep((value) => value - 1)}><ChevronLeft size={15} />上一步</button> : <span />}{step < 3 ? <button className="primary" onClick={next}>继续<ChevronRight size={15} /></button> : <button className="primary" onClick={onComplete}>进入工作台</button>}</footer>
      </section>
    </div>
  );
}
