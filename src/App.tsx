import { useEffect, useState } from "react";
import { api } from "./api";
import "./App.css";
import { ProjectHome } from "./components/ProjectHome";
import { Workbench } from "./components/Workbench";
import { useSelectionStore } from "./stores/selectionStore";
import type {
  MutationRequest,
  MutationResponse,
  ProjectDescriptor,
  ProjectState,
} from "./types";

function App() {
  const [rootPath, setRootPath] = useState("");
  const [projects, setProjects] = useState<ProjectDescriptor[]>([]);
  const [activeProject, setActiveProject] = useState<ProjectDescriptor | null>(null);
  const [state, setState] = useState<ProjectState | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const clearSelection = useSelectionStore((selection) => selection.clear);

  useEffect(() => {
    void initialize();
  }, []);

  const initialize = async () => {
    setBusy(true);
    try {
      const info = await api.getDefaultWorkspace();
      const savedRoot = localStorage.getItem("workbench.projectRoot") || info.defaultPath;
      setRootPath(savedRoot);
      setProjects(await api.listProjects(savedRoot));
    } catch (reason) {
      handleError(reason);
    } finally {
      setBusy(false);
    }
  };

  const refreshProjects = async (path = rootPath) => {
    if (!path) return;
    setProjects(await api.listProjects(path));
  };

  const refreshState = async (project = activeProject) => {
    if (!project) return;
    const next = await api.loadProjectState(project.path);
    setState(next);
    const projectRow = next.projects[0];
    if (projectRow) {
      setActiveProject((current) =>
        current
          ? {
              ...current,
              name: projectRow.name,
              description: projectRow.description,
              revision: projectRow.revision,
              updatedAt: projectRow.updated_at,
            }
          : current,
      );
    }
  };

  const changeRoot = async (path: string) => {
    setBusy(true);
    try {
      localStorage.setItem("workbench.projectRoot", path);
      setRootPath(path);
      setProjects(await api.listProjects(path));
    } catch (reason) {
      handleError(reason);
    } finally {
      setBusy(false);
    }
  };

  const createProject = async (name: string, structureType: string) => {
    setBusy(true);
    try {
      const project = await api.createProject(rootPath, name, structureType);
      await seedStructure(project, structureType);
      await refreshProjects();
      await openProject(project);
    } catch (reason) {
      handleError(reason);
    } finally {
      setBusy(false);
    }
  };

  const openProject = async (projectOrPath: ProjectDescriptor | string) => {
    setBusy(true);
    try {
      const project =
        typeof projectOrPath === "string"
          ? await api.openProject(projectOrPath)
          : projectOrPath;
      const next = await api.loadProjectState(project.path);
      setActiveProject(project);
      setState(next);
      clearSelection();
    } catch (reason) {
      handleError(reason);
    } finally {
      setBusy(false);
    }
  };

  const copyProject = async (project: ProjectDescriptor, name: string) => {
    setBusy(true);
    try {
      await api.copyProject(project.path, name);
      await refreshProjects();
    } catch (reason) {
      handleError(reason);
    } finally {
      setBusy(false);
    }
  };

  const deleteProject = async (project: ProjectDescriptor) => {
    setBusy(true);
    try {
      await api.deleteProject(project.path);
      await refreshProjects();
    } catch (reason) {
      handleError(reason);
    } finally {
      setBusy(false);
    }
  };

  const mutate = async (request: MutationRequest): Promise<MutationResponse> => {
    if (!activeProject) throw new Error("没有打开的项目");
    setBusy(true);
    try {
      const result = await api.mutate(activeProject.path, request);
      await refreshState(activeProject);
      return result;
    } catch (reason) {
      handleError(reason);
      throw reason;
    } finally {
      setBusy(false);
    }
  };

  const handleError = (reason: unknown) => {
    const message = reason instanceof Error ? reason.message : String(reason);
    setError(message);
    window.setTimeout(() => setError(null), 7000);
  };

  return (
    <>
      {activeProject && state ? (
        <Workbench
          project={activeProject}
          state={state}
          busy={busy}
          onBack={() => {
            setActiveProject(null);
            setState(null);
            clearSelection();
            void refreshProjects();
          }}
          onMutate={mutate}
          onRefresh={() => refreshState(activeProject)}
          onError={handleError}
        />
      ) : (
        <ProjectHome
          rootPath={rootPath}
          projects={projects}
          busy={busy}
          onRootChange={changeRoot}
          onCreate={createProject}
          onOpen={openProject}
          onCopy={copyProject}
          onDelete={deleteProject}
        />
      )}
      {busy && <div className="busy-bar" />}
      {error && (
        <div className="toast error-toast">
          <strong>操作未完成</strong>
          <span>{error}</span>
          <button onClick={() => setError(null)}>×</button>
        </div>
      )}
    </>
  );
}

async function seedStructure(project: ProjectDescriptor, structureType: string) {
  if (structureType === "custom") return;
  const type = structureType === "short" || structureType === "feature" ? "short" : "season";
  const name = structureType === "short" || structureType === "feature" ? "正片" : "第一季";
  const root = await api.mutate(project.path, {
    action: "create",
    entityType: "contentUnit",
    values: {
      project_id: project.id,
      parent_id: null,
      type,
      name,
      sort_order: 0,
    },
    changeSetName: "初始化作品结构",
  });
  if (structureType === "feature") {
    for (const [index, actName] of ["第一幕", "第二幕", "第三幕"].entries()) {
      await api.mutate(project.path, {
        action: "create",
        entityType: "contentUnit",
        values: {
          project_id: project.id,
          parent_id: root.objectId,
          type: "act",
          name: actName,
          sort_order: index,
        },
        changeSetId: root.changeSetId,
        changeSetName: "初始化作品结构",
      });
    }
  }
}

export default App;
