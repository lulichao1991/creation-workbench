import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUpRight, Copy, Film, FolderCog, FolderOpen, Plus, Settings2, Trash2 } from "lucide-react";
import { useState } from "react";
import type { ProjectDescriptor } from "../types";
import { useAppDialog } from "./AppDialog";

interface Props {
  rootPath: string;
  projects: ProjectDescriptor[];
  busy: boolean;
  onRootChange: (path: string) => Promise<void>;
  onCreate: (name: string, structureType: string) => Promise<boolean>;
  onOpen: (project: ProjectDescriptor | string) => Promise<void>;
  onCopy: (project: ProjectDescriptor, name: string) => Promise<void>;
  onDelete: (project: ProjectDescriptor) => Promise<void>;
  onOpenSettings: () => void;
}

const structureOptions = [
  ["short", "短片"],
  ["single-season", "单季系列"],
  ["multi-season", "多季系列"],
  ["feature", "电影 / 长片"],
  ["custom", "自定义结构"],
];

export function ProjectHome({
  rootPath,
  projects,
  busy,
  onRootChange,
  onCreate,
  onOpen,
  onCopy,
  onDelete,
  onOpenSettings,
}: Props) {
  const [name, setName] = useState("");
  const [structureType, setStructureType] = useState("single-season");
  const dialog = useAppDialog();

  const chooseRoot = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") await onRootChange(selected);
  };

  const chooseProject = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") await onOpen(selected);
  };

  return (
    <main className="home-shell">
      <header className="home-header">
        <div className="brand-lockup">
          <span className="brand-mark"><Film size={21} strokeWidth={1.8} /></span>
          <strong>创作工作台</strong>
        </div>
        <div className="heading-actions"><button className="ghost" onClick={onOpenSettings}><Settings2 size={16} />全局设置</button><button className="secondary" onClick={chooseProject} disabled={busy}><FolderOpen size={16} />打开其他项目</button></div>
      </header>

      <section className="home-hero">
        <h1>从故事构想到<br /><span>每一个镜头。</span></h1>
      </section>

      <section className="root-path-bar">
        <div>
          <FolderCog size={16} />
          <span className="label">项目根目录</span>
          <code>{rootPath}</code>
        </div>
        <button className="ghost" onClick={chooseRoot} disabled={busy}>
          更改目录
        </button>
      </section>

      <section className="create-panel">
        <h2>新建项目</h2>
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="项目名称，例如：智斗游戏"
          onKeyDown={(event) => {
            if (event.key === "Enter" && name.trim()) {
              void onCreate(name, structureType).then((created) => created && setName(""));
            }
          }}
        />
        <select value={structureType} onChange={(event) => setStructureType(event.target.value)}>
          {structureOptions.map(([value, label]) => (
            <option value={value} key={value}>
              {label}
            </option>
          ))}
        </select>
        <button
          className="primary"
          disabled={busy || !name.trim()}
          title={busy ? "正在处理上一项操作" : name.trim() ? "创建并打开项目" : "请先输入项目名称"}
          onClick={() => void onCreate(name, structureType).then((created) => created && setName(""))}
        >
          <Plus size={17} />创建项目
        </button>
      </section>

      <section className="project-section">
        <div className="section-heading">
          <h2>最近项目</h2>
          <span className="count-badge">{projects.length}</span>
        </div>
        {projects.length === 0 ? (
          <div className="empty-state">
            <strong>还没有项目</strong>
          </div>
        ) : (
          <div className="project-grid">
            {projects.map((project) => (
              <article className="project-card" key={project.path}>
                <button className="project-open" onClick={() => void onOpen(project)} disabled={busy}>
                  <span className="project-mark">{project.name.slice(0, 1)}</span>
                  <span>
                    <strong>{project.name}</strong>
                    <small>修订 {project.revision} · {formatDate(project.updatedAt)}</small>
                  </span>
                  <ArrowUpRight className="project-arrow" size={18} />
                </button>
                <div className="card-actions">
                  <button
                    className="ghost"
                    onClick={async () => {
                      const copyName = await dialog.prompt("复制项目", { label: "项目副本名称", defaultValue: `${project.name} 副本`, confirmLabel: "创建副本" });
                      if (copyName) void onCopy(project, copyName);
                    }}
                  >
                    <Copy size={14} />复制
                  </button>
                  <button
                    className="danger-text"
                    onClick={async () => {
                      if (await dialog.confirm("项目目录及其中全部数据将被永久删除。此操作无法撤销。", { title: `删除“${project.name}”？`, confirmLabel: "永久删除", danger: true })) {
                        void onDelete(project);
                      }
                    }}
                  >
                    <Trash2 size={14} />删除
                  </button>
                </div>
              </article>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}

function formatDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat("zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
}
