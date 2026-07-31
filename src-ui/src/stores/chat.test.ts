import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useChatStore } from "./chat";

// Mock the Tauri API layer — stores must be testable without a backend.
const tauriApi = vi.hoisted(() => ({
  streamChat: vi.fn(),
  runAgentChat: vi.fn(),
  saveSession: vi.fn(),
  loadSession: vi.fn(),
  deleteSession: vi.fn(),
  listSessions: vi.fn(),
}));

vi.mock("../api/tauri", () => tauriApi);

describe("chat store", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    setActivePinia(createPinia());
  });

  it("初始化时自动创建默认会话", () => {
    const chat = useChatStore();
    expect(chat.sessions.length).toBe(1);
    expect(chat.activeSessionId).toBe(chat.sessions[0].id);
    expect(chat.messages.length).toBe(0);
  });

  it("newSession 创建新会话并切换为活动会话", () => {
    const chat = useChatStore();
    const first = chat.sessions[0].id;
    chat.newSession("My Task");
    expect(chat.sessions.length).toBe(2);
    expect(chat.sessions[0].name).toBe("My Task");
    expect(chat.activeSessionId).not.toBe(first);
  });

  it("renameSession 只修改目标会话名称", () => {
    const chat = useChatStore();
    const id = chat.activeSessionId!;
    chat.renameSession(id, "Renamed");
    expect(chat.activeSession!.name).toBe("Renamed");
  });

  it("clearContext 清空当前会话消息", () => {
    const chat = useChatStore();
    const session = chat.sessions[0];
    session.messages.push({
      id: "m1",
      role: "user",
      content: "hello",
      timestamp: Date.now(),
    });
    chat.clearContext();
    expect(chat.messages.length).toBe(0);
  });

  it("stopStreaming 停止流式状态并清空缓冲", () => {
    const chat = useChatStore();
    chat.streaming = true;
    chat.streamingContent = "partial text";
    chat.stopStreaming();
    expect(chat.streaming).toBe(false);
    expect(chat.streamingContent).toBe("");
  });

  it("setTemperature / setMaxTokens 更新模型参数并持久化", () => {
    const chat = useChatStore();
    chat.setTemperature(0.3);
    chat.setMaxTokens(2048);
    expect(chat.modelParams.temperature).toBe(0.3);
    expect(chat.modelParams.maxTokens).toBe(2048);
    const saved = JSON.parse(localStorage.getItem("rupoo:model_params")!);
    expect(saved.temperature).toBe(0.3);
  });

  it("sendMessage 追加用户消息并进入流式状态", async () => {
    const chat = useChatStore();
    const stop = vi.fn();
    tauriApi.runAgentChat.mockResolvedValue(stop);
    await chat.sendMessage("写一个排序算法");
    expect(chat.messages.length).toBe(1);
    expect(chat.messages[0].role).toBe("user");
    expect(chat.messages[0].content).toBe("写一个排序算法");
    expect(chat.streaming).toBe(true);
    // 触发 onDone 结束流式并追加助手消息
    const onDone = tauriApi.runAgentChat.mock.calls[0][3].onDone;
    onDone("排序算法如下…");
    expect(chat.streaming).toBe(false);
    expect(chat.messages.length).toBe(2);
    expect(chat.messages[1].role).toBe("assistant");
  });

  it("exportSession 导出单个会话 JSON", () => {
    const chat = useChatStore();
    const id = chat.activeSessionId!;
    const json = chat.exportSession(id);
    const parsed = JSON.parse(json!);
    expect(parsed.id).toBe(id);
    expect(parsed.messages).toEqual([]);
  });
});
