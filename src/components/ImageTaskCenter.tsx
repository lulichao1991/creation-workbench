import { AlertCircle, CheckCircle2, Clock3, LoaderCircle, RefreshCw, XCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api } from "../api";
import { terminalImageStatuses, type ImageJob } from "../services/imageGeneration";
import type { ProjectState } from "../types";

const statusLabels: Record<ImageJob["status"], string> = {
  created: "已创建",
  queued: "排队中",
  running: "生成中",
  completed: "已完成",
  partial: "部分成功",
  cancelled: "已取消",
  failed: "失败",
  interrupted: "已中断",
};

export function useRecentImageJobs(projectPath: string) {
  const [jobs, setJobs] = useState<ImageJob[]>([]);
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    const flags = await api.getFeatureFlags();
    setEnabled(flags.image_generation);
    setJobs(flags.image_generation ? await api.imageListRecentJobs(projectPath) : []);
  }, [projectPath]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    void refresh().catch(() => undefined).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [refresh]);

  const hasActiveJob = jobs.some((job) => !terminalImageStatuses.has(job.status));
  useEffect(() => {
    const onUpdate = () => { void refresh().catch(() => undefined); };
    window.addEventListener("workbench:image-jobs-updated", onUpdate);
    const timer = window.setInterval(onUpdate, hasActiveJob ? 1000 : 15000);
    return () => {
      window.removeEventListener("workbench:image-jobs-updated", onUpdate);
      window.clearInterval(timer);
    };
  }, [hasActiveJob, refresh]);

  return { jobs, enabled, loading, refresh };
}

export function ImageTaskCenter({
  projectPath,
  state,
  jobs,
  loading,
  onRefresh,
  onError,
}: {
  projectPath: string;
  state: ProjectState;
  jobs: ImageJob[];
  loading: boolean;
  onRefresh: () => Promise<void>;
  onError: (error: unknown) => void;
}) {
  const cancel = async (jobId: string) => {
    try {
      await api.imageCancel(projectPath, jobId);
      await onRefresh();
      window.dispatchEvent(new Event("workbench:image-jobs-updated"));
    } catch (error) {
      onError(error);
    }
  };

  return (
    <div className="image-task-center">
      <div className="image-task-summary">
        <div><span>生成中</span><strong>{jobs.filter((job) => !terminalImageStatuses.has(job.status)).length}</strong></div>
        <div><span>待选候选</span><strong>{jobs.reduce((count, job) => count + job.results.filter((result) => result.selectionState === "available").length, 0)}</strong></div>
        <div><span>失败 / 中断</span><strong>{jobs.filter((job) => ["failed", "interrupted"].includes(job.status)).length}</strong></div>
      </div>
      <div className="image-task-toolbar">
        <button className="ghost" disabled={loading} onClick={() => void onRefresh().catch(onError)}><RefreshCw size={13} className={loading ? "spin" : ""} />刷新</button>
      </div>
      {jobs.length === 0 ? <div className="panel-empty">暂无生图任务</div> : (
        <div className="image-task-rows">{jobs.map((job) => {
          const active = !terminalImageStatuses.has(job.status);
          return (
            <article className={`image-task-row ${job.status}`} key={job.id}>
              <span className="image-task-icon">{active ? <LoaderCircle size={15} className="spin" /> : job.status === "completed" ? <CheckCircle2 size={15} /> : job.status === "partial" ? <AlertCircle size={15} /> : ["failed", "interrupted"].includes(job.status) ? <XCircle size={15} /> : <Clock3 size={15} />}</span>
              <div>
                <strong>{targetLabel(state, job)}</strong>
                <small>{statusLabels[job.status]} · {job.model || "默认模型"} · {job.results.length} 个结果</small>
                {job.error?.message && <em>{job.error.message}</em>}
              </div>
              {active && <button className="danger-text" onClick={() => void cancel(job.id)}>取消</button>}
            </article>
          );
        })}</div>
      )}
    </div>
  );
}

function targetLabel(state: ProjectState, job: ImageJob) {
  if (job.targetType === "assetRequirement") {
    const requirement = state.assetRequirements.find((item) => item.id === job.targetId);
    const asset = state.assets.find((item) => item.id === requirement?.asset_id);
    return `${asset?.name ?? "资产"} · ${requirement?.requirement_type ?? "图片需求"}`;
  }
  const frame = state.keyframes.find((item) => item.id === job.targetId);
  const shot = state.shots.find((item) => item.id === frame?.shot_id);
  return `${shot?.title ?? "分镜"} · ${frameTypeLabel(frame?.type)}`;
}

function frameTypeLabel(type?: string) {
  return ({ single: "分镜图", start: "起始帧", middle: "中间帧", end: "结束帧" } as Record<string, string>)[type ?? ""] ?? "关键帧";
}
