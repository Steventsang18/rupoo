import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useLayoutStore } from "./layout";

const tauriApi = vi.hoisted(() => ({
  getConfig: vi.fn(),
  setConfig: vi.fn(() => Promise.resolve(true)),
}));

vi.mock("../api/tauri", () => tauriApi);

describe("layout store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setActivePinia(createPinia());
  });

  it("默认面板尺寸与可见性", () => {
    const layout = useLayoutStore();
    expect(layout.leftWidth).toBe(220);
    expect(layout.rightWidth).toBe(280);
    expect(layout.bottomHeight).toBe(200);
    expect(layout.showExplorer).toBe(true);
    expect(layout.showTerminal).toBe(true);
  });

  it("toggleExplorer / toggleChat / toggleTerminal 切换可见性", () => {
    const layout = useLayoutStore();
    layout.toggleExplorer();
    expect(layout.showExplorer).toBe(false);
    layout.toggleChat();
    expect(layout.showChat).toBe(false);
    layout.toggleTerminal();
    expect(layout.showTerminal).toBe(false);
    expect(layout.bottomStyle.height).toBe("0px");
  });

  it("setLeftWidth 更新并持久化到后端配置", () => {
    const layout = useLayoutStore();
    layout.setLeftWidth(320);
    expect(layout.leftWidth).toBe(320);
    expect(tauriApi.setConfig).toHaveBeenCalledWith("rupoo:leftWidth", "320");
  });

  it("loadConfig 应用后端配置并做边界钳制", async () => {
    tauriApi.getConfig
      .mockResolvedValueOnce("9999")
      .mockResolvedValueOnce("240")
      .mockResolvedValueOnce("50");
    const layout = useLayoutStore();
    await layout.loadConfig();
    expect(layout.leftWidth).toBe(400); // 钳制到上限
    expect(layout.rightWidth).toBe(240);
    expect(layout.bottomHeight).toBe(80); // 钳制到下限
  });

  it("loadConfig 后端异常时保持默认值", async () => {
    tauriApi.getConfig.mockRejectedValue(new Error("offline"));
    const layout = useLayoutStore();
    await layout.loadConfig();
    expect(layout.leftWidth).toBe(220);
    expect(layout.bottomHeight).toBe(200);
  });
});
