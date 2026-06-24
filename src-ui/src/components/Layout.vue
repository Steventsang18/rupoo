<script setup lang="ts">
import { computed } from "vue";
import { useAppStore } from "../stores/app";
import ActivityBar from "./ActivityBar.vue";
import StatusBar from "./StatusBar.vue";
import WelcomePage from "./WelcomePage.vue";
import ChatPanel from "./ChatPanel.vue";
import SettingsPanel from "./SettingsPanel.vue";
import AgentPanel from "./AgentPanel.vue";
import SessionsPanel from "./SessionsPanel.vue";
import FilesPanel from "./FilesPanel.vue";

const app = useAppStore();

const activeComponent = computed(() => {
  switch (app.activeTab) {
    case "chat":     return ChatPanel;
    case "agent":    return AgentPanel;
    case "files":    return FilesPanel;
    case "sessions": return SessionsPanel;
    case "settings": return SettingsPanel;
    default:         return WelcomePage;
  }
});

const isReady = computed(() => app.agentStatus === "ready");
</script>

<template>
  <div class="layout">
    <!-- Title Bar -->
    <div class="titlebar" data-tauri-drag-region>
      <div class="titlebar-drag">
        <div class="titlebar-brand">
          <div class="titlebar-logo">R</div>
          <span class="titlebar-name">rupoo</span>
          <span class="titlebar-version">v{{ app.version }}</span>
        </div>
      </div>
    </div>

    <div class="layout-body">
      <!-- Activity Bar -->
      <ActivityBar />

      <!-- Main Content -->
      <main class="main-content">
        <Transition name="fade" mode="out-in">
          <component :is="activeComponent" :key="app.activeTab" />
        </Transition>
      </main>
    </div>

    <!-- Status Bar -->
    <StatusBar />
  </div>
</template>

<style scoped>
.layout {
  display: flex;
  flex-direction: column;
  width: 100vw;
  height: 100vh;
  background: var(--bg-root);
  overflow: hidden;
}

/* ── Title Bar ── */
.titlebar {
  height: var(--titlebar-height);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  background: var(--bg-raised);
  border-bottom: 1px solid var(--border-default);
  /* macOS traffic light integration */
  padding-left: 76px;  /* Leave space for traffic lights */
}

.titlebar-drag {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
}

.titlebar-brand {
  display: flex;
  align-items: center;
  gap: 6px;
}

.titlebar-logo {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-gradient);
  border-radius: var(--radius-2xs);
  color: #fff;
  font-size: 12px;
  font-weight: 700;
}

.titlebar-name {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: var(--ls-tight);
}

.titlebar-version {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  font-weight: 500;
}

/* ── Body ── */
.layout-body {
  flex: 1;
  display: flex;
  overflow: hidden;
  min-height: 0;
}

/* ── Main Content ── */
.main-content {
  flex: 1;
  overflow: hidden;
  min-width: 0;
  position: relative;
  background: var(--bg-root);
}
</style>
