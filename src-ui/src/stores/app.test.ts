import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useAppStore } from "./app";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("app store", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    setActivePinia(createPinia());
  });

  it("默认进入 chat 页且无 API key 时 needsSetup 为 true", () => {
    const app = useAppStore();
    expect(app.activeTab).toBe("chat");
    expect(app.needsSetup).toBe(true);
  });

  it("navigate 切换页签并记住上一个页签", () => {
    const app = useAppStore();
    app.navigate("settings");
    expect(app.activeTab).toBe("settings");
    expect(app.previousTab).toBe("chat");
    app.navigate("chat");
    expect(app.previousTab).toBe("settings");
  });

  it("toggleTheme 在 dark/light 间切换并写入 data-theme", () => {
    const app = useAppStore();
    expect(app.settings.theme).toBe("dark");
    app.toggleTheme();
    expect(app.settings.theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    app.toggleTheme();
    expect(app.settings.theme).toBe("dark");
  });

  it("updateSettings 合并部分设置，completeSetup 标记首次启动完成", () => {
    const app = useAppStore();
    app.updateSettings({ apiKey: "sk-test", model: "gpt-4o-mini" });
    expect(app.settings.apiKey).toBe("sk-test");
    expect(app.settings.model).toBe("gpt-4o-mini");
    expect(app.needsSetup).toBe(false);
    app.completeSetup();
    expect(app.settings.firstLaunch).toBe(false);
  });

  it("版本号与产品版本一致（防回归：GUI 不得显示旧版本）", () => {
    const app = useAppStore();
    expect(app.version).toBe("0.6.3");
  });

  it("init 在无后端时进入 offline 状态", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    (invoke as ReturnType<typeof vi.fn>).mockRejectedValue(new Error("no backend"));
    const app = useAppStore();
    await app.init();
    expect(app.agentStatus).toBe("offline");
  });
});
