import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import "./App.css";
import { ProjectHome } from "./components/ProjectHome";
import { Workbench } from "./components/Workbench";
import { SettingsCenter } from "./components/SettingsCenter";
import { FirstRunOnboarding } from "./components/FirstRunOnboarding";
import { useSelectionStore } from "./stores/selectionStore";
import { toUserErrorMessage } from "./domain/userError";
import type {
  BatchMutationRequest,
  BatchMutationResponse,
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
  const [pendingOperations, setPendingOperations] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [activeChangeSetId, setActiveChangeSetId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState(() => localStorage.getItem("workbench.onboardingComplete") !== "true");
  const clearSelection = useSelectionStore((selection) => selection.clear);
  const saveState = useSelectionStore((selection) => selection.saveState);
  const errorTimerRef = useRef<number | null>(null);
  const busy = pendingOperations > 0;

  const beginOperation = useCallback(() => setPendingOperations((count) => count + 1), []);
  const endOperation = useCallback(() => setPendingOperations((count) => Math.max(0, count - 1)), []);
  const dismissError = useCallback(() => {
    if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
    errorTimerRef.current = null;
    setError(null);
  }, []);
  const handleError = useCallback((reason: unknown) => {
    if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
    setError(toUserErrorMessage(reason));
    errorTimerRef.current = window.setTimeout(() => {
      errorTimerRef.current = null;
      setError(null);
    }, 7000);
  }, []);

  useEffect(() => {
    void initialize();
    return () => {
      if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
    };
  }, []);

  const initialize = async () => {
    beginOperation();
    try {
      const info = await api.getDefaultWorkspace();
      const savedRoot = localStorage.getItem("workbench.projectRoot") || info.defaultPath;
      setRootPath(savedRoot);
      setProjects(await api.listProjects(savedRoot));
    } catch (reason) {
      handleError(reason);
    } finally {
      endOperation();
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
    beginOperation();
    try {
      localStorage.setItem("workbench.projectRoot", path);
      setRootPath(path);
      setProjects(await api.listProjects(path));
    } catch (reason) {
      handleError(reason);
    } finally {
      endOperation();
    }
  };

  const createProject = async (name: string, structureType: string) => {
    beginOperation();
    try {
      const project = await api.createProject(rootPath, name, structureType);
      await seedStructure(project, structureType);
      await refreshProjects();
      await openProject(project);
      return true;
    } catch (reason) {
      handleError(reason);
      return false;
    } finally {
      endOperation();
    }
  };

  const openProject = async (projectOrPath: ProjectDescriptor | string) => {
    beginOperation();
    try {
      const project =
        typeof projectOrPath === "string"
          ? await api.openProject(projectOrPath)
          : projectOrPath;
      const next = await api.loadProjectState(project.path);
      setActiveProject(project);
      setState(next);
      setActiveChangeSetId(null);
      clearSelection();
    } catch (reason) {
      handleError(reason);
    } finally {
      endOperation();
    }
  };

  const copyProject = async (project: ProjectDescriptor, name: string) => {
    beginOperation();
    try {
      await api.copyProject(project.path, name);
      await refreshProjects();
    } catch (reason) {
      handleError(reason);
    } finally {
      endOperation();
    }
  };

  const deleteProject = async (project: ProjectDescriptor) => {
    beginOperation();
    try {
      await api.deleteProject(project.path);
      await refreshProjects();
    } catch (reason) {
      handleError(reason);
    } finally {
      endOperation();
    }
  };

  const mutate = async (request: MutationRequest): Promise<MutationResponse> => {
    if (!activeProject) throw new Error("没有打开的项目");
    beginOperation();
    try {
      const result = await api.mutate(activeProject.path, {
        ...request,
        changeSetId: request.changeSetId ?? activeChangeSetId ?? undefined,
      });
      setActiveChangeSetId((current) => current ?? result.changeSetId);
      await refreshState(activeProject);
      return result;
    } catch (reason) {
      handleError(reason);
      throw reason;
    } finally {
      endOperation();
    }
  };

  const mutateBatch = async (request: BatchMutationRequest): Promise<BatchMutationResponse> => {
    if (!activeProject) throw new Error("没有打开的项目");
    beginOperation();
    try {
      const result = await api.mutateBatch(activeProject.path, {
        ...request,
        changeSetId: request.changeSetId ?? activeChangeSetId ?? undefined,
      });
      setActiveChangeSetId((current) => current ?? result.changeSetId);
      await refreshState(activeProject);
      return result;
    } catch (reason) {
      handleError(reason);
      throw reason;
    } finally {
      endOperation();
    }
  };

  const undoChangeSet = async (changeSetId: string) => {
    if (!activeProject) throw new Error("没有打开的项目");
    beginOperation();
    try {
      setActiveChangeSetId(null);
      await api.undoChangeSet(activeProject.path, changeSetId);
      await refreshState(activeProject);
    } catch (reason) {
      handleError(reason);
      throw reason;
    } finally {
      endOperation();
    }
  };

  return (
    <>
      {activeProject && state ? (
        <Workbench
          project={activeProject}
          state={state}
          busy={busy}
          saveState={saveState}
          onBack={() => {
            setActiveProject(null);
            setState(null);
            setActiveChangeSetId(null);
            clearSelection();
            void refreshProjects();
          }}
          onMutate={mutate}
          onMutateBatch={mutateBatch}
          onUndo={undoChangeSet}
          onOpenSettings={() => setSettingsOpen(true)}
          onRefresh={() => refreshState(activeProject)}
          onError={handleError}
          activeChangeSetId={activeChangeSetId}
          onCloseChangeSet={() => setActiveChangeSetId(null)}
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
          onOpenSettings={() => setSettingsOpen(true)}
        />
      )}
      {settingsOpen && <SettingsCenter rootPath={rootPath} disabled={busy} onRootChange={changeRoot} onClose={() => setSettingsOpen(false)} onError={handleError} onRestartOnboarding={() => { localStorage.removeItem("workbench.onboardingComplete"); setSettingsOpen(false); setOnboardingOpen(true); }} />}
      {onboardingOpen && !settingsOpen && <FirstRunOnboarding rootPath={rootPath} disabled={busy} onRootChange={changeRoot} onOpenSettings={() => setSettingsOpen(true)} onError={handleError} onComplete={() => { localStorage.setItem("workbench.onboardingComplete", "true"); setOnboardingOpen(false); }} />}
      {busy && <div className="busy-bar" />}
      {error && (
        <div className="toast error-toast">
          <strong>操作未完成</strong>
          <span>{error}</span>
          <button onClick={dismissError}>×</button>
        </div>
      )}
    </>
  );
}

async function seedStructure(project: ProjectDescriptor, structureType: string) {
  if (structureType === "custom") return;
  const type = structureType === "short" || structureType === "feature" ? "short" : "season";
  const name = structureType === "short" || structureType === "feature" ? "正片" : "第一季";
  const rootId = crypto.randomUUID();
  await api.mutateBatch(project.path, {
    changeSetName: "初始化作品结构",
    mutations: [
      { action: "create", entityType: "contentUnit", objectId: rootId, values: { project_id: project.id, parent_id: null, type, name, sort_order: 0 } },
      ...(structureType === "feature" ? ["第一幕", "第二幕", "第三幕"].map((actName, index) => ({
        action: "create" as const,
        entityType: "contentUnit",
        values: {
          project_id: project.id,
          parent_id: rootId,
          type: "act",
          name: actName,
          sort_order: index,
        },
      })) : []),
    ],
  });
}

export default App;
