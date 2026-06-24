<script setup lang="ts">
import { computed } from "vue";
import { useAppStore } from "../stores/app";

const app = useAppStore();

const statusText = computed(() => {
  switch (app.agentStatus) {
    case "ready":        return "Ready";
    case "error":        return "Error";
    case "initializing": return "Starting...";
    default:             return "Offline";
  }
});

const statusColor = computed(() => {
  switch (app.agentStatus) {
    case "ready":        return "var(--success)";
    case "error":        return "var(--error)";
    case "initializing": return "var(--warning)";
    default:             return "var(--text-tertiary)";
  }
});
</script>

<template>
  <footer class="status-bar">
    <!-- Left: Agent status -->
    <div class="sb-left">
      <span class="sb-dot" :style="{ backgroundColor: statusColor }" />
      <span class="sb-label">{{ statusText }}</span>
    </div>

    <!-- Center: model info -->
    <div class="sb-center">
      <span v-if="app.modelInfo" class="sb-item">{{ app.modelInfo }}</span>
      <span v-else class="sb-item sb-dim">{{ app.settings.provider }} / {{ app.settings.model }}</span>
    </div>

    <!-- Right: token usage + misc -->
    <div class="sb-right">
      <span v-if="app.tokenUsage.total > 0" class="sb-item sb-mono">
        {{ app.tokenUsage.total.toLocaleString() }} tokens
      </span>
      <button class="sb-action" @click="app.navigate('settings')" title="Settings">
        <svg width="14" height="14" viewBox="0 0 20 20" fill="none">
          <circle cx="10" cy="10" r="2.5" stroke="currentColor" stroke-width="1.5" fill="none"/>
          <path d="M10 1.5v2M10 16.5v2M1.5 10h2M16.5 10h2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
  </footer>
</template>

<style scoped>
.status-bar {
  height: var(--statusbar-height);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  padding: 0 var(--space-3);
  background: var(--bg-raised);
  border-top: 1px solid var(--border-default);
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  gap: var(--space-2);
  z-index: 10;
}

.sb-left {
  display: flex;
  align-items: center;
  gap: 5px;
}

.sb-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  transition: background-color var(--duration-normal) var(--ease-out);
}

.sb-label {
  font-weight: 500;
}

.sb-center {
  flex: 1;
  display: flex;
  justify-content: center;
}

.sb-right {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.sb-item {
  padding: 2px 6px;
  border-radius: var(--radius-2xs);
}

.sb-item:hover {
  background: var(--bg-hover);
  color: var(--text-secondary);
}

.sb-dim {
  opacity: 0.6;
}

.sb-mono {
  font-family: var(--font-mono);
  font-size: 10px;
}

.sb-action {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-2xs);
  color: var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-out);
}

.sb-action:hover {
  background: var(--bg-hover);
  color: var(--text-secondary);
}
</style>
