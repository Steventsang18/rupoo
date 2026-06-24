<script setup lang="ts">
import { useTheme } from "../composables/useTheme";

const props = defineProps<{
  agentStatus: string;
  showExplorer?: boolean;
  showChat?: boolean;
  showTerminal?: boolean;
}>();

const emit = defineEmits<{
  "toggle-explorer": [];
  "toggle-chat": [];
  "toggle-terminal": [];
  "new-editor-tab": [];
}>();

const { theme, toggleTheme } = useTheme();

// Tauri window controls - only available in Tauri environment
async function minimize() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().minimize();
  } catch {}
}

async function maximize() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().maximize();
  } catch {}
}

async function closeWindow() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  } catch {}
}
</script>

<template>
  <div class="navbar">
    <div class="navbar-left">
      <!-- Theme toggle (leftmost) -->
      <button
        class="nav-icon-btn theme-btn"
        @click="toggleTheme"
        :title="theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'"
      >
        <!-- Sun icon (shown in dark mode) -->
        <svg v-if="theme === 'dark'" width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="3" stroke="currentColor" stroke-width="1.3"/>
          <path d="M8 1v1.5M8 13.5V15M1 8h1.5M13.5 8H15M3.05 3.05l1.06 1.06M11.89 11.89l1.06 1.06M3.05 12.95l1.06-1.06M11.89 4.11l1.06-1.06" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
        <!-- Moon icon (shown in light mode) -->
        <svg v-else width="16" height="16" viewBox="0 0 16 16" fill="none">
          <path d="M6 2.5a6 6 0 1 0 7.5 7.5A5.5 5.5 0 0 1 6 2.5z" stroke="currentColor" stroke-width="1.3" fill="none"/>
        </svg>
      </button>

      <div class="nav-sep" />

      <button
        class="nav-icon-btn"
        :class="{ active: props.showExplorer }"
        @click="emit('toggle-explorer')"
        title="Toggle Explorer"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <rect x="1" y="2" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <rect x="1" y="9" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <rect x="9" y="2" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <rect x="9" y="9" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
        </svg>
      </button>

      <span class="logo">
        <span class="logo-icon">R</span>
        rupoo
      </span>
      <span class="version">v0.4.1</span>
    </div>

    <div class="navbar-center">
      <!-- Global tabs bar - only show real-time follow mode -->
      <div class="global-tabs-bar">
        <div class="global-tab active">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <circle cx="8" cy="8" r="3" stroke="currentColor" stroke-width="1.2" fill="none"/>
            <path d="M8 1v2M8 13v2M1 8h2M13 8h2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
          </svg>
          <span class="tab-title">实时跟随</span>
        </div>
        <button class="global-tab-add" @click="emit('new-editor-tab')" title="新建空白 Tab">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
            <path d="M12 5v14M5 12h14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </div>

    <div class="navbar-right">
      <button
        class="nav-icon-btn"
        :class="{ active: props.showChat }"
        @click="emit('toggle-chat')"
        title="Toggle Chat"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <path d="M2 2.5h12v9H5.5L3 14V2.5z" stroke="currentColor" stroke-width="1.3" fill="none"/>
        </svg>
      </button>

      <button class="nav-btn" @click="emit('toggle-chat')" v-if="!props.showChat">+ New</button>
      <button class="nav-btn">Export</button>

      <div class="nav-sep" />

      <button
        class="nav-icon-btn"
        :class="{ active: props.showTerminal }"
        @click="emit('toggle-terminal')"
        title="Toggle Terminal"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <path d="M2 4l3 2.5L2 9" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M7 10h5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>

      <span class="status-dot" :class="props.agentStatus" />
      <span class="status-text">{{ props.agentStatus }}</span>

      <div class="nav-sep" />

      <!-- Window controls -->
      <button class="window-btn" @click="minimize()" title="Minimize">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none">
          <path d="M6 12h12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </button>
      <button class="window-btn" @click="maximize()" title="Maximize">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none">
          <rect x="6" y="6" width="12" height="12" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/>
        </svg>
      </button>
      <button class="window-btn close-btn" @click="closeWindow()" title="Close">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none">
          <path d="M18 6L6 18M6 6l12 12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.navbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 44px;
  padding: 0 8px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
  user-select: none;
  -webkit-app-region: drag;
  position: relative;
}

.navbar-left,
.navbar-right {
  display: flex;
  align-items: center;
  gap: 4px;
  -webkit-app-region: no-drag;
}

.navbar-center {
  -webkit-app-region: no-drag;
  flex: 1;
  display: flex;
  justify-content: center;
  overflow: hidden;
}

.logo {
  font-weight: 700;
  font-size: var(--fs-base);
  color: var(--text-primary);
  display: flex;
  align-items: center;
  gap: 5px;
  margin-left: 4px;
}

.logo-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  background: var(--brand);
  color: #fff;
  border-radius: 6px;
  font-size: var(--fs-sm);
  font-weight: 700;
}

.version {
  font-size: 10px;
  color: rgba(148, 153, 166, 0.5);
  font-weight: 400;
  margin-left: 3px;
  letter-spacing: 0.2px;
}

.nav-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 28px;
  background: transparent;
  border: none;
  border-radius: 6px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease, transform 100ms ease;
}

.nav-icon-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
  transform: scale(1.05);
}

.nav-icon-btn:active {
  transform: scale(0.96);
}

.nav-icon-btn.active {
  color: var(--brand);
  background: var(--brand-soft);
}

.theme-btn {
  color: var(--text-muted);
}

.theme-btn:hover {
  color: var(--brand);
}

.nav-btn {
  padding: 5px 12px;
  border: 1px solid var(--border-default);
  background: transparent;
  color: var(--text-secondary);
  border-radius: 6px;
  cursor: pointer;
  font-size: var(--fs-sm);
  font-weight: 500;
  font-family: "Inter", "PingFang SC", "Microsoft YaHei", sans-serif;
  transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease, transform 100ms ease;
}

.nav-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
  border-color: var(--border-hover);
}

.nav-btn:active {
  transform: scale(0.96);
}

.nav-sep {
  width: 1px;
  height: 18px;
  background: var(--border-default);
  margin: 0 6px;
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--warn);
  flex-shrink: 0;
  margin-left: 4px;
}

.status-dot.ready {
  background: var(--success);
}

.status-dot.error {
  background: var(--error);
}

.status-text {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  font-weight: 500;
  margin-left: 5px;
  letter-spacing: 0.2px;
}

/* Window controls */
.window-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: transparent;
  border: none;
  border-radius: 6px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease, transform 100ms ease;
}

.window-btn:hover {
  background: var(--bg-hover);
}

.window-btn:active {
  transform: scale(0.92);
}

.window-btn.close-btn:hover {
  background: var(--error);
  color: #fff;
}

/* Center global tabs */
.global-tabs-bar {
  display: flex;
  gap: 2px;
  align-items: center;
  padding: 0 8px;
}

.global-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 16px;
  font-size: var(--fs-sm);
  color: var(--text-muted);
  background: transparent;
  border: none;
  border-bottom: 2px solid transparent;
  cursor: pointer;
  white-space: nowrap;
  transition: background-color 120ms ease, color 120ms ease;
}

.global-tab.active {
  color: var(--text-primary);
  border-bottom-color: var(--brand);
  background: var(--bg-root);
}

.global-tab:hover:not(.active) {
  background: var(--bg-hover);
  color: var(--text-secondary);
}

.global-tab .tab-close {
  font-size: 12px;
  padding: 2px 4px;
  border-radius: 4px;
  color: var(--text-muted);
  opacity: 0;
  transition: opacity 100ms ease, color 100ms ease, background-color 100ms ease;
}

.global-tab:hover .tab-close {
  opacity: 1;
}

.global-tab .tab-close:hover {
  color: var(--error);
  background: var(--bg-active);
}

.global-tab-add {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 6px;
  color: var(--text-muted);
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease, transform 120ms ease;
}

.global-tab-add:hover {
  color: var(--brand);
  background: var(--brand-soft);
  transform: translateY(-1px);
}

.global-tab-add:active {
  transform: translateY(0) scale(0.95);
}
</style>
