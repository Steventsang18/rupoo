<script setup lang="ts">
import { computed } from "vue";
import { useAppStore, type TabId } from "../stores/app";

const app = useAppStore();

interface TabItem {
  id: TabId;
  icon: string;       // SVG path
  label: string;
  badge?: number | string;
}

const tabs: TabItem[] = [
  {
    id: "chat",
    icon: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none"><path d="M3 2.5h14v11H7.5L5 16V13.5H3V2.5z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/><path d="M7 7h6M7 10h4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>`,
    label: "Chat",
  },
  {
    id: "agent",
    icon: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="7" r="3.5" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M10 12c-3.3 0-6 1.3-6 3v1.5h12V15c0-1.7-2.7-3-6-3z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/><circle cx="17" cy="4" r="2" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M14 6l1.5 1.5M16 2l1.5-1.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>`,
    label: "Agent",
  },
  {
    id: "files",
    icon: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none"><rect x="2.5" y="3" width="6" height="5.5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><rect x="2.5" y="11.5" width="6" height="5.5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><rect x="11.5" y="3" width="6" height="5.5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/><rect x="11.5" y="11.5" width="6" height="5.5" rx="1" stroke="currentColor" stroke-width="1.5" fill="none"/></svg>`,
    label: "Files",
  },
  {
    id: "sessions",
    icon: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none"><path d="M2 4h16v10.5H10l-3 3V14.5H2V4z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/><path d="M6 8h8M6 11h5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>`,
    label: "History",
  },
  {
    id: "settings",
    icon: `<svg width="20" height="20" viewBox="0 0 20 20" fill="none"><circle cx="10" cy="10" r="2.5" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M10 1.5v2M10 16.5v2M1.5 10h2M16.5 10h2M4 4l1.5 1.5M14.5 14.5l1.5 1.5M4 16l1.5-1.5M14.5 5.5l1.5-1.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`,
    label: "Settings",
  },
];

const agentDotColor = computed(() => {
  switch (app.agentStatus) {
    case "ready":        return "var(--success)";
    case "error":        return "var(--error)";
    case "initializing": return "var(--warning)";
    default:             return "var(--text-tertiary)";
  }
});
</script>

<template>
  <nav class="activity-bar">
    <div class="ab-top">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="ab-item"
        :class="{ active: app.activeTab === tab.id }"
        @click="app.navigate(tab.id)"
        :title="tab.label"
      >
        <div class="ab-icon" v-html="tab.icon" />
        <div v-if="tab.badge" class="ab-badge">{{ tab.badge }}</div>
      </button>
    </div>

    <div class="ab-bottom">
      <div class="ab-divider" />
      <button
        class="ab-item theme-toggle"
        @click="app.toggleTheme"
        :title="app.settings.theme === 'dark' ? 'Light mode' : 'Dark mode'"
      >
        <svg v-if="app.settings.theme === 'dark'" width="18" height="18" viewBox="0 0 20 20" fill="none">
          <circle cx="10" cy="10" r="3.5" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <path d="M10 1.5v2M10 16.5v2M1.5 10h2M16.5 10h2M4 4l1.5 1.5M14.5 14.5l1.5 1.5M4 16l1.5-1.5M14.5 5.5l1.5-1.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
        <svg v-else width="18" height="18" viewBox="0 0 20 20" fill="none">
          <path d="M7.5 3a6.5 6.5 0 1 0 7 7 6 6 0 0 1-7-7z" stroke="currentColor" stroke-width="1.3" fill="none"/>
        </svg>
      </button>
      <div class="ab-agent-indicator" :style="{ backgroundColor: agentDotColor }" title="Agent status" />
    </div>
  </nav>
</template>

<style scoped>
.activity-bar {
  width: var(--activity-bar-width);
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  background: var(--bg-raised);
  border-right: 1px solid var(--border-default);
  padding: var(--space-2) 0;
  z-index: 10;
}

.ab-top {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.ab-item {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm);
  color: var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-out);
  cursor: pointer;
}

.ab-item:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.ab-item.active {
  color: var(--brand-500);
  background: var(--info-bg);
}

.ab-item.active::before {
  content: '';
  position: absolute;
  left: -8px;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 20px;
  background: var(--brand-500);
  border-radius: 0 2px 2px 0;
}

.ab-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
}

.ab-badge {
  position: absolute;
  top: 4px;
  right: 4px;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  font-size: 10px;
  font-weight: 700;
  color: #fff;
  background: var(--brand-500);
  border-radius: var(--radius-full);
  display: flex;
  align-items: center;
  justify-content: center;
}

.ab-bottom {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
}

.ab-divider {
  width: 24px;
  height: 1px;
  background: var(--border-default);
}

.theme-toggle {
  width: 36px;
  height: 36px;
}

.ab-agent-indicator {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  transition: background-color var(--duration-normal) var(--ease-out);
  box-shadow: 0 0 6px var(--bg-elevated);
}
</style>
