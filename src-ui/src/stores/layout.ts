import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { getConfig, setConfig } from "../api/tauri";

const LEFT_WIDTH_KEY = "rupoo:leftWidth";
const RIGHT_WIDTH_KEY = "rupoo:rightWidth";
const BOTTOM_HEIGHT_KEY = "rupoo:bottomHeight";

export const useLayoutStore = defineStore("layout", () => {
  // --- Panel sizes (persist to backend config) ---
  const leftWidth = ref(220);
  const rightWidth = ref(280);
  const bottomHeight = ref(200);

  // --- Visibility ---
  const showExplorer = ref(true);
  const showChat = ref(true);
  const showTerminal = ref(true);

  // --- Agent status ---
  const agentStatus = ref<"initializing" | "ready" | "error">("initializing");

  // --- Load config from backend ---
  async function loadConfig() {
    try {
      const [left, right, bottom] = await Promise.all([
        getConfig(LEFT_WIDTH_KEY),
        getConfig(RIGHT_WIDTH_KEY),
        getConfig(BOTTOM_HEIGHT_KEY),
      ]);
      if (left) leftWidth.value = Math.max(180, Math.min(400, parseInt(left) || 220));
      if (right) rightWidth.value = Math.max(200, Math.min(500, parseInt(right) || 280));
      if (bottom) bottomHeight.value = Math.max(80, Math.min(500, parseInt(bottom) || 200));
    } catch {
      // Use defaults on error
    }
  }

  // --- Resized callback (writes to backend config) ---
  function onLeftResized(e: { size: number }) {
    leftWidth.value = e.size;
    setConfig(LEFT_WIDTH_KEY, String(e.size)).catch(() => {});
  }

  function onRightResized(e: { size: number }) {
    rightWidth.value = e.size;
    setConfig(RIGHT_WIDTH_KEY, String(e.size)).catch(() => {});
  }

  function onBottomResized(e: { size: number }) {
    bottomHeight.value = e.size;
    setConfig(BOTTOM_HEIGHT_KEY, String(e.size)).catch(() => {});
  }

  // --- Direct setters with config persistence ---
  function setLeftWidth(width: number) {
    leftWidth.value = width;
    setConfig(LEFT_WIDTH_KEY, String(width)).catch(() => {});
  }

  function setRightWidth(width: number) {
    rightWidth.value = width;
    setConfig(RIGHT_WIDTH_KEY, String(width)).catch(() => {});
  }

  function setBottomHeight(height: number) {
    bottomHeight.value = height;
    setConfig(BOTTOM_HEIGHT_KEY, String(height)).catch(() => {});
  }

  // --- Toggles ---
  function toggleExplorer() {
    showExplorer.value = !showExplorer.value;
  }

  function toggleChat() {
    showChat.value = !showChat.value;
  }

  function toggleTerminal() {
    showTerminal.value = !showTerminal.value;
  }

  // --- Computed ---
  const bottomStyle = computed(() => ({
    height: showTerminal.value ? `${bottomHeight.value}px` : "0px",
  }));

  return {
    leftWidth,
    rightWidth,
    bottomHeight,
    showExplorer,
    showChat,
    showTerminal,
    agentStatus,
    loadConfig,
    onLeftResized,
    onRightResized,
    onBottomResized,
    setLeftWidth,
    setRightWidth,
    setBottomHeight,
    toggleExplorer,
    toggleChat,
    toggleTerminal,
    bottomStyle,
  };
});
