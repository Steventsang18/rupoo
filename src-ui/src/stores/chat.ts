import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { streamChat, runAgentChat, saveSession, loadSession, deleteSession, listSessions, type ChatMessageForAgent } from "../api/tauri";

// ============================================================
// Types
// ============================================================

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: number;
}

export interface ChatSession {
  id: string;
  name: string;
  messages: ChatMessage[];
  createdAt: number;
  updatedAt: number;
}

export interface ModelParams {
  temperature: number;
  maxTokens: number;
}

// ============================================================
// Helpers
// ============================================================

const STORAGE_KEY = "rupoo:chat_sessions";
const PARAMS_KEY = "rupoo:model_params";

function uid(): string {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
}

function loadSessions(): ChatSession[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveSessions(sessions: ChatSession[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions));
}

function loadParams(): ModelParams {
  try {
    const raw = localStorage.getItem(PARAMS_KEY);
    if (raw) return JSON.parse(raw);
  } catch { /* ignore */ }
  return { temperature: 0.7, maxTokens: 4096 };
}

function saveParams(p: ModelParams) {
  localStorage.setItem(PARAMS_KEY, JSON.stringify(p));
}

// ============================================================
// Store
// ============================================================

export const useChatStore = defineStore("chat", () => {
  // ── State ──
  const sessions = ref<ChatSession[]>(loadSessions());
  const activeSessionId = ref<string | null>(sessions.value[0]?.id ?? null);
  const streaming = ref(false);
  const sending = ref(false);
  const streamingContent = ref("");
  const error = ref<string | null>(null);
  const modelParams = ref<ModelParams>(loadParams());
  let stopFn: (() => void) | null = null;

  // ── Getters ──
  const activeSession = computed(() =>
    sessions.value.find((s) => s.id === activeSessionId.value) ?? null,
  );

  const messages = computed(() => activeSession.value?.messages ?? []);

  // ── Persistence ──
  function persist() {
    saveSessions(sessions.value);
  }

  // ── Session management ──
  function newSession(name?: string) {
    const id = uid();
    const session: ChatSession = {
      id,
      name: name || `Chat ${sessions.value.length + 1}`,
      messages: [],
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    sessions.value.unshift(session);
    activeSessionId.value = id;
    persist();
  }

  function deleteSession(id: string) {
    const idx = sessions.value.findIndex((s) => s.id === id);
    if (idx === -1) return;
    sessions.value.splice(idx, 1);
    if (activeSessionId.value === id) {
      activeSessionId.value = sessions.value[0]?.id ?? null;
    }
    persist();
    // Also delete from backend
    deleteFromBackend(id);
  }

  function renameSession(id: string, name: string) {
    const s = sessions.value.find((s) => s.id === id);
    if (!s) return;
    s.name = name;
    s.updatedAt = Date.now();
    persist();
  }

  function switchSession(id: string) {
    if (sessions.value.some((s) => s.id === id)) {
      activeSessionId.value = id;
    }
  }

  // ── Messaging ──
  async function sendMessage(content: string, useAgent = true) {
    if (!activeSessionId.value) newSession();

    const session = sessions.value.find((s) => s.id === activeSessionId.value);
    if (!session) return;

    // Add user message
    const userMsg: ChatMessage = {
      id: uid(),
      role: "user",
      content,
      timestamp: Date.now(),
    };
    session.messages.push(userMsg);
    session.updatedAt = Date.now();
    persist();

    // Start streaming
    streaming.value = true;
    streamingContent.value = "";
    error.value = null;

    // Convert history for agent
    const history: ChatMessageForAgent[] = session.messages
      .slice(0, -1)
      .map((m) => ({ role: m.role, content: m.content }));

    const chatFn = useAgent ? runAgentChat : streamChat;

    if (useAgent) {
      stopFn = await runAgentChat(
        content,
        history,
        { maxTurns: 3, safeMode: false },
        {
          onChunk(text) {
            streamingContent.value = text;
          },
          onDone(fullText) {
            const assistantMsg: ChatMessage = {
              id: uid(),
              role: "assistant",
              content: fullText,
              timestamp: Date.now(),
            };
            session.messages.push(assistantMsg);
            session.updatedAt = Date.now();
            streaming.value = false;
            streamingContent.value = "";
            persist();
            // Auto-save to backend
            saveToBackend(session.id);
          },
          onError(err) {
            error.value = err;
            streaming.value = false;
            streamingContent.value = "";
            persist();
          },
        },
      );
    } else {
      stopFn = await streamChat(
        content,
        { temperature: modelParams.value.temperature, maxTokens: modelParams.value.maxTokens },
        {
          onChunk(text) {
            streamingContent.value = text;
          },
          onDone(fullText) {
            const assistantMsg: ChatMessage = {
              id: uid(),
              role: "assistant",
              content: fullText,
              timestamp: Date.now(),
            };
            session.messages.push(assistantMsg);
            session.updatedAt = Date.now();
            streaming.value = false;
            streamingContent.value = "";
            persist();
          },
          onError(err) {
            error.value = err;
            streaming.value = false;
            streamingContent.value = "";
            persist();
          },
        },
      );
    }
  }

  // ── Backend persistence ──
  async function saveToBackend(sessionId: string) {
    const session = sessions.value.find((s) => s.id === sessionId);
    if (!session) return;

    try {
      const messages: ChatMessageForAgent[] = session.messages.map((m) => ({
        role: m.role,
        content: m.content,
      }));
      await saveSession(sessionId, messages, {
        name: session.name,
        createdAt: session.createdAt,
        updatedAt: session.updatedAt,
      });
    } catch (e) {
      console.warn("Failed to save session to backend:", e);
    }
  }

  async function loadFromBackend(sessionId: string): Promise<boolean> {
    try {
      const result = await loadSession(sessionId);
      const session = sessions.value.find((s) => s.id === sessionId);
      if (session) {
        session.messages = result.messages.map((m) => ({
          id: uid(),
          role: m.role as "user" | "assistant" | "system",
          content: m.content,
          timestamp: Date.now(),
        }));
        session.updatedAt = Date.now();
        persist();
        return true;
      }
    } catch (e) {
      console.warn("Failed to load session from backend:", e);
    }
    return false;
  }

  async function deleteFromBackend(sessionId: string): Promise<void> {
    try {
      await deleteSession(sessionId);
    } catch (e) {
      console.warn("Failed to delete session from backend:", e);
    }
  }

  async function listBackendSessions(): Promise<{ id: string; name: string; messageCount: number }[]> {
    try {
      const sessions = await listSessions();
      return (sessions as any[]).map((s: any) => ({
        id: s.id || String(s),
        name: s.name || "Chat",
        messageCount: s.message_count || 0,
      }));
    } catch (e) {
      console.warn("Failed to list sessions from backend:", e);
      return [];
    }
  }

  function stopStreaming() {
    stopFn?.();
    streaming.value = false;
    streamingContent.value = "";
  }

  function clearError() {
    error.value = null;
  }

  // ── Context management ──
  function clearContext() {
    const session = sessions.value.find((s) => s.id === activeSessionId.value);
    if (!session) return;
    session.messages = [];
    session.updatedAt = Date.now();
    persist();
  }

  // ── Model params ──
  function setTemperature(t: number) {
    modelParams.value.temperature = t;
    saveParams(modelParams.value);
  }

  function setMaxTokens(t: number) {
    modelParams.value.maxTokens = t;
    saveParams(modelParams.value);
  }

  // ── Export ──
  function exportSession(id: string): string | null {
    const s = sessions.value.find((s) => s.id === id);
    if (!s) return null;
    return JSON.stringify(s, null, 2);
  }

  function exportAll(): string {
    return JSON.stringify(sessions.value, null, 2);
  }

  // ── Init ──
  if (sessions.value.length === 0) {
    newSession();
  }

  return {
    sessions,
    activeSessionId,
    streaming,
    streamingContent,
    error,
    modelParams,
    activeSession,
    messages,
    newSession,
    deleteSession,
    renameSession,
    switchSession,
    sendMessage,
    stopStreaming,
    clearError,
    clearContext,
    setTemperature,
    setMaxTokens,
    exportSession,
    exportAll,
    saveToBackend,
    loadFromBackend,
    listBackendSessions,
  };
});
