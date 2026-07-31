import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";

export type TabId = "chat" | "agent" | "files" | "sessions" | "settings";

export const useAppStore = defineStore("app", () => {
  // ── Nav state ──
  const activeTab = ref<TabId>("chat");
  const previousTab = ref<TabId | null>(null);

  function navigate(tab: TabId) {
    if (tab === activeTab.value) return;
    previousTab.value = activeTab.value;
    activeTab.value = tab;
  }

  // ── Agent state ──
  const agentStatus = ref<"initializing" | "ready" | "error" | "offline">("initializing");
  const version = ref("0.6.3");
  const modelInfo = ref<string | null>(null);
  const tokenUsage = ref({ prompt: 0, completion: 0, total: 0 });

  // ── Settings state ──
  const settings = ref({
    provider: "openai",
    model: "gpt-4o",
    apiKey: "",
    temperature: 0.7,
    maxTokens: 4096,
    theme: "dark" as "dark" | "light",
    firstLaunch: true,
  });

  // ── Computed ──
  const needsSetup = computed(() => !settings.value.apiKey);

  // ── Init ──
  async function init() {
    // Load persisted settings
    try {
      const saved = localStorage.getItem("rupoo:settings");
      if (saved) {
        const parsed = JSON.parse(saved);
        settings.value = { ...settings.value, ...parsed };
        applyTheme(settings.value.theme);
      }
    } catch { /* ignore */ }

    // Try to connect to backend
    try {
      const status = await invoke<{
        initialized: boolean;
        version: string;
        status: string;
      }>("agent_status");
      agentStatus.value = status.status as "ready" | "error";
      version.value = status.version;
    } catch {
      agentStatus.value = "offline";
    }

    // Listen for agent events
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen("agent-event", (e: any) => {
        if (e.payload.event === "agent_initialized") {
          agentStatus.value = "ready";
        } else if (e.payload.event === "agent_init_failed") {
          agentStatus.value = "error";
        }
      });
      await listen("agent_token_usage", (e: any) => {
        tokenUsage.value = e.payload;
      });
    } catch { /* ignore */ }
  }

  // ── Theme ──
  function applyTheme(t: "dark" | "light") {
    document.documentElement.setAttribute("data-theme", t);
    settings.value.theme = t;
    persistSettings();
  }

  function toggleTheme() {
    applyTheme(settings.value.theme === "dark" ? "light" : "dark");
  }

  // ── Settings persistence ──
  function persistSettings() {
    localStorage.setItem("rupoo:settings", JSON.stringify(settings.value));
  }

  function updateSettings(partial: Partial<typeof settings.value>) {
    settings.value = { ...settings.value, ...partial };
    persistSettings();
  }

  function completeSetup() {
    settings.value.firstLaunch = false;
    persistSettings();
  }

  return {
    activeTab,
    previousTab,
    agentStatus,
    version,
    modelInfo,
    tokenUsage,
    settings,
    needsSetup,
    navigate,
    init,
    applyTheme,
    toggleTheme,
    updateSettings,
    completeSetup,
    persistSettings,
  };
});
