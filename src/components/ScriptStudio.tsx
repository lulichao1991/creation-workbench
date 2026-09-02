import { ArrowRight, Check, ChevronDown, Clapperboard, FileUp, Film, GraduationCap, LoaderCircle, Megaphone, MonitorPlay, Music2, Palette, Search, Smartphone, Sparkles, Square, Tags, Video, X } from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { contentPresets, creationTypes, normalizeCreativeStyle, styleLibrarySections, stylePresets, styleSelections, toggleStylePreset, withStyleDimension, type CreationType, type CreativeStyle, type ScriptDraftResult, type StudioDraft, type StylePreset, type StyleSection } from "../features/scriptStudio";
import { shuoVisualCategories } from "../features/shuoVisualStyles";
import { toUserErrorMessage } from "../domain/userError";
import "./ScriptStudio.css";

const sectionIcons = { contentType: Clapperboard, genre: Tags, visual: Palette, platform: MonitorPlay };
const contentIcons = { drama: Film, documentary: Video, advertising: Megaphone, explainer: GraduationCap, music: Music2 };
const thumbnailAssets = import.meta.glob<string>("../assets/shuo-story-styles/*.webp", { eager: true, import: "default", query: "?url" });

type SettingsChange = (style: CreativeStyle | null, contentType: CreationType) => void | Promise<void>;

export function StylePicker({ value, contentType = "auto", onChange, label = "创作设定", disabled = false }: { value: CreativeStyle | null; contentType: CreationType; onChange: SettingsChange; label?: string; disabled?: boolean }) {
  const [open, setOpen] = useState(false);
  const labels = [...(contentType === "auto" ? [] : [creationTypes[contentType]]), ...styleSelections(value).map((item) => item.label)];
  return <>
    <button className="studio-style-trigger" disabled={disabled} onClick={() => setOpen(true)} aria-haspopup="dialog" aria-label={label} title={labels.join(" · ") || label}>
      <Palette size={16} /><span>{labels.slice(0, 2).join(" · ") || label}</span>{labels.length > 2 && <small>+{labels.length - 2}</small>}<ChevronDown size={13} />
    </button>
    {open && <StyleLibrary value={value} contentType={contentType} onClose={() => setOpen(false)} onApply={onChange} />}
  </>;
}

function StyleLibrary({ value, contentType, onClose, onApply }: { value: CreativeStyle | null; contentType: CreationType; onClose: () => void; onApply: SettingsChange }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState("");
  const [type, setType] = useState(contentType);
  const [section, setSection] = useState<StyleSection>("contentType");
  const [visualCategory, setVisualCategory] = useState<string>("all");
  const [selected, setSelected] = useState(() => normalizeCreativeStyle(value));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const saveLock = useRef(false);
  const close = () => { if (!saveLock.current) onClose(); };
  const apply = async () => {
    if (saveLock.current) return;
    saveLock.current = true;
    setSaving(true);
    setError("");
    try { await onApply(selected, type); onClose(); }
    catch (reason) { setError(toUserErrorMessage(reason)); }
    finally { saveLock.current = false; setSaving(false); }
  };
  useEffect(() => { dialogRef.current?.showModal(); }, []);
  const choices = styleSelections(selected);
  const currentChoices = choices.filter((item) => item.field === section);
  const currentCount = section === "contentType" ? Number(type !== "auto") : currentChoices.length;
  const total = choices.length + Number(type !== "auto");
  const presets = section === "contentType" ? contentPresets : stylePresets[section];
  const filtered = presets.filter((preset) => (section !== "visual" || visualCategory === "all" || preset.category === visualCategory) && `${preset.name} ${preset.description}`.toLowerCase().includes(query.toLowerCase().trim()));
  const clearSection = () => section === "contentType" ? setType("auto") : setSelected(withStyleDimension(selected, section, ""));
  const SectionIcon = sectionIcons[section];
  return <dialog ref={dialogRef} className="style-library" aria-labelledby="style-library-title" aria-busy={saving} onCancel={(event) => { event.preventDefault(); close(); }} onClick={(event) => { if (event.target === event.currentTarget) { const rect = event.currentTarget.getBoundingClientRect(); if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) close(); } }}>
    <header className="style-library-header"><h2 id="style-library-title">创作风格库</h2><button className="icon-button" aria-label="关闭风格库" disabled={saving} onClick={close}><X size={18} /></button></header>
    <div className="style-library-body" inert={saving}>
      <nav className="style-section-nav" aria-label="风格分类">
        {(Object.entries(styleLibrarySections) as Array<[StyleSection, string]>).map(([key, name]) => {
          const Icon = sectionIcons[key];
          const count = key === "contentType" ? Number(type !== "auto") : choices.filter((item) => item.field === key).length;
          return <button key={key} className={section === key ? "active" : ""} aria-label={name} aria-pressed={section === key} onClick={() => { setSection(key); setQuery(""); }}><Icon size={17} /><span>{name}</span>{count > 0 && <small>{count}</small>}</button>;
        })}
      </nav>
      <section className="style-gallery" aria-label={`${styleLibrarySections[section]}预设`}>
        <div className="style-gallery-toolbar">
          {section === "visual" ? <nav className="style-visual-categories" aria-label="视觉风格分类">{shuoVisualCategories.map((category) => <button key={category.id} className={visualCategory === category.id ? "active" : ""} aria-pressed={visualCategory === category.id} onClick={() => setVisualCategory(category.id)}>{category.label}</button>)}</nav> : <span>{section === "genre" ? "可多选" : "单选"}</span>}
          <label className="style-search"><Search size={15} /><input aria-label="搜索当前分类" placeholder={`搜索${styleLibrarySections[section]}`} value={query} onChange={(event) => setQuery(event.target.value)} /></label>
        </div>
        <div key={`${section}:${visualCategory}`} className={`style-card-grid style-grid-${section}`}>
          {!query.trim() && (section !== "visual" || visualCategory === "all") && <button className={`style-card style-follow ${!currentCount ? "selected" : ""}`} aria-label="跟随内容" aria-pressed={!currentCount} onClick={clearSection}><div className="style-follow-art"><Sparkles size={30} /></div><strong>跟随内容{!currentCount && <Check size={15} />}</strong>{section !== "visual" && <small>不限定{styleLibrarySections[section]}</small>}</button>}
          {filtered.map((preset) => {
            const active = section === "contentType" ? type === preset.id : currentChoices.some((item) => item.prompt === preset.prompt);
            return <button key={preset.id} className={`style-card ${active ? "selected" : ""}`} aria-label={preset.name} aria-pressed={active} title={preset.description} style={{ "--cover-color": preset.color } as CSSProperties} onClick={() => section === "contentType" ? setType(type === preset.id ? "auto" : preset.id as CreationType) : setSelected(toggleStylePreset(selected, section, preset.prompt))}>
              <StyleArtwork section={section} preset={preset} /><strong>{preset.name}{active && <Check size={15} />}</strong>{preset.description && <small>{preset.description}</small>}
            </button>;
          })}
          {section !== "visual" && currentChoices.filter((item) => !presets.some((preset) => preset.prompt === item.prompt)).map((item) => <button key={item.prompt} className="style-card style-saved selected" aria-pressed="true" title={item.prompt} onClick={() => section !== "contentType" && setSelected(toggleStylePreset(selected, section, item.prompt))}><SectionIcon size={24} /><strong>{item.label}<Check size={15} /></strong><small>{item.prompt}</small></button>)}
          {!filtered.length && <p className="style-no-results">没有匹配的预设</p>}
        </div>
      </section>
    </div>
    {error && <p className="style-save-error" role="alert">未能确认保存，选择已保留。{error}</p>}
    <footer className="style-library-footer">
      <div className="style-selected-chips" aria-label="已选风格" aria-live="polite" inert={saving}>
        {type !== "auto" && <button title="移除内容形态" aria-label={`移除${creationTypes[type]}`} onClick={() => setType("auto")}><Clapperboard size={13} />{creationTypes[type]}<X size={12} /></button>}
        {choices.map((item) => { const Icon = sectionIcons[item.field]; return <button key={`${item.field}:${item.prompt}`} title={item.prompt} aria-label={`移除${item.label}`} onClick={() => setSelected(toggleStylePreset(selected, item.field, item.prompt))}><Icon size={13} />{item.label}<X size={12} /></button>; })}
        {!total && <span>未限定，跟随创作内容</span>}
      </div>
      <div className="style-library-actions">{total > 0 && <button className="style-clear" disabled={saving} onClick={() => { setSelected(null); setType("auto"); }}>清空</button>}<button className="ghost" disabled={saving} onClick={close}>取消</button><button className="primary" disabled={saving} onClick={() => void apply()}>{saving ? <>保存中…<LoaderCircle className="studio-spin" size={15} /></> : <>应用选择<ArrowRight size={15} /></>}</button></div>
    </footer>
  </dialog>;
}

function StyleArtwork({ section, preset }: { section: StyleSection; preset: StylePreset }) {
  const thumbnail = preset.thumbnail && thumbnailAssets[`../assets/shuo-story-styles/${preset.thumbnail}.webp`];
  if (thumbnail) return <img className="style-thumbnail" src={thumbnail} alt="" width={320} height={192} loading="lazy" decoding="async" draggable={false} />;
  if (section === "genre") return <div className="style-genre-mark" aria-hidden="true"><span /><i /><b /></div>;
  if (section === "contentType") {
    const Icon = contentIcons[preset.id as keyof typeof contentIcons];
    return <div className="style-content-art" aria-hidden="true"><span /><Icon size={36} strokeWidth={1.2} /><i /><b /></div>;
  }
  if (section === "platform") {
    const Icon = ["vertical", "community", "channels"].includes(preset.id) ? Smartphone : MonitorPlay;
    return <div className="style-platform-art" aria-hidden="true"><Icon size={64} strokeWidth={1} /><i /><b /></div>;
  }
  return <div className="style-image-placeholder" aria-hidden="true"><Palette size={30} /><small>暂无匹配缩略图</small></div>;
}

interface Props {
  draft: StudioDraft;
  onChange: (draft: StudioDraft) => void;
  onSettingsChange?: SettingsChange;
  busy: boolean;
  result: ScriptDraftResult | null;
  error: string;
  onGenerate: () => void;
  onCancel: () => void;
  onAccept: () => void;
  onBack: () => void;
  onManual: () => void;
  accepting?: boolean;
}

export function ScriptStudio({ draft, onChange, onSettingsChange, busy, result, error, onGenerate, onCancel, onAccept, onBack, onManual, accepting = false }: Props) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [fileError, setFileError] = useState("");
  const [reading, setReading] = useState(false);
  const [episodeIndex, setEpisodeIndex] = useState(0);
  const locked = busy || accepting || reading;
  useEffect(() => { setEpisodeIndex(0); }, [result]);
  const readFile = async (file?: File) => {
    if (!file || locked) return;
    setReading(true);
    setFileError("");
    try {
      if (!/\.(txt|md)$/i.test(file.name) || file.size > 2_000_000) throw new Error("请选择 2 MB 以内的 TXT 或 Markdown 文本文件。");
      const text = await file.text();
      if (text.length > 100_000 || !text.trim()) throw new Error("原稿需有正文，且不超过 10 万字。");
      onChange({ ...draft, text, fileName: file.name });
    } catch (reason) { setFileError(reason instanceof Error ? reason.message : "读取文件失败"); } finally { setReading(false); if (fileRef.current) fileRef.current.value = ""; }
  };
  if (result) {
    const episode = result.episodes[episodeIndex] ?? result.episodes[0];
    return <section className="studio-result">
      <header><div><span className="studio-result-label">待确认草稿</span><h2>{result.title}</h2></div><button className="ghost" disabled={accepting} onClick={onBack}>返回调整</button><button className="primary" disabled={accepting} onClick={onAccept}>{accepting ? <LoaderCircle className="studio-spin" size={15} /> : <Check size={15} />}采用剧本</button></header>
      {result.summary && <p className="studio-result-summary">{result.summary}</p>}
      {result.episodes.length > 1 && <><p className="studio-save-note">采用后将新增 {result.episodes.length} 份脚本，保留现有内容。</p><div className="studio-episode-tabs">{result.episodes.map((item, index) => <button className={episodeIndex === index ? "active" : ""} key={index} onClick={() => setEpisodeIndex(index)}>{index + 1}. {item.title}</button>)}</div></>}
      {error && <p className="studio-error" role="alert">{error}</p>}
      <div className="studio-draft-scenes">{episode.scenes.map((scene, index) => <article key={index}><header><span>{String(index + 1).padStart(2, "0")}</span><h3>{scene.title}</h3><small>{[scene.location, scene.time].filter(Boolean).join(" · ")}</small></header><div>{scene.content}</div></article>)}</div>
    </section>;
  }
  return <div className="script-studio-stage">
    <div className="studio-intro"><span className="studio-intro-mark"><Sparkles size={23} /></span><h2>让故事从这里开始</h2></div>
    <section className="script-studio" aria-label="剧本创作">
      <div className="studio-mode-tabs" aria-label="创作方式">{([["original", "原创剧本"], ["import", "导入剧本"], ["rewrite", "参考改写"]] as const).map(([mode, label]) => <button key={mode} className={draft.mode === mode ? "active" : ""} aria-pressed={draft.mode === mode} disabled={locked} onClick={() => { onChange({ ...draft, mode }); setFileError(""); }}>{mode === "import" ? <FileUp size={16} /> : <Sparkles size={16} />}{label}</button>)}</div>
      <div className="studio-composer" onDragOver={(event) => event.preventDefault()} onDrop={(event) => { event.preventDefault(); void readFile(event.dataTransfer.files[0]); }}>
        <textarea aria-label={draft.mode === "original" ? "创作设定" : "剧本原稿"} disabled={locked} value={draft.text} maxLength={100_000} onChange={(event) => onChange({ ...draft, text: event.target.value })} placeholder={draft.mode === "original" ? "一个故事、一段真实记录，或一个想让人记住的创意……" : draft.mode === "import" ? "粘贴原稿，或拖入 TXT / Markdown 文件……" : "粘贴参考脚本，或拖入 TXT / Markdown 文件……"} />
        {draft.mode === "rewrite" && <input className="studio-rewrite-direction" aria-label="改写方向" disabled={locked} placeholder="想保留什么，又想改变什么？" maxLength={2000} value={draft.direction} onChange={(event) => onChange({ ...draft, direction: event.target.value })} />}
        <div className="studio-attachment-row"><button className="studio-file-button" disabled={locked} onClick={() => fileRef.current?.click()}><FileUp size={16} />{reading ? "读取中…" : draft.fileName || "上传原稿"}</button><span>{draft.text.length.toLocaleString()} / 100,000</span></div>
        <input ref={fileRef} type="file" accept=".txt,.md,text/plain,text/markdown" hidden onChange={(event) => void readFile(event.target.files?.[0])} />
      </div>
      <footer className="studio-toolbar">
        {draft.mode === "import" ? <span className="studio-import-note">保留原稿，不进行 AI 改写</span> : <div className="studio-options"><StylePicker value={draft.style} contentType={draft.contentType} disabled={locked} onChange={onSettingsChange ?? ((style, contentType) => onChange({ ...draft, style, contentType }))} /><select aria-label="创作数量" disabled={locked} value={draft.episodes} onChange={(event) => onChange({ ...draft, episodes: Number(event.target.value) })}>{[1, 3, 5].map((count) => <option key={count} value={count}>{count} {draft.contentType === "drama" ? "集" : "条"}</option>)}</select><select aria-label="脚本表达" disabled={locked} value={draft.scriptMode} onChange={(event) => onChange({ ...draft, scriptMode: event.target.value as StudioDraft["scriptMode"] })}><option value="drama">场景模式</option><option value="narration">解说模式</option></select></div>}
        {busy ? <button className="ghost" onClick={onCancel}><Square size={13} />停止创作</button> : <button className="primary studio-generate" disabled={!draft.text.trim() || locked} onClick={onGenerate}>{draft.mode === "import" ? "预览原稿" : "开始创作"}<ArrowRight size={17} /></button>}
      </footer>
      {busy && <div className="studio-progress" role="status"><LoaderCircle className="studio-spin" size={16} />正在创作剧本，完成后可预览确认<span /></div>}
    </section>
    {(error || fileError) && <p className="studio-error" role="alert">{fileError || error}</p>}
    <button className="studio-manual" disabled={locked} onClick={onManual}>直接手动写作<ArrowRight size={13} /></button>
  </div>;
}
