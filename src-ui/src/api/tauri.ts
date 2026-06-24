import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============================================================
// Event types for Tauri streaming
// ============================================================

export interface StreamChunk {
  content: string;
  done: boolean;
  error?: string;
}

export interface AgentStreamEvent {
  payload: { content: string };
}

export interface AgentDoneEvent {
  payload: { message: string };
}

export interface AgentErrorEvent {
  payload: { error: string; code?: string };
}

export interface FileEvent {
  payload: {
    action: string;
    path: string;
    success: boolean;
    detail: string;
    timestamp: string;
  };
}

export interface LogEvent {
  payload: {
    level: string;
    category: string;
    message: string;
    detail: string;
    timestamp: string;
  };
}

// ============================================================
// Agent chat types
// ============================================================

export interface ChatMessageForAgent {
  role: string;
  content: string;
}

export interface AgentChatRequest {
  prompt: string;
  history: ChatMessageForAgent[];
  max_turns?: number;
  safe_mode?: boolean;
}

export interface AgentChatResponse {
  content: string;
  token_usage?: TokenUsage;
}

export interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

// ============================================================
// Session types
// ============================================================

export interface SaveSessionRequest {
  session_id: string;
  name?: string;
  messages: ChatMessageForAgent[];
  metadata: Record<string, unknown>;
}

export interface LoadSessionResponse {
  session_id: string;
  name: string;
  messages: ChatMessageForAgent[];
  metadata: Record<string, unknown>;
}

export interface SessionInfo {
  session_id: string;
  name: string;
  message_count: number;
  updated_at: string;
}

export interface RenameSessionRequest {
  session_id: string;
  new_name: string;
}

// ============================================================
// Log types
// ============================================================

export interface LogEntry {
  level: string;
  category: string;
  message: string;
  detail: string;
  timestamp: string;
}

export interface LogFilterRequest {
  level?: string;
  category?: string;
  limit?: number;
}

// ============================================================
// Layout types
// ============================================================

export interface LayoutConfig {
  explorer_width: number;
  chat_width: number;
  console_height: number;
  theme: string;
  window_x: number;
  window_y: number;
  window_width: number;
  window_height: number;
}

export interface LayoutSaveRequest {
  explorer_width: number;
  chat_width: number;
  console_height: number;
}

export interface WindowStateRequest {
  x: number;
  y: number;
  width: number;
  height: number;
}

// ============================================================
// Streaming chat — returns stop function
// ============================================================

export function streamChat(
  message: string,
  params: { temperature?: number; maxTokens?: number },
  callbacks: {
    onChunk: (text: string) => void;
    onDone: (fullText: string) => void;
    onError: (err: string) => void;
  },
): Promise<() => void> {
  return new Promise((resolveSetup) => {
    let unlisten1: UnlistenFn | null = null;
    let unlisten2: UnlistenFn | null = null;
    let unlisten3: UnlistenFn | null = null;
    let acc = "";
    let resolved = false;

    function cleanup() {
      unlisten1?.();
      unlisten2?.();
      unlisten3?.();
    }

    invoke("chat_send", {
      message,
      params: {
        temperature: params.temperature ?? 0.7,
        max_tokens: params.maxTokens ?? 4096,
      },
    }).catch((e) => {
      if (!resolved) {
        resolved = true;
        callbacks.onError(String(e));
        cleanup();
      }
    });

    Promise.all([
      listen("agent_stream", (e: unknown) => {
        const ev = e as AgentStreamEvent;
        acc += ev.payload.content;
        callbacks.onChunk(acc);
      }),
      listen("agent_done", (e: unknown) => {
        const ev = e as AgentDoneEvent;
        if (!resolved) {
          resolved = true;
          callbacks.onDone(acc || ev.payload.message);
          cleanup();
        }
      }),
      listen("agent_error", (e: unknown) => {
        const ev = e as AgentErrorEvent;
        if (!resolved) {
          resolved = true;
          callbacks.onError(ev.payload.error);
          cleanup();
        }
      }),
    ]).then(([u1, u2, u3]) => {
      unlisten1 = u1;
      unlisten2 = u2;
      unlisten3 = u3;

      resolveSetup(() => {
        resolved = true;
        cleanup();
      });
    });
  });
}

// ============================================================
// File operations
// ============================================================

export interface FileTreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileTreeNode[];
}

export async function readFileTree(dir: string): Promise<FileTreeNode[]> {
  return invoke<FileTreeNode[]>("file_read_tree", { dir });
}

export async function readFileContent(filePath: string): Promise<string> {
  return invoke<string>("file_read_content", { filePath });
}

export async function writeFile(path: string, content: string): Promise<boolean> {
  return invoke<boolean>("file_write", { req: { path, content } });
}

export async function flushPendingWrites(): Promise<number> {
  return invoke<number>("flush_pending_writes");
}

export async function createFile(
  parentDir: string,
  name: string,
  isDir: boolean,
): Promise<FileTreeNode> {
  return invoke<FileTreeNode>("file_create", { req: { parentDir, name, isDir } });
}

export async function deleteFile(path: string, isDir: boolean): Promise<boolean> {
  return invoke<boolean>("file_delete", { req: { path, isDir } });
}

export async function renameFile(oldPath: string, newName: string): Promise<string> {
  return invoke<string>("file_rename", { req: { oldPath, newName } });
}

export async function importExternalFile(sourcePath: string, targetDir: string): Promise<string> {
  return invoke<string>("file_import_external", { sourcePath, targetDir });
}

// ============================================================
// Plans
// ============================================================

export interface PlanSummary {
  id: string;
  name: string;
  status: string;
  steps_count: number;
  current_step: number;
  created_at: string;
}

export async function listPlans(): Promise<PlanSummary[]> {
  return invoke<PlanSummary[]>("plan_list");
}

export async function createPlan(name: string): Promise<PlanSummary> {
  return invoke<PlanSummary>("plan_create", { req: { name } });
}

export async function executePlan(planId: string): Promise<string> {
  return invoke<string>("plan_execute", { planId });
}

export async function deletePlan(planId: string): Promise<boolean> {
  return invoke<boolean>("plan_delete", { planId });
}

// ============================================================
// Agent status
// ============================================================

export async function getAgentStatus(): Promise<{
  initialized: boolean;
  version: string;
  status: string;
}> {
  return invoke("agent_status");
}

// ============================================================
// Config
// ============================================================

export async function getConfig(key: string): Promise<string | null> {
  return invoke<string | null>("config_get", { key });
}

export async function setConfig(key: string, value: string): Promise<boolean> {
  return invoke<boolean>("config_set", { key, value });
}

// ============================================================
// Log operations
// ============================================================

export async function getRecentLogs(filter?: LogFilterRequest): Promise<LogEntry[]> {
  return invoke<LogEntry[]>("log_get_recent", { filter });
}

export async function clearLogs(): Promise<boolean> {
  return invoke<boolean>("log_clear");
}

// ============================================================
// Layout & theme operations
// ============================================================

export async function getLayout(): Promise<LayoutConfig> {
  return invoke<LayoutConfig>("layout_get");
}

export async function saveLayout(req: LayoutSaveRequest): Promise<boolean> {
  return invoke<boolean>("layout_save", { req });
}

export async function setTheme(theme: string): Promise<boolean> {
  return invoke<boolean>("theme_set", { theme });
}

export async function getTheme(): Promise<string> {
  return invoke<string>("theme_get");
}

export async function saveWindowState(req: WindowStateRequest): Promise<boolean> {
  return invoke<boolean>("window_state_save", { req });
}

export async function getWindowState(): Promise<{
  x: number;
  y: number;
  width: number;
  height: number;
}> {
  return invoke("window_state_get");
}

// ============================================================
// Event listeners for app-level use
// ============================================================

export function onFileEvent(cb: (e: FileEvent) => void): Promise<UnlistenFn> {
  return listen("file-event", cb);
}

export function onAgentEvent(cb: (e: unknown) => void): Promise<UnlistenFn> {
  return listen("agent-event", cb);
}

export function onLogEvent(cb: (e: LogEvent) => void): Promise<UnlistenFn> {
  return listen("log-event", cb);
}

// ============================================================
// Agent chat command (calls actual agent inference)
// ============================================================

export function runAgentChat(
  message: string,
  history: ChatMessageForAgent[],
  params: { maxTurns?: number; safeMode?: boolean },
  callbacks: {
    onChunk: (text: string) => void;
    onDone: (fullText: string) => void;
    onError: (err: string) => void;
  },
): Promise<() => void> {
  return new Promise((resolveSetup) => {
    let unlisten1: UnlistenFn | null = null;
    let unlisten2: UnlistenFn | null = null;
    let unlisten3: UnlistenFn | null = null;
    let acc = "";
    let resolved = false;

    function cleanup() {
      unlisten1?.();
      unlisten2?.();
      unlisten3?.();
    }

    invoke("run_agent_chat", {
      req: {
        prompt: message,
        history,
        max_turns: params.maxTurns ?? 3,
        safe_mode: params.safeMode ?? false,
      },
    }).catch((e) => {
      if (!resolved) {
        resolved = true;
        callbacks.onError(String(e));
        cleanup();
      }
    });

    Promise.all([
      listen("agent_stream", (e: unknown) => {
        const ev = e as AgentStreamEvent;
        acc = ev.payload.content;
        callbacks.onChunk(acc);
      }),
      listen("agent_done", (e: unknown) => {
        const ev = e as AgentDoneEvent;
        if (!resolved) {
          resolved = true;
          callbacks.onDone(acc || ev.payload.message);
          cleanup();
        }
      }),
      listen("agent_error", (e: unknown) => {
        const ev = e as AgentErrorEvent;
        if (!resolved) {
          resolved = true;
          callbacks.onError(ev.payload.error);
          cleanup();
        }
      }),
    ]).then(([u1, u2, u3]) => {
      unlisten1 = u1;
      unlisten2 = u2;
      unlisten3 = u3;

      resolveSetup(() => {
        resolved = true;
        cleanup();
      });
    });
  });
}

// ============================================================
// Session management
// ============================================================

export async function saveSession(
  sessionId: string,
  messages: ChatMessageForAgent[],
  metadata: Record<string, unknown>,
  name?: string,
): Promise<boolean> {
  return invoke<boolean>("save_session", {
    req: { session_id: sessionId, name, messages, metadata },
  });
}

export async function loadSession(sessionId: string): Promise<LoadSessionResponse> {
  return invoke<LoadSessionResponse>("load_session", { req: { session_id: sessionId } });
}

export async function deleteSession(sessionId: string): Promise<boolean> {
  return invoke<boolean>("delete_session", { sessionId });
}

export async function renameSession(sessionId: string, newName: string): Promise<boolean> {
  return invoke<boolean>("rename_session", { req: { session_id: sessionId, new_name: newName } });
}

export async function listSessions(): Promise<SessionInfo[]> {
  return invoke<SessionInfo[]>("list_sessions");
}