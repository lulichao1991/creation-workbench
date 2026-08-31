import { open } from "@tauri-apps/plugin-dialog";
import { ArrowUpRight, Copy, Film, FolderCog, FolderOpen, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import type { ProjectDescriptor } from "../types";

interface Props {
  rootPath: string;
  projects: ProjectDescriptor[];
  busy: boolean;
  onRootChange: (path: string) => Promise<void>;
  onCreate: (name: string, structureType: string) => Promise<void>;
  onOpen: (project: ProjectDescriptor | string) => Promise<void>;
  onCopy: (project: ProjectDescriptor, name: string) => Promise<void>;
  onDelete: (project: ProjectDescriptor) => Promise<void>;
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
}: Props) {
  const [name, setName] = useState("");
  const [structureType, setStructureType] = useState("single-season");

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
          <div>
            <strong>创作工作台</strong>
            <small>LOCAL CREATION SYSTEM</small>
          </div>
        </div>
        <button className="secondary" onClick={chooseProject} disabled={busy}>
          <FolderOpen size={16} />打开其他项目
        </button>
      </header>

      <section className="home-hero">
        <p className="eyebrow">STORY PRODUCTION, LOCAL FIRST</p>
        <h1>从故事构想到<br /><span>每一个镜头。</span></h1>
        <p className="subtitle">在一个本地工作区里管理剧本、分镜、资产、关键帧与生成任务。创作事实只保存在你的电脑上。</p>
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
        <div>
          <p className="eyebrow">NEW PROJECT</p>
          <h2>开始一个作品</h2>
        </div>
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="项目名称，例如：智斗游戏"
          onKeyDown={(event) => {
            if (event.key === "Enter" && name.trim()) {
              void onCreate(name, structureType).then(() => setName(""));
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
          onClick={() => void onCreate(name, structureType).then(() => setName(""))}
        >
          <Plus size={17} />创建项目
        </button>
      </section>

      <section className="project-section">
        <div className="section-heading">
          <div>
            <p className="eyebrow">RECENT PROJECTS</p>
            <h2>本地项目</h2>
          </div>
          <span className="count-badge">{projects.length}</span>
        </div>
        {projects.length === 0 ? (
          <div className="empty-state">
            <strong>还没有项目</strong>
            <span>创建第一个项目后，它会出现在这里。</span>
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
                    onClick={() => {
                      const copyName = window.prompt("项目副本名称", `${project.name} 副本`);
                      if (copyName?.trim()) void onCopy(project, copyName);
                    }}
                  >
                    <Copy size={14} />复制
                  </button>
                  <button
                    className="danger-text"
                    onClick={() => {
                      if (window.confirm(`确认删除“${project.name}”？项目目录及其中数据将被永久删除。`)) {
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
