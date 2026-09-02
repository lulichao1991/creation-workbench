import { useRef, useState } from "react";
import { ArrowLeft, Bot, Clapperboard, FileText, Images, Layers3, PanelRightClose, PanelRightOpen, Plus } from "lucide-react";
import { ScriptStudio, StylePicker } from "./components/ScriptStudio";
import { creationTypes, emptyStudioDraft, importedScript, type ScriptDraftResult, type CreativeStyle, type CreationType } from "./features/scriptStudio";
import { normalizeCreativeSettings } from "./features/creativeSettings";
import "./App.css";

const previewSettingsKey = "workbench.studioPreview.creativeSettings";
function loadPreviewSettings() {
  try { return normalizeCreativeSettings(JSON.parse(localStorage.getItem(previewSettingsKey) ?? "null")); }
  catch { return normalizeCreativeSettings(null); }
}

export default function StudioPreview() {
  const [draft, setDraft] = useState(() => ({ ...emptyStudioDraft, ...loadPreviewSettings() }));
  const [result, setResult] = useState<ScriptDraftResult | null>(null);
  const [accepted, setAccepted] = useState<ScriptDraftResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [agentOpen, setAgentOpen] = useState(false);
  const [sceneIndex, setSceneIndex] = useState(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scenes = accepted?.episodes.flatMap((episode) => episode.scenes) ?? [];
  const changeSettings = (style: CreativeStyle | null, contentType: CreationType) => {
    const settings = normalizeCreativeSettings({ style, contentType });
    try { localStorage.setItem(previewSettingsKey, JSON.stringify(settings)); }
    catch { throw new Error("预览设定未能保存，请检查浏览器存储后重试。"); }
    setDraft((current) => ({ ...current, ...settings }));
  };
  const generate = () => {
    if (draft.mode === "import") { setResult(importedScript(draft.text, draft.fileName.replace(/\.(txt|md)$/i, "") || "导入原稿")); return; }
    setBusy(true);
    timer.current = setTimeout(() => {
      setBusy(false);
      if (draft.contentType && draft.contentType !== "auto" && draft.contentType !== "drama") {
        const demoText = {
          documentary: "拍摄计划：清晨，记录老街店主开门、整理工具的过程。\n\n同期声：开门声、脚步声与街巷环境声。\n\n拟采访问题：你每天开门后做的第一件事是什么？\n\n待补充：实际受访者、拍摄许可与真实采访素材。",
          advertising: "画面：中性背景中的产品轮廓，光线缓慢扫过表面。\n\n细节：展示产品材质与真实使用动作。\n\n字幕：[经确认的核心卖点]。\n\n结尾：[品牌名] 与 [行动引导]。\n\n待补充：产品信息、品牌规范及卖点依据。",
          explainer: "开场：以一个可观察的问题引入主题。\n\n画面：用分层图解展示关键过程。\n\n解说：[经核实的原理说明]，配合一个明确边界的类比。\n\n结尾：回到问题，归纳关键结论。\n\n待核实：主题事实、数据和引用来源。",
          music: "前奏：一个反复出现的空间意象，缓慢建立情绪。\n\n主段：主体动作与视觉节奏呼应音乐段落。\n\n高潮：色彩与空间变化达到峰值。\n\n尾奏：回到开场意象，留出停顿。\n\n待补充：实际音轨、段落时长与授权歌词。本演示未分析任何音频。",
        }[draft.contentType];
        setResult({ kind: "scriptDraft", title: `${creationTypes[draft.contentType]} · 演示草稿`, summary: "以下仅展示脚本结构，不是依据输入或风格生成的真实内容。", episodes: Array.from({ length: draft.episodes }, (_, index) => ({ title: `方案 ${index + 1}`, summary: "结构演示", scenes: [{ title: "表达段落", location: "待定", time: "待定", content: demoText }] })) });
        return;
      }
      setResult({ kind: "scriptDraft", title: "末班来信 · 演示草稿", summary: "雨夜，一位即将离职的邮递员收到一封写给十年前自己的信。（此为交互演示，不是 AI 生成结果。）", episodes: Array.from({ length: draft.episodes }, (_, index) => ({ title: ["无名来信", "旧城回声", "最后一班车", "雨停之前", "寄给明天"][index], summary: "演示分集梗概", scenes: [{ title: "末班车站", location: "旧城公交站", time: "夜", content: "雨水沿着站牌淌下。\n\n林川把最后一封信塞回邮包，却发现信封上写着自己的名字。\n\n他抬头。街对面，一个撑黑伞的人站在邮局门口。\n\n林川：这封信……谁送来的？\n\n公交车驶过。黑伞不见了。" }, { title: "无人邮局", location: "邮局", time: "夜", content: "林川推开玻璃门。\n\n柜台上的座钟，停在十年前的同一个时刻。\n\n电话忽然响起。\n\n听筒里的声音：别打开那封信。" }] })) });
    }, 1600);
  };
  return <div className="app-shell studio-preview-shell">
    <header className="app-header"><button className="back-button" aria-label="返回创作入口" onClick={() => { setAccepted(null); setResult(null); }}><ArrowLeft size={16} /></button><span className="header-brand-mark"><Clapperboard size={17} /></span><div className="project-title"><h1>剧本工作室</h1></div><span className="studio-preview-badge">交互预览 · 不调用 AI，不写入项目</span></header>
    <nav className="workspace-tabs" aria-label="工作区"><button className="active"><FileText size={15} />剧本</button><button disabled><Clapperboard size={15} />分镜</button><button disabled><Images size={15} />资产</button></nav>
    <div className={`studio-preview-layout ${agentOpen ? "agent-open" : ""}`}>
      <aside className="studio-preview-rail"><Layers3 size={18} /></aside>
      <main className="center-panel">{accepted ? <div className="script-editor-shell"><div className="script-editor-toolbar"><StylePicker label="创作设定" value={draft.style} contentType={draft.contentType} onChange={changeSettings} /></div><div className="workspace-content split-workspace"><div className="sub-list"><div className="panel-heading"><strong>{scenes.length} 场</strong><button className="icon-button" aria-label="新增演示场" onClick={() => setAccepted({ ...accepted, episodes: [{ ...accepted.episodes[0], scenes: [...scenes, { title: `场 ${scenes.length + 1}`, location: "", time: "", content: "" }] }] })}><Plus size={15} /></button></div>{scenes.map((scene, index) => <button key={index} className={`sub-list-row ${index === sceneIndex ? "selected" : ""}`} onClick={() => setSceneIndex(index)}><strong>{scene.title}</strong><small>{scene.location}</small></button>)}</div><div className="editor-area"><div className="editor-card"><h2>{scenes[sceneIndex]?.title}</h2><textarea className="studio-preview-editor" aria-label="演示剧本文本" value={scenes[sceneIndex]?.content ?? ""} onChange={(event) => { const updated = scenes.map((scene, index) => index === sceneIndex ? { ...scene, content: event.target.value } : scene); setAccepted({ ...accepted, episodes: [{ ...accepted.episodes[0], scenes: updated }] }); }} /></div></div></div></div> : <ScriptStudio draft={draft} onChange={setDraft} onSettingsChange={changeSettings} result={result} busy={busy} error="" onGenerate={generate} onCancel={() => { if (timer.current) clearTimeout(timer.current); setBusy(false); }} onAccept={() => { setAccepted(result); setSceneIndex(0); }} onBack={() => setResult(null)} onManual={() => { setAccepted(importedScript("开始写下第一场……", "场 01")); setSceneIndex(0); }} />}</main>
      <aside className="right-panel"><div className="inspector-header">{agentOpen && <span><Bot size={16} />创作助手</span>}<button className="panel-toggle" aria-label={agentOpen ? "收起 Agent" : "展开 Agent"} onClick={() => setAgentOpen(!agentOpen)}>{agentOpen ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}</button></div>{agentOpen ? <div className="studio-preview-agent"><Bot size={24} /><p>讨论、补充，或修改当前剧本。</p><small>这里是独立的 Agent 区域。预览版未连接对话服务。</small><textarea disabled placeholder="对话服务在桌面工作台中使用" /></div> : <span className="studio-preview-bot"><Bot size={17} /></span>}</aside>
    </div>
  </div>;
}
