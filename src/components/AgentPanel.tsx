import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  Bot,
  Check,
  MessageCircle,
  Send,
  ShieldCheck,
  Sparkles,
  Square,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type {
  AgentMessage,
  AgentMode,
  AgentTask,
  ExpertType,
  RuntimeEvent,
} from "../features/agent";
import { runtimeEventName } from "../features/agent/runtime";
import {
  buildAgentSelection,
  buildWriteScope,
  displayRef,
} from "../features/agent/panelState";
import type { FeatureFlags } from "../features/featureFlags";
import type { AICard, PatchProposal } from "../features/permission";
import { useSelectionStore } from "../stores/selectionStore";
import type { ProjectDescriptor, Workspace } from "../types";

interface Props {
  project: ProjectDescriptor;
  revision: number;
  workspace: Workspace;
  currentUnitId: string | null;
  activeChangeCount: number;
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

export function AgentPanel({ project, revision, workspace, currentUnitId, activeChangeCount, hasActiveChangeSet, onCloseChangeSet, onRefresh, onError }: Props) {
  const selection = useSelectionStore();
  const [flags, setFlags] = useState<FeatureFlags | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<AgentMode>("edit");
  const [attachSelection, setAttachSelection] = useState(true);
  const [activeTask, setActiveTask] = useState<AgentTask | null>(null);
  const [activeExpert, setActiveExpert] = useState<ExpertType | "main">("main");
  const [streamingText, setStreamingText] = useState("");
  const [proposal, setProposal] = useState<PatchProposal | null>(null);
  const [cards, setCards] = useState<AICard[]>([]);
  const [selectedPatchIds, setSelectedPatchIds] = useState<Set<string>>(new Set());
  const [working, setWorking] = useState(false);
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

  useEffect(() => {
    void api.getFeatureFlags().then(setFlags).catch(onError);
  }, [onError]);

  useEffect(() => {
    if (!agentEnabled) return;
    const id = `agent-ui-${project.id}`;
    void api.agentCreateSession(project.path, {
      requestId: id,
      projectId: project.id,
      scopeType: "project",
      scopeId: project.id,
      title: `${project.name} 主 Agent`,
    }).then(async (session) => {
      setSessionId(session.id);
      const history = await api.agentListMessages(project.path, session.id);
      setMessages(history);
      const prior = latestProposal(history);
      if (prior) {
        const current = await api.patchGet(project.path, prior.id);
        setProposal(current);
        setSelectedPatchIds(selectablePatchIds(current));
        setCards(await api.cardList(project.path, current.taskId));
      }
    }).catch(onError);
  }, [agentEnabled, project.id, project.name, project.path, onError]);

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
      } else if (["task_completed", "task_failed", "task_cancelled"].includes(runtimeEvent.type)) {
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

  const sendMessage = async () => {
    const message = input.trim();
    if (!message || !sessionId || taskRunning) return;
    setWorking(true);
    setInput("");
    setProposal(null);
    setCards([]);
    setStreamingText("");
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
      if (["completed", "failed", "cancelled", "waiting_for_user"].includes(task.status)) {
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

  const resolveCard = async (card: AICard, status: "resolved" | "dismissed") => {
    try {
      await api.cardResolve(project.path, { cardId: card.id, status, resolution: { action: status } });
      if (activeTask) setCards(await api.cardList(project.path, activeTask.id));
    } catch (error) {
      onError(error);
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
        <small>需要本机安装 Pi，或通过 PI_AGENT_CLI 指定 Runtime。</small>
      </div>
    );
  }

  return (
    <div className="agent-panel">
      <section className="agent-context-strip">
        <div><span>当前对象</span><strong>{displayRef(agentSelection.center)}</strong></div>
        <div><span>模式 / revision</span><strong>{modeLabels[mode]} · r{revision}</strong></div>
        <div><span>写入 / 保护</span><strong>{writeScope.refs.length} / {writeScope.protectedRefs.length} 字段</strong></div>
      </section>

      <section className="agent-status-row" aria-live="polite">
        <span className={`agent-status-dot ${taskRunning ? "running" : ""}`} />
        <strong>{expertLabels[activeExpert]}</strong>
        <small>{activeTask ? taskStatusLabel(activeTask.status) : "等待你的请求"}</small>
        {taskRunning && <button className="agent-stop" onClick={() => void stopTask()}><Square size={11} />停止</button>}
      </section>

      <section className="agent-change-row">
        <span>本轮修改 <strong>{activeChangeCount}</strong> 项</span>
        {hasActiveChangeSet && <button className="ghost" onClick={onCloseChangeSet}>结束本轮</button>}
        <button className="ghost" onClick={() => selection.select({ workspace: "history" })}>历史 / 快照</button>
      </section>

      <section className="agent-conversation" aria-label="Agent 对话">
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
            <AlertTriangle size={13} />{taskErrorMessage(activeTask?.error)} 请检查本机 Pi 或 PI_AGENT_CLI 配置后重试。
          </p>
        )}
        {cards.map((card) => (
          <article className={`ai-card ${card.cardType}`} key={card.id}>
            <header><ShieldCheck size={14} /><strong>{card.title}</strong><span>{card.status}</span></header>
            <p>{card.body}</p>
            {card.cardType === "permission" && <PermissionCardDetails card={card} />}
            {card.status === "open" && card.cardType !== "permission" && (
              <div className="agent-card-actions">
                <button className="ghost" onClick={() => discuss(`继续讨论：${card.title}`)}><MessageCircle size={12} />讨论</button>
                <button className="ghost" onClick={() => void resolveCard(card, "dismissed")}><X size={12} />忽略</button>
                <button className="secondary" onClick={() => void resolveCard(card, "resolved")}><Check size={12} />已处理</button>
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
          disabled={Boolean(taskRunning)}
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
          <button className="primary" disabled={!input.trim() || !sessionId || Boolean(taskRunning) || working} onClick={() => void sendMessage()}>
            <Send size={13} />发送
          </button>
        </div>
      </section>
    </div>
  );
}

function AgentMessageView({ message }: { message: AgentMessage }) {
  const structured = message.structured as AgentResult | null;
  return (
    <article className={`agent-message ${message.role}`}>
      <header>{message.role === "user" ? <span>你</span> : <Bot size={13} />}<strong>{message.role === "user" ? "用户" : "主 Agent"}</strong></header>
      <p>{message.content}</p>
      {structured?.findings?.length ? <ul>{structured.findings.map((finding, index) => <li key={index}>{displayValue(finding)}</li>)}</ul> : null}
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

function taskErrorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
  return "Agent 任务失败。";
}
