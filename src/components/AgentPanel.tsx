import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  Bot,
  Check,
  MessageCircle,
  Coins,
  Send,
  ShieldCheck,
  Sparkles,
  Square,
  Users,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type {
  AgentMessage,
  AgentMode,
  AgentSession,
  AgentTask,
  ExpertDefinition,
  ExpertTeamConsultation,
  ExpertTeamResult,
  ExpertType,
  RuntimeEvent,
} from "../features/agent";
import { runtimeEventName, type RuntimeDiagnostics } from "../features/agent/runtime";
import {
  buildAgentSelection,
  buildChangeAnalysisSelection,
  buildWriteScope,
  canRequestExpertTeam,
  displayRef,
  isExpertTeamRunning,
} from "../features/agent/panelState";
import type { FeatureFlags } from "../features/featureFlags";
import type { AICard, PatchProposal } from "../features/permission";
import { useSelectionStore } from "../stores/selectionStore";
import type { ProjectDescriptor, Workspace } from "../types";
import { AgentModelSettingsPanel } from "./AgentModelSettingsPanel";

interface Props {
  project: ProjectDescriptor;
  revision: number;
  workspace: Workspace;
  currentUnitId: string | null;
  activeChangeCount: number;
  activeChangeSetId: string | null;
  hasActiveChangeSet: boolean;
  onCloseChangeSet: () => void;
  onRefresh: () => Promise<void>;
  onError: (error: unknown) => void;
}

interface AgentResult {
  summary?: string;
  findings?: unknown[];
  patchProposal?: PatchProposal | null;
  questions?: string[];
  risks?: string[];
  affectedObjects?: Array<{ objectType?: string; objectId?: string; field?: string }>;
  recommendedReviewScope?: string[];
  deepAnalysisRequiresConfirmation?: boolean;
  stale?: boolean;
  baseRevision?: number;
  currentRevision?: number;
}

const expertLabels: Record<ExpertType | "main", string> = {
  main: "主 Agent",
  writer: "编剧 Agent",
  director: "导演 / 分镜 Agent",
  cinematography: "摄影 Agent",
  art: "美术 Agent",
  keyframe: "关键帧 Agent",
  prompt: "提示词 Agent",
};

const modeLabels: Record<AgentMode, string> = {
  discussion: "讨论",
  suggestion: "建议",
  edit: "编辑",
};

export function AgentPanel({ project, revision, workspace, currentUnitId, activeChangeCount, activeChangeSetId, hasActiveChangeSet, onCloseChangeSet, onRefresh, onError }: Props) {
  const selection = useSelectionStore();
  const [flags, setFlags] = useState<FeatureFlags | null>(null);
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<AgentMode>("edit");
  const [attachSelection, setAttachSelection] = useState(true);
  const [activeTask, setActiveTask] = useState<AgentTask | null>(null);
  const [activeExpert, setActiveExpert] = useState<ExpertType | "main">("main");
  const [activeToolName, setActiveToolName] = useState<string | null>(null);
  const [streamingText, setStreamingText] = useState("");
  const [proposal, setProposal] = useState<PatchProposal | null>(null);
  const [cards, setCards] = useState<AICard[]>([]);
  const [selectedPatchIds, setSelectedPatchIds] = useState<Set<string>>(new Set());
  const [working, setWorking] = useState(false);
  const [diagnosing, setDiagnosing] = useState(false);
  const [runtimeDiagnostics, setRuntimeDiagnostics] = useState<RuntimeDiagnostics | null>(null);
  const [experts, setExperts] = useState<ExpertDefinition[]>([]);
  const [showTeamBuilder, setShowTeamBuilder] = useState(false);
  const [teamRequest, setTeamRequest] = useState("");
  const [teamMembers, setTeamMembers] = useState<Set<ExpertType>>(
    new Set(["writer", "director", "cinematography"]),
  );
  const [consultation, setConsultation] = useState<ExpertTeamConsultation | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const activeTaskIdRef = useRef<string | null>(null);

  const agentEnabled = Boolean(flags?.agent_core && flags?.expert_agents);
  const agentSelection = useMemo(() => buildAgentSelection({
    projectId: project.id,
    revision,
    objectType: attachSelection ? selection.objectType : "project",
    objectId: attachSelection ? selection.objectId : project.id,
    field: attachSelection ? selection.field : null,
    selectedIds: attachSelection ? selection.selectedIds : [],
    currentUnitId: attachSelection ? currentUnitId : null,
  }), [project.id, revision, attachSelection, selection.objectType, selection.objectId, selection.field, selection.selectedIds, currentUnitId]);
  const writeScope = useMemo(
    () => buildWriteScope(agentSelection, mode),
    [agentSelection, mode],
  );
  const taskRunning = activeTask && ["created", "context_building", "queued", "running"].includes(activeTask.status);
  const teamRunning = isExpertTeamRunning(consultation?.status);

  const openSession = useCallback(async (session: AgentSession) => {
    setSessionId(session.id);
    setActiveTask(null);
    activeTaskIdRef.current = null;
    setStreamingText("");
    setActiveToolName(null);
    setProposal(null);
    setCards([]);
    setSelectedPatchIds(new Set());
    setConsultation(null);
    const history = await api.agentListMessages(project.path, session.id);
    setMessages(history);
    const prior = latestProposal(history);
    if (prior) {
      const current = await api.patchGet(project.path, prior.id);
      setProposal(current);
      setSelectedPatchIds(selectablePatchIds(current));
      setCards(await api.cardList(project.path, current.taskId));
    }
  }, [project.path]);

  useEffect(() => {
    void api.getFeatureFlags().then(setFlags).catch(onError);
  }, [onError]);

  useEffect(() => {
    if (!agentEnabled) return;
    let disposed = false;
    void (async () => {
      let items = await api.agentListSessions(project.path);
      let current = items.find((item) => item.status === "active");
      if (!items.length) {
        current = await api.agentCreateSession(project.path, {
          requestId: crypto.randomUUID(),
          projectId: project.id,
          scopeType: "project",
          scopeId: project.id,
          title: "讨论 1",
        });
        items = [current];
      }
      if (disposed) return;
      setSessions(items);
      if (current) await openSession(current);
      else {
        setSessionId(null);
        setMessages([]);
      }
    })().catch(onError);
    return () => { disposed = true; };
  }, [agentEnabled, project.id, project.path, openSession, onError]);

  useEffect(() => {
    if (!agentEnabled) return;
    void api.agentListExperts().then(setExperts).catch(onError);
  }, [agentEnabled, onError]);

  useEffect(() => {
    if (!sessionId || !flags?.expert_team) return;
    void api.expertTeamList(project.path, sessionId)
      .then((items) => setConsultation(items[0] ?? null))
      .catch(onError);
  }, [sessionId, flags?.expert_team, project.path, onError]);

  useEffect(() => {
    if (!consultation || !["running", "synthesizing"].includes(consultation.status)) return;
    const timer = window.setInterval(() => {
      void api.expertTeamGet(project.path, consultation.id).then(async (next) => {
        setConsultation(next);
        if (!["running", "synthesizing"].includes(next.status) && sessionId) {
          setMessages(await api.agentListMessages(project.path, sessionId));
        }
      }).catch(onError);
    }, 600);
    return () => window.clearInterval(timer);
  }, [consultation?.id, consultation?.status, project.path, sessionId, onError]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<RuntimeEvent>(runtimeEventName, (event) => {
      const runtimeEvent = event.payload;
      if (runtimeEvent.task_id !== activeTaskIdRef.current) return;
      if (runtimeEvent.type === "text_delta") {
        setStreamingText((value) => value + runtimeEvent.delta);
      } else if (runtimeEvent.type === "task_started") {
        setActiveTask((task) => task ? { ...task, status: "running" } : task);
      } else if (runtimeEvent.type === "tool_call_requested") {
        setActiveToolName(runtimeEvent.tool_name);
      } else if (runtimeEvent.type === "tool_call_completed") {
        setActiveToolName(null);
      } else if (["task_completed", "task_failed", "task_cancelled"].includes(runtimeEvent.type)) {
        setActiveToolName(null);
        void refreshTask(runtimeEvent.task_id);
      }
    }).then((cleanup) => {
      if (disposed) cleanup(); else unlisten = cleanup;
    }).catch(onError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [project.path, sessionId, onError]);

  const refreshTask = async (taskId: string) => {
    activeTaskIdRef.current = taskId;
    const task = await api.agentGetTask(project.path, taskId);
    setActiveTask(task);
    setStreamingText("");
    setActiveToolName(null);
    if (sessionId) setMessages(await api.agentListMessages(project.path, sessionId));
    const result = task.result as AgentResult | null;
    const nextProposal = result?.patchProposal ?? null;
    setProposal(nextProposal);
    setSelectedPatchIds(nextProposal ? selectablePatchIds(nextProposal) : new Set());
    setCards(await api.cardList(project.path, taskId));
  };

  const enableAgent = async () => {
    setWorking(true);
    try {
      await api.setFeatureFlag("agent_core", true);
      setFlags(await api.setFeatureFlag("expert_agents", true));
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const newDiscussion = async () => {
    if (taskRunning || teamRunning || working) return;
    setWorking(true);
    try {
      const session = await api.agentCreateSession(project.path, {
        requestId: crypto.randomUUID(),
        projectId: project.id,
        scopeType: "project",
        scopeId: project.id,
        title: `讨论 ${sessions.length + 1}`,
      });
      setSessions((items) => [session, ...items]);
      await openSession(session);
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const switchDiscussion = async (nextSessionId: string) => {
    if (nextSessionId === sessionId || taskRunning || teamRunning || working) return;
    const selected = sessions.find((item) => item.id === nextSessionId);
    if (!selected) return;
    setWorking(true);
    try {
      const session = selected.status === "closed"
        ? await api.agentResumeSession(project.path, selected.id)
        : selected;
      setSessions((items) => items.map((item) => item.id === session.id ? session : item));
      await openSession(session);
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const closeDiscussion = async () => {
    if (!sessionId || taskRunning || teamRunning || working) return;
    setWorking(true);
    try {
      const closed = await api.agentCloseSession(project.path, sessionId);
      const nextSessions = sessions.map((item) => item.id === closed.id ? closed : item);
      setSessions(nextSessions);
      const next = nextSessions.find((item) => item.status === "active" && item.id !== closed.id);
      if (next) {
        await openSession(next);
      } else {
        setSessionId(null);
        setMessages([]);
        setActiveTask(null);
        activeTaskIdRef.current = null;
        setStreamingText("");
        setActiveToolName(null);
        setProposal(null);
        setCards([]);
        setConsultation(null);
      }
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const sendMessage = async () => {
    const message = input.trim();
    if (!message || !sessionId || taskRunning || teamRunning) return;
    setWorking(true);
    setInput("");
    setProposal(null);
    setCards([]);
    setStreamingText("");
    setActiveToolName(null);
    try {
      const requestId = crypto.randomUUID();
      const dispatch = await api.agentSendMessage(project.path, {
        requestId,
        sessionId,
        message,
        workspace,
        mode,
        selection: agentSelection,
        writeScope,
        tokenBudget: 8_000,
      });
      setActiveExpert(dispatch.route.expertType ?? "main");
      activeTaskIdRef.current = dispatch.taskId;
      const task = await api.agentGetTask(project.path, dispatch.taskId);
      if (["completed", "failed", "cancelled", "waiting_for_user", "stale"].includes(task.status)) {
        await refreshTask(dispatch.taskId);
      } else {
        setActiveTask(task);
        setMessages(await api.agentListMessages(project.path, sessionId));
        if (!dispatch.runtimeStarted) await refreshTask(dispatch.taskId);
      }
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const analyzeChangeSet = async () => {
    if (!activeChangeSetId || activeChangeCount === 0 || !sessionId || taskRunning || teamRunning) return;
    setWorking(true);
    setProposal(null);
    setCards([]);
    setStreamingText("");
    setActiveToolName(null);
    try {
      if (!flags?.change_analysis) {
        setFlags(await api.setFeatureFlag("change_analysis", true));
      }
      const dispatch = await api.agentSendMessage(project.path, {
        requestId: crypto.randomUUID(),
        sessionId,
        message: `分析本轮修改（${activeChangeCount} 项）`,
        workspace,
        mode: "change_analysis",
        selection: buildChangeAnalysisSelection(project.id, activeChangeSetId, revision),
        writeScope: { refs: [], protectedRefs: [] },
        tokenBudget: 12_000,
      });
      setActiveExpert("main");
      activeTaskIdRef.current = dispatch.taskId;
      const task = await api.agentGetTask(project.path, dispatch.taskId);
      if (["completed", "failed", "cancelled", "waiting_for_user", "stale"].includes(task.status)) {
        await refreshTask(dispatch.taskId);
      } else {
        setActiveTask(task);
        setMessages(await api.agentListMessages(project.path, sessionId));
        if (!dispatch.runtimeStarted) await refreshTask(dispatch.taskId);
      }
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const openTeamBuilder = async () => {
    if (!flags?.expert_team) {
      try {
        setFlags(await api.setFeatureFlag("expert_team", true));
      } catch (error) {
        onError(error);
        return;
      }
    }
    setShowTeamBuilder(true);
  };

  const requestTeam = async () => {
    const message = teamRequest.trim();
    if (!sessionId || !message || teamMembers.size < 2 || taskRunning || teamRunning) return;
    setWorking(true);
    try {
      const next = await api.expertTeamRequest(project.path, {
        requestId: crypto.randomUUID(),
        sessionId,
        message,
        selection: agentSelection,
        members: [...teamMembers],
        tokenBudget: 8_000,
      });
      setConsultation(next);
      setTeamRequest("");
      setShowTeamBuilder(false);
      setMessages(await api.agentListMessages(project.path, sessionId));
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const confirmTeam = async () => {
    if (!consultation || consultation.status !== "awaiting_confirmation") return;
    setWorking(true);
    try {
      setConsultation(await api.expertTeamConfirm(project.path, consultation.id));
    } catch (error) {
      try {
        setConsultation(await api.expertTeamGet(project.path, consultation.id));
      } catch {
        // Keep the confirmation error as the actionable message.
      }
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const cancelTeam = async () => {
    if (!consultation) return;
    setWorking(true);
    try {
      const next = await api.expertTeamCancel(project.path, consultation.id);
      setConsultation(next);
      if (sessionId) setMessages(await api.agentListMessages(project.path, sessionId));
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const stopTask = async () => {
    if (!activeTask) return;
    try {
      await api.agentCancelTask(activeTask.id);
    } catch (error) {
      onError(error);
    }
  };

  const applyPatch = async (ids: Set<string>) => {
    if (!proposal || proposal.status === "stale") return;
    const candidates = proposal.items.filter((item) => item.permissionState !== "denied" && item.applyState === "pending");
    const approvedItemIds = candidates
      .filter((item) => ids.has(item.id) && item.permissionState === "requires_confirmation")
      .map((item) => item.id);
    const rejectedItemIds = candidates.filter((item) => !ids.has(item.id)).map((item) => item.id);
    setWorking(true);
    try {
      await api.patchApply(project.path, {
        proposalId: proposal.id,
        approvedItemIds,
        rejectedItemIds,
        permissionCardId: proposal.permissionCardId,
      });
      setProposal(await api.patchGet(project.path, proposal.id));
      setCards(await api.cardList(project.path, proposal.taskId));
      await onRefresh();
    } catch (error) {
      try {
        setProposal(await api.patchGet(project.path, proposal.id));
      } catch {
        // Keep the original error as the actionable message.
      }
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const rejectPatch = async () => {
    if (!proposal) return;
    setWorking(true);
    try {
      setProposal(await api.patchReject(project.path, proposal.id));
      setCards(await api.cardList(project.path, proposal.taskId));
    } catch (error) {
      onError(error);
    } finally {
      setWorking(false);
    }
  };

  const discuss = (text: string) => {
    setMode("discussion");
    setInput(text);
    window.setTimeout(() => inputRef.current?.focus(), 0);
  };

  const resolveCard = async (card: AICard, status: "resolved" | "dismissed", action: string) => {
    try {
      await api.cardResolve(project.path, { cardId: card.id, status, resolution: { action } });
      if (activeTask) setCards(await api.cardList(project.path, activeTask.id));
    } catch (error) {
      onError(error);
    }
  };

  const diagnoseRuntime = async () => {
    setDiagnosing(true);
    try {
      setRuntimeDiagnostics(await api.agentRuntimeDoctor());
    } catch (error) {
      onError(error);
    } finally {
      setDiagnosing(false);
    }
  };

  if (!flags) return <div className="agent-loading">正在读取 Agent 配置…</div>;
  if (!agentEnabled) {
    return (
      <div className="agent-enable">
        <div className="agent-avatar"><Sparkles size={15} /></div>
        <strong>启用主 Agent 工作区</strong>
        <p>启用后可围绕当前选区调用单一专业 Agent。所有修改仍需预览和确认。</p>
        <button className="primary full" disabled={working} onClick={() => void enableAgent()}>
          {working ? "正在启用…" : "启用 Agent"}
        </button>
        <small>Pi SDK Agent Host 已随工作台内置；启用后可直接配置 Provider 与模型。</small>
        <button className="ghost full" disabled={diagnosing} onClick={() => void diagnoseRuntime()}>{diagnosing ? "正在检测…" : "检测 Agent Host"}</button>
        {runtimeDiagnostics && <small>{runtimeDiagnostics.healthy ? `Agent Host 正常 · Pi SDK ${runtimeDiagnostics.sdkVersion ?? "未知"} · ${runtimeDiagnostics.modelCount} 个模型` : runtimeDiagnostics.error}</small>}
      </div>
    );
  }

  return (
    <div className="agent-panel">
      <section className="agent-session-row">
        <select
          aria-label="当前讨论"
          value={sessionId ?? ""}
          disabled={Boolean(taskRunning) || teamRunning || working}
          onChange={(event) => void switchDiscussion(event.target.value)}
        >
          {!sessionId && <option value="" disabled>选择或新建讨论</option>}
          {sessions.map((session) => (
            <option key={session.id} value={session.id}>
              {session.title}{session.status === "closed" ? "（已结束，选择后恢复）" : ""}
            </option>
          ))}
        </select>
        <button className="ghost" disabled={Boolean(taskRunning) || teamRunning || working} onClick={() => void newDiscussion()}>
          新建讨论
        </button>
        <button className="ghost" disabled={!sessionId || Boolean(taskRunning) || teamRunning || working} onClick={() => void closeDiscussion()}>
          结束讨论
        </button>
      </section>
      <AgentModelSettingsPanel disabled={Boolean(taskRunning) || teamRunning || working} onError={onError} />
      <section className="agent-context-strip">
        <div><span>当前对象</span><strong>{displayRef(agentSelection.center)}</strong></div>
        <div><span>模式 / revision</span><strong>{modeLabels[mode]} · r{revision}</strong></div>
        <div><span>写入 / 保护</span><strong>{writeScope.refs.length} / {writeScope.protectedRefs.length} 字段</strong></div>
      </section>

      <section className="agent-status-row" aria-live="polite">
        <span className={`agent-status-dot ${taskRunning ? "running" : ""}`} />
        <strong>{expertLabels[activeExpert]}</strong>
        <small>{activeToolName ? `正在读取：${activeToolName}` : activeTask ? taskStatusLabel(activeTask.status) : "等待你的请求"}</small>
        {taskRunning && <button className="agent-stop" onClick={() => void stopTask()}><Square size={11} />停止</button>}
        {teamRunning && <button className="agent-stop" onClick={() => void cancelTeam()}><Square size={11} />取消会诊</button>}
        {!taskRunning && !teamRunning && <button className="agent-stop" disabled={diagnosing} onClick={() => void diagnoseRuntime()}>{diagnosing ? "检测中" : "Runtime 检测"}</button>}
      </section>
      {runtimeDiagnostics?.error && <p className="agent-runtime-error"><AlertTriangle size={12} />{runtimeDiagnostics.error}</p>}
      {runtimeDiagnostics?.healthy && <p className="agent-runtime-ok">Pi SDK {runtimeDiagnostics.sdkVersion} · ModelRuntime {runtimeDiagnostics.modelRuntimeHealthy ? "正常" : "异常"} · Provider 已登录 {runtimeDiagnostics.providerAuth.filter((item) => item.configured).length}/{runtimeDiagnostics.providerCount} · Session {runtimeDiagnostics.sessionHealth.active} · Tool Gateway {runtimeDiagnostics.toolGatewayHealthy ? "正常" : "异常"}</p>}

      <section className="agent-change-row">
        <span>本轮修改 <strong>{activeChangeCount}</strong> 项</span>
        {hasActiveChangeSet && (
          <button
            className="secondary"
            disabled={activeChangeCount === 0 || Boolean(taskRunning) || working}
            onClick={() => void analyzeChangeSet()}
          >
            <Sparkles size={11} />{taskRunning ? "分析中…" : "分析本轮修改"}
          </button>
        )}
        {hasActiveChangeSet && <button className="ghost" onClick={onCloseChangeSet}>结束本轮</button>}
        <button className="ghost" disabled={Boolean(taskRunning) || teamRunning} onClick={() => void openTeamBuilder()}><Users size={11} />专家团</button>
        <button className="ghost" onClick={() => selection.select({ workspace: "history" })}>历史 / 快照</button>
      </section>

      <section className="agent-conversation" aria-label="Agent 对话">
        {showTeamBuilder && (
          <article className="expert-team-builder">
            <header><Users size={14} /><strong>申请专家团会诊</strong><span>只读 · 高成本</span></header>
            <textarea
              aria-label="专家团会诊问题"
              value={teamRequest}
              placeholder="描述需要多个专业方向共同判断的问题"
              onChange={(event) => setTeamRequest(event.target.value)}
            />
            <div className="expert-team-options">
              {experts.map((item) => (
                <label key={item.expertType}>
                  <input
                    type="checkbox"
                    checked={teamMembers.has(item.expertType)}
                    onChange={() => setTeamMembers((current) => {
                      const next = new Set(current);
                      if (next.has(item.expertType)) next.delete(item.expertType); else next.add(item.expertType);
                      return next;
                    })}
                  />
                  {item.displayName}
                </label>
              ))}
            </div>
            <p><Coins size={12} />申请后只生成确认卡；确认前不会启动任何专家任务。</p>
            <div className="expert-team-actions">
              <button className="ghost" onClick={() => setShowTeamBuilder(false)}>取消</button>
              <button className="primary" disabled={!canRequestExpertTeam(teamRequest, teamMembers, working)} onClick={() => void requestTeam()}>生成申请卡</button>
            </div>
          </article>
        )}
        {consultation && (
          <ExpertTeamView
            consultation={consultation}
            disabled={working}
            onConfirm={() => void confirmTeam()}
            onCancel={() => void cancelTeam()}
          />
        )}
        {!messages.length && (
          <div className="agent-welcome">
            <Bot size={22} />
            <strong>围绕当前选区开始共创</strong>
            <p>我会先判断专业方向，只读取必要上下文；含糊请求会先澄清。</p>
          </div>
        )}
        {messages.map((message) => <AgentMessageView key={message.id} message={message} />)}
        {streamingText && (
          <article className="agent-message assistant streaming">
            <header><Bot size={13} /><strong>{expertLabels[activeExpert]}</strong><span>生成中</span></header>
            <p>{streamingText}</p>
          </article>
        )}
        {Boolean(activeTask?.error) && (
          <p className="agent-runtime-error" role="alert">
            <AlertTriangle size={13} />{taskErrorMessage(activeTask?.error)} 请在 AI 模型设置中检查 Provider 登录与模型配置后重试。
          </p>
        )}
        {activeTask?.status === "stale" && (
          <p className="stale-warning"><AlertTriangle size={13} />项目事实已变化，这份分析基于 r{activeTask.contextRevision}，请重新分析。</p>
        )}
        {cards.map((card) => (
          <article className={`ai-card ${card.cardType}`} key={card.id}>
            <header><ShieldCheck size={14} /><strong>{card.title}</strong><span>{card.status}</span></header>
            <p>{card.body}</p>
            {card.cardType === "permission" && <PermissionCardDetails card={card} />}
            {(card.cardType === "problem" || card.cardType === "suggestion" || card.cardType === "stale") && <AnalysisCardDetails card={card} />}
            {card.status === "open" && card.cardType !== "permission" && activeTask?.status !== "stale" && (
              <div className="agent-card-actions">
                <button className="ghost" onClick={() => discuss(`继续讨论：${card.title}`)}><MessageCircle size={12} />讨论</button>
                {(card.cardType === "problem" || card.cardType === "suggestion") && <button className="ghost" onClick={() => discuss(`请申请相应专业 Agent 复查：${card.title}`)}><Bot size={12} />专家复查</button>}
                <button className="ghost" onClick={() => void resolveCard(card, "dismissed", "ignore")}><X size={12} />忽略</button>
                <button className="secondary" onClick={() => void resolveCard(card, "resolved", "mark_affected")}><Check size={12} />标记受影响</button>
              </div>
            )}
          </article>
        ))}
        {proposal && <PatchDiff
          proposal={proposal}
          selectedIds={selectedPatchIds}
          onToggle={(id) => setSelectedPatchIds((current) => {
            const next = new Set(current);
            if (next.has(id)) next.delete(id); else next.add(id);
            return next;
          })}
          onApplySelected={() => void applyPatch(selectedPatchIds)}
          onApplyAll={() => void applyPatch(selectablePatchIds(proposal))}
          onReject={() => void rejectPatch()}
          onDiscuss={() => discuss(`继续讨论修改提案“${proposal.title}”：`)}
          disabled={working}
        />}
      </section>

      <section className="agent-composer">
        <div className="agent-mode-tabs" aria-label="Agent 模式">
          {(Object.keys(modeLabels) as AgentMode[]).map((value) => (
            <button className={mode === value ? "active" : ""} key={value} onClick={() => setMode(value)}>{modeLabels[value]}</button>
          ))}
        </div>
        <textarea
          ref={inputRef}
          value={input}
          aria-label="给主 Agent 的消息"
          placeholder={taskRunning ? "任务运行中…" : "描述你希望分析或修改的内容"}
          disabled={Boolean(taskRunning) || teamRunning}
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              void sendMessage();
            }
          }}
        />
        <div className="agent-composer-footer">
          <label><input type="checkbox" checked={attachSelection} onChange={(event) => setAttachSelection(event.target.checked)} />附加当前选区</label>
          <button className="primary" disabled={!input.trim() || !sessionId || Boolean(taskRunning) || teamRunning || working} onClick={() => void sendMessage()}>
            <Send size={13} />发送
          </button>
        </div>
      </section>
    </div>
  );
}

function ExpertTeamView({ consultation, disabled, onConfirm, onCancel }: {
  consultation: ExpertTeamConsultation;
  disabled: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const result = consultation.result as ExpertTeamResult | null;
  const actionable = consultation.status === "awaiting_confirmation";
  const cancellable = ["awaiting_confirmation", "running", "synthesizing"].includes(consultation.status);
  return (
    <article className={`expert-team-card ${consultation.status}`}>
      <header><Users size={14} /><strong>专家团会诊</strong><span>{teamStatusLabel(consultation.status)}</span></header>
      <p className="expert-team-question">{consultation.userRequest}</p>
      <div className="expert-team-members">
        {consultation.members.map((member) => (
          <span className={member.status} key={member.id}>
            {expertLabels[member.expertType]} · {memberStatusLabel(member.status)}
          </span>
        ))}
      </div>
      {actionable && (
        <>
          <section className="expert-team-application">
            <strong>会诊申请</strong>
            <small>每位专家使用独立 Pi AgentSession 按需读取事实，并且互不查看彼此意见。</small>
          </section>
          <section className="expert-team-cost">
            <Coins size={13} />
            <div><strong>成本等级：高</strong><small>默认只读；确认前不会创建或启动专业任务。</small></div>
          </section>
        </>
      )}
      {consultation.status === "synthesizing" && <p className="expert-team-progress">各专家已完成，主 Agent 正在综合共识与分歧…</p>}
      {result && consultation.status !== "cancelled" && (
        <div className="expert-team-result">
          <strong>{result.summary ?? "会诊已结束"}</strong>
          <ResultGroup title="共识" values={result.consensus} />
          <ResultGroup title="分歧" values={result.disagreements} emphasis />
          <ResultGroup title="建议" values={result.recommendations} />
          {result.questions?.map((question) => <p className="agent-question" key={question}>{question}</p>)}
          {result.risks?.map((risk) => <p className="agent-risk" key={risk}><AlertTriangle size={12} />{risk}</p>)}
          <small>会诊结果只读；如需修改，请另行发起修改提案。</small>
        </div>
      )}
      {consultation.status === "cancelled" && <p className="expert-team-progress">会诊已取消，没有写入项目事实。</p>}
      {consultation.status === "failed" && <p className="agent-runtime-error"><AlertTriangle size={12} />会诊未完成，请检查 Runtime 配置后重新申请。</p>}
      {consultation.status === "stale" && <p className="stale-warning"><AlertTriangle size={12} />项目已从 r{consultation.baseRevision} 发生变化，请重新申请会诊。</p>}
      {(actionable || cancellable) && (
        <div className="expert-team-actions">
          <button className="ghost danger" disabled={disabled} onClick={onCancel}>{actionable ? "放弃申请" : "取消会诊"}</button>
          {actionable && <button className="primary" disabled={disabled} onClick={onConfirm}><Check size={12} />确认专家与高成本并启动</button>}
        </div>
      )}
    </article>
  );
}

function ResultGroup({ title, values, emphasis = false }: { title: string; values?: unknown[]; emphasis?: boolean }) {
  if (!values?.length) return null;
  return (
    <section className={emphasis ? "expert-team-disagreements" : ""}>
      <strong>{title}</strong>
      <ul>{values.map((value, index) => <li key={index}>{displayValue(value)}</li>)}</ul>
    </section>
  );
}

function AgentMessageView({ message }: { message: AgentMessage }) {
  const structured = message.structured as AgentResult | null;
  return (
    <article className={`agent-message ${message.role}`}>
      <header>{message.role === "user" ? <span>你</span> : <Bot size={13} />}<strong>{message.role === "user" ? "用户" : "主 Agent"}</strong></header>
      <p>{message.content}</p>
      {structured?.findings?.length ? <ul>{structured.findings.map((finding, index) => <li key={index}>{displayValue(finding)}</li>)}</ul> : null}
      {structured?.affectedObjects?.length ? <p className="agent-impact-list">受影响对象：{structured.affectedObjects.map((reference) => `${reference.objectType}:${reference.objectId}${reference.field ? `.${reference.field}` : ""}`).join("、")}</p> : null}
      {structured?.recommendedReviewScope?.length ? <p className="agent-impact-list">建议复查：{structured.recommendedReviewScope.join("、")}</p> : null}
      {structured?.deepAnalysisRequiresConfirmation && <p className="agent-question">跨剧集深度分析需要你确认后才能继续。</p>}
      {structured?.stale && <p className="agent-risk"><AlertTriangle size={12} />分析结果已过期（r{structured.baseRevision} → r{structured.currentRevision}）。</p>}
      {structured?.questions?.map((question) => <p className="agent-question" key={question}>{question}</p>)}
      {structured?.risks?.map((risk) => <p className="agent-risk" key={risk}><AlertTriangle size={12} />{risk}</p>)}
    </article>
  );
}

function PatchDiff({ proposal, selectedIds, onToggle, onApplySelected, onApplyAll, onReject, onDiscuss, disabled }: {
  proposal: PatchProposal;
  selectedIds: Set<string>;
  onToggle: (id: string) => void;
  onApplySelected: () => void;
  onApplyAll: () => void;
  onReject: () => void;
  onDiscuss: () => void;
  disabled: boolean;
}) {
  const actionable = ["draft", "pending", "approved"].includes(proposal.status);
  return (
    <article className={`patch-diff ${proposal.status}`}>
      <header><Sparkles size={14} /><strong>{proposal.title}</strong><span>{proposal.status}</span></header>
      {proposal.items.map((item) => (
        <label className={`patch-item ${item.permissionState}`} key={item.id}>
          <input
            type="checkbox"
            checked={selectedIds.has(item.id)}
            disabled={!actionable || item.permissionState === "denied"}
            onChange={() => onToggle(item.id)}
          />
          <span className="patch-field">{item.objectType}:{item.objectId}.{item.fieldName}</span>
          <span className={`permission-badge ${item.permissionState}`}>{permissionLabel(item.permissionState)}</span>
          <div className="patch-values"><del>{displayValue(item.oldValue)}</del><ins>{displayValue(item.newValue)}</ins></div>
          <small>{item.reason}</small>
        </label>
      ))}
      {proposal.status === "stale" && <p className="stale-warning"><AlertTriangle size={13} />项目事实已变化，这份提案不能直接应用。</p>}
      {actionable && (
        <div className="patch-actions">
          <button className="ghost" disabled={disabled} onClick={onDiscuss}>继续讨论</button>
          <button className="ghost danger" disabled={disabled} onClick={onReject}>拒绝</button>
          <button className="secondary" disabled={disabled || selectedIds.size === 0} onClick={onApplySelected}>应用选中</button>
          <button className="primary" disabled={disabled} onClick={onApplyAll}>应用全部</button>
        </div>
      )}
    </article>
  );
}

function PermissionCardDetails({ card }: { card: AICard }) {
  const options = card.options as { currentWriteScope?: { refs?: unknown[] }; requestedScope?: Array<{ objectType?: string; objectId?: string; field?: string; reason?: string }>; oneTimeOnly?: boolean; impact?: string };
  return (
    <div className="permission-details">
      <small>当前授权：{options.currentWriteScope?.refs?.length ?? 0} 项</small>
      {options.requestedScope?.map((request, index) => <code key={index}>{request.objectType}:{request.objectId}.{request.field} · {request.reason}</code>)}
      <small>{options.oneTimeOnly ? "仅限本次" : "持续授权"} · {options.impact}</small>
    </div>
  );
}

function AnalysisCardDetails({ card }: { card: AICard }) {
  const options = card.options as {
    evidence?: unknown[];
    affectedObjects?: Array<{ objectType?: string; objectId?: string; field?: string }>;
    recommendedReviewScope?: string[];
    deepAnalysisRequiresConfirmation?: boolean;
    baseRevision?: number;
    currentRevision?: number;
  };
  return (
    <div className="analysis-card-details">
      {card.relatedRef && <code>{displayRef(card.relatedRef)}</code>}
      {options.evidence?.map((evidence, index) => <small key={index}>依据：{displayValue(evidence)}</small>)}
      {options.affectedObjects?.length ? (
        <small>影响：{options.affectedObjects.map((reference) => `${reference.objectType}:${reference.objectId}${reference.field ? `.${reference.field}` : ""}`).join("、")}</small>
      ) : null}
      {options.recommendedReviewScope?.length ? <small>建议复查：{options.recommendedReviewScope.join("、")}</small> : null}
      {options.deepAnalysisRequiresConfirmation && <small>跨剧集深度分析需要你确认后才能继续。</small>}
      {card.cardType === "stale" && options.baseRevision !== undefined && <small>r{options.baseRevision} → r{options.currentRevision}</small>}
    </div>
  );
}

function selectablePatchIds(proposal: PatchProposal): Set<string> {
  return new Set(proposal.items.filter((item) => item.permissionState !== "denied" && item.applyState === "pending").map((item) => item.id));
}

function latestProposal(messages: AgentMessage[]): PatchProposal | null {
  for (const message of [...messages].reverse()) {
    const proposal = (message.structured as AgentResult | null)?.patchProposal;
    if (proposal?.id) return proposal;
  }
  return null;
}

function displayValue(value: unknown): string {
  if (typeof value === "string") return value || "（空）";
  if (value === null || value === undefined) return "（空）";
  return JSON.stringify(value);
}

function permissionLabel(state: string): string {
  return ({ allowed: "范围内", requires_confirmation: "需确认", denied: "受保护", stale: "已过期" } as Record<string, string>)[state] ?? state;
}

function taskStatusLabel(status: string): string {
  return ({ created: "正在理解", context_building: "正在组装上下文", queued: "正在调用专业 Agent", running: "专业 Agent 处理中", waiting_for_user: "等待你的决定", completed: "已完成", cancelled: "已取消", failed: "失败", stale: "结果过期", interrupted: "已中断" } as Record<string, string>)[status] ?? status;
}

function teamStatusLabel(status: string): string {
  return ({ awaiting_confirmation: "等待确认", running: "专家独立分析中", synthesizing: "主 Agent 综合中", completed: "已完成", cancelled: "已取消", failed: "失败", stale: "已过期" } as Record<string, string>)[status] ?? status;
}

function memberStatusLabel(status: string): string {
  return ({ planned: "待确认", queued: "等待运行", running: "分析中", completed: "已完成", cancelled: "已取消", failed: "失败", stale: "已过期" } as Record<string, string>)[status] ?? status;
}

function taskErrorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return "Agent 任务失败。";
}
