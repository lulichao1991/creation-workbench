import { type ToolDefinition } from "@earendil-works/pi-coding-agent";
import { Type, type TSchema } from "typebox";

export interface ToolGatewayRequest {
  toolCallId: string;
  sessionId: string;
  taskId: string;
  parentTaskId?: string;
  toolName: string;
  arguments: Record<string, unknown>;
}

export type ToolGateway = (request: ToolGatewayRequest, signal?: AbortSignal) => Promise<unknown>;

interface ToolSpec {
  name: string;
  label: string;
  description: string;
  parameters: TSchema;
}

export interface CallExpertInput {
  toolCallId: string;
  expertType: string;
  task: string;
  focusRefs: Array<{ objectType: string; objectId: string }>;
  signal?: AbortSignal;
}

interface WorkbenchToolOptions {
  allowedToolNames?: readonly string[];
  parentTaskId?: string;
  callExpert?: (input: CallExpertInput) => Promise<unknown>;
}

const objectRef = {
  objectType: Type.String({ minLength: 1, maxLength: 48 }),
  objectId: Type.String({ minLength: 1, maxLength: 256 }),
};

const toolSpecs: ToolSpec[] = [
  { name: "get_selection", label: "读取当前选区", description: "读取当前选区、多选、写入范围、保护范围和项目 revision。", parameters: Type.Object({}, { additionalProperties: false }) },
  { name: "read_object", label: "读取对象", description: "按对象类型和 ID 读取当前项目内的完整非敏感结构化事实。", parameters: Type.Object(objectRef, { additionalProperties: false }) },
  { name: "read_parent", label: "读取上级", description: "读取对象的直接上级对象。", parameters: Type.Object(objectRef, { additionalProperties: false }) },
  { name: "read_children", label: "读取下级", description: "读取对象的直接下级对象，结果有数量上限。", parameters: Type.Object({ ...objectRef, limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 100 })) }, { additionalProperties: false }) },
  { name: "read_neighbors", label: "读取相邻对象", description: "读取镜头、场、内容单元或故事元素出现点的前后相邻对象。", parameters: Type.Object({ ...objectRef, count: Type.Optional(Type.Integer({ minimum: 1, maximum: 5 })) }, { additionalProperties: false }) },
  { name: "read_scene", label: "读取场", description: "读取场的完整事实及其镜头列表。", parameters: Type.Object({ sceneId: Type.String({ minLength: 1, maxLength: 256 }) }, { additionalProperties: false }) },
  { name: "read_shot_context", label: "读取镜头上下文", description: "读取镜头、所在场、前后镜头和正式关联资产元数据。", parameters: Type.Object({ shotId: Type.String({ minLength: 1, maxLength: 256 }) }, { additionalProperties: false }) },
  { name: "read_asset", label: "读取资产", description: "读取正式资产事实、需求、关联镜头和媒体元数据；不返回本地路径或图片 Base64。", parameters: Type.Object({ assetId: Type.String({ minLength: 1, maxLength: 256 }) }, { additionalProperties: false }) },
  { name: "read_generation_task", label: "读取生成任务", description: "读取静态生成任务、关联镜头和提示词编译记录。", parameters: Type.Object({ generationTaskId: Type.String({ minLength: 1, maxLength: 256 }) }, { additionalProperties: false }) },
  { name: "compile_prompt_preview", label: "编译提示词预览", description: "调用确定性 PromptCompiler 生成只读预览、来源与警告；不会设为正式提示词或调用视频模型。", parameters: Type.Object({ generationTaskId: Type.String({ minLength: 1, maxLength: 256 }), modelProfileKey: Type.Optional(Type.String({ minLength: 1, maxLength: 128 })), templateId: Type.Optional(Type.String({ minLength: 1, maxLength: 256 })) }, { additionalProperties: false }) },
  { name: "read_story_structure", label: "读取故事结构", description: "按项目、季、集或故事元素读取内容单元、StoryElement、Occurrence 和关系。", parameters: Type.Object({ scopeType: Type.Optional(Type.Union([Type.Literal("project"), Type.Literal("season"), Type.Literal("episode"), Type.Literal("contentUnit"), Type.Literal("storyElement")])), scopeId: Type.Optional(Type.String({ minLength: 1, maxLength: 256 })), limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 160 })) }, { additionalProperties: false }) },
  { name: "search_project", label: "搜索项目", description: "全文搜索剧本、镜头、资产、关系、StoryElement 和项目记忆。", parameters: Type.Object({ query: Type.String({ minLength: 1, maxLength: 200 }), limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 50 })) }, { additionalProperties: false }) },
  { name: "read_active_memories", label: "读取有效记忆", description: "按对象作用域读取当前有效的项目记忆和长期记忆。", parameters: Type.Object({ objectType: Type.Optional(Type.String({ minLength: 1, maxLength: 48 })), objectId: Type.Optional(Type.String({ minLength: 1, maxLength: 256 })) }, { additionalProperties: false }) },
  { name: "read_change_set", label: "读取修改集", description: "读取本轮 ChangeSet 和其中的字段级变更。", parameters: Type.Object({ changeSetId: Type.String({ minLength: 1, maxLength: 256 }) }, { additionalProperties: false }) },
];

export function createWorkbenchTools(
  sessionId: string,
  currentTaskId: () => string | undefined,
  gateway: ToolGateway,
  options: WorkbenchToolOptions = {},
): ToolDefinition[] {
  const allowed = options.allowedToolNames && new Set(options.allowedToolNames);
  const tools: ToolDefinition[] = toolSpecs
    .filter((spec) => !allowed || allowed.has(spec.name))
    .map((spec): ToolDefinition => ({
    name: spec.name,
    label: spec.label,
    description: spec.description,
    promptSnippet: `${spec.name}: ${spec.description}`,
    parameters: spec.parameters,
    executionMode: "parallel",
    execute: async (toolCallId, params, signal) => {
      const taskId = currentTaskId();
      if (!taskId) throw new Error("当前没有可审计的 Agent 任务");
      const result = await gateway({
        toolCallId,
        sessionId,
        taskId,
        parentTaskId: options.parentTaskId,
        toolName: spec.name,
        arguments: params as Record<string, unknown>,
      }, signal);
      return {
        content: [{ type: "text", text: JSON.stringify(result) }],
        details: {},
      };
    },
    }));
  if (options.callExpert) {
    tools.push({
      name: "call_expert",
      label: "调用专业 Agent",
      description: "创建一个独立专业 Pi AgentSession，让其自行读取项目事实并返回结构化专业意见。仅在需要专业判断时调用。",
      promptSnippet: "call_expert: 调用 writer、director、cinematography、art、keyframe 或 prompt 专业 Agent；专业 Agent 会独立读取事实，禁止猜测。",
      parameters: Type.Object({
        expertType: Type.Union([
          Type.Literal("writer"),
          Type.Literal("director"),
          Type.Literal("cinematography"),
          Type.Literal("art"),
          Type.Literal("keyframe"),
          Type.Literal("prompt"),
        ]),
        task: Type.String({ minLength: 1, maxLength: 4_000 }),
        focusRefs: Type.Optional(Type.Array(Type.Object(objectRef, { additionalProperties: false }), { maxItems: 8 })),
      }, { additionalProperties: false }),
      executionMode: "sequential",
      execute: async (toolCallId, params, signal) => {
        const input = params as {
          expertType: string;
          task: string;
          focusRefs?: Array<{ objectType: string; objectId: string }>;
        };
        return {
          content: [{
            type: "text",
            text: JSON.stringify(await options.callExpert!({
              toolCallId,
              expertType: input.expertType,
              task: input.task,
              focusRefs: input.focusRefs ?? [],
              signal,
            })),
          }],
          details: {},
        };
      },
    });
  }
  return tools;
}

export const WORKBENCH_TOOL_NAMES = [...toolSpecs.map((spec) => spec.name), "call_expert"];
