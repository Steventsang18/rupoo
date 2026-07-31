import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia } from "pinia";
import WelcomePage from "./WelcomePage.vue";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

function mountPage() {
  return mount(WelcomePage, {
    global: { plugins: [createPinia()] },
    attachTo: document.body,
  });
}

describe("WelcomePage", () => {
  it("渲染英雄区标题与副标题", () => {
    const wrapper = mountPage();
    expect(wrapper.find(".hero-title").text()).toBe("Your AI Agent Desktop");
    expect(wrapper.find(".hero-subtitle").exists()).toBe(true);
  });

  it("渲染 4 个快捷操作卡片", () => {
    const wrapper = mountPage();
    const cards = wrapper.findAll(".action-card");
    expect(cards.length).toBe(4);
    expect(cards[0].find(".action-title").text()).toBe("Connect AI Provider");
  });

  it("未配置 API key 时显示设置引导 CTA，点击后弹出 Provider 配置弹窗", async () => {
    const wrapper = mountPage();
    expect(wrapper.find(".hero-cta").exists()).toBe(true);
    await wrapper.find(".hero-cta").trigger("click");
    expect(wrapper.find(".modal-card").exists()).toBe(true);
    expect(wrapper.find(".modal-title").text()).toBe("Connect AI Provider");
    const providerOptions = wrapper.findAll(".provider-option");
    expect(providerOptions.length).toBeGreaterThanOrEqual(5);
    // 输入 API key 后主按钮可用
    await wrapper.find('input[type="password"]').setValue("sk-test-key");
    const saveBtn = wrapper.find(".btn-primary");
    expect((saveBtn.element as HTMLButtonElement).disabled).toBe(false);
  });
});
