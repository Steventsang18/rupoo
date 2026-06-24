<script setup lang="ts">
import { ref } from "vue";
import { useAppStore } from "../stores/app";

const app = useAppStore();

// Mock agent state — in production this would come from Tauri IPC events
const currentPlan = ref<{
  name: string;
  status: string;
  steps: { name: string; status: string }[];
} | null>(null);

const recentToolCalls = ref([
  { tool: "web_search", params: "Rust async best practices", time: "2m ago", status: "done" },
  { tool: "file_read", params: "src/main.rs", time: "5m ago", status: "done" },
  { tool: "shell_exec", params: "cargo test", time: "8m ago", status: "done" },
]);

const memoryCount = ref(42);
const skillCount = ref(7);
</script>

<template>
  <div class="agent-view">
    <div class="agent-scroll">
      <h1 class="agent-title">Agent</h1>
      <p class="agent-desc">Monitor your AI agent's activity, plans, and capabilities</p>

      <!-- Status Cards -->
      <div class="agent-stats">
        <div class="stat-card">
          <div class="stat-value" :class="app.agentStatus === 'ready' ? 'text-success' : 'text-warn'">
            {{ app.agentStatus === 'ready' ? 'Online' : app.agentStatus }}
          </div>
          <div class="stat-label">Status</div>
        </div>
        <div class="stat-card">
          <div class="stat-value">{{ memoryCount }}</div>
          <div class="stat-label">Memory Entries</div>
        </div>
        <div class="stat-card">
          <div class="stat-value">{{ skillCount }}</div>
          <div class="stat-label">Skills Loaded</div>
        </div>
        <div class="stat-card">
          <div class="stat-value">{{ app.tokenUsage.total.toLocaleString() }}</div>
          <div class="stat-label">Total Tokens</div>
        </div>
      </div>

      <!-- Active Plan -->
      <section class="agent-section">
        <h2 class="section-title">Active Plan</h2>
        <div v-if="currentPlan" class="plan-card">
          <div class="plan-header">
            <span class="plan-name">{{ currentPlan.name }}</span>
            <span class="plan-badge" :class="currentPlan.status">{{ currentPlan.status }}</span>
          </div>
          <div class="plan-steps">
            <div v-for="(step, i) in currentPlan.steps" :key="i" class="plan-step" :class="step.status">
              <div class="step-indicator">
                <div v-if="step.status === 'done'" class="step-check">
                  <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
                    <path d="M3 6l2 2 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                  </svg>
                </div>
                <div v-else-if="step.status === 'running'" class="step-running" />
                <div v-else class="step-pending" />
              </div>
              <span class="step-text">{{ step.name }}</span>
            </div>
          </div>
        </div>
        <div v-else class="empty-card">
          <p>No active plan. Start a chat to begin.</p>
        </div>
      </section>

      <!-- Recent Tool Calls -->
      <section class="agent-section">
        <h2 class="section-title">Recent Tool Calls</h2>
        <div class="tool-list">
          <div v-for="(call, i) in recentToolCalls" :key="i" class="tool-item">
            <div class="tool-icon">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" v-if="call.status === 'error'"/>
                <path d="M4 8l3 3 5-5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" v-else/>
              </svg>
            </div>
            <div class="tool-info">
              <div class="tool-name">{{ call.tool }}</div>
              <div class="tool-params">{{ call.params }}</div>
            </div>
            <div class="tool-time">{{ call.time }}</div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.agent-view {
  height: 100%;
  overflow-y: auto;
}

.agent-scroll {
  max-width: 600px;
  margin: 0 auto;
  padding: var(--space-8) var(--space-6);
}

.agent-title {
  font-size: var(--fs-lg);
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: var(--space-1);
}

.agent-desc {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  margin-bottom: var(--space-8);
}

/* ── Stats ── */
.agent-stats {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-3);
  margin-bottom: var(--space-6);
}

.stat-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  padding: var(--space-4);
  text-align: center;
}

.stat-value {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--text-primary);
  font-family: var(--font-mono);
}

.text-success { color: var(--success); }
.text-warn { color: var(--warning); }

.stat-label {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  margin-top: var(--space-1);
  text-transform: uppercase;
  letter-spacing: var(--ls-caps);
  font-weight: 500;
}

/* ── Section ── */
.agent-section {
  margin-bottom: var(--space-6);
}

.section-title {
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: var(--ls-caps);
  margin-bottom: var(--space-3);
}

/* ── Plan ── */
.plan-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.plan-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  background: var(--bg-elevated);
  border-bottom: 1px solid var(--border-default);
}

.plan-name {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
}

.plan-badge {
  padding: 2px 8px;
  font-size: var(--fs-2xs);
  font-weight: 600;
  border-radius: var(--radius-full);
  text-transform: capitalize;
}

.plan-badge.running {
  background: var(--info-bg);
  color: var(--info);
}

.plan-badge.done {
  background: var(--success-bg);
  color: var(--success);
}

.plan-steps {
  padding: var(--space-3);
}

.plan-step {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) 0;
}

.step-indicator {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.step-check {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--success-bg);
  color: var(--success);
  display: flex;
  align-items: center;
  justify-content: center;
}

.step-running {
  width: 12px;
  height: 12px;
  border: 2px solid var(--brand-500);
  border-top-color: transparent;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.step-pending {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--border-strong);
}

.step-text {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

.empty-card {
  padding: var(--space-6);
  text-align: center;
  background: var(--bg-surface);
  border: 1px dashed var(--border-default);
  border-radius: var(--radius-sm);
  color: var(--text-tertiary);
  font-size: var(--fs-sm);
}

/* ── Tool calls ── */
.tool-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.tool-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
}

.tool-icon {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-elevated);
  border-radius: var(--radius-2xs);
  color: var(--success);
  flex-shrink: 0;
}

.tool-info {
  flex: 1;
  min-width: 0;
}

.tool-name {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  font-family: var(--font-mono);
}

.tool-params {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tool-time {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}
</style>
