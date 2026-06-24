<script setup lang="ts">
import { useChatStore } from "../stores/chat";
import { useAppStore } from "../stores/app";

const chatStore = useChatStore();
const app = useAppStore();

function formatDate(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString("en-US", { hour12: false, hour: "2-digit", minute: "2-digit" });
  }
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

function openSession(id: string) {
  chatStore.switchSession(id);
  app.navigate("chat");
}

function getPreview(session: typeof chatStore.sessions extends (infer U)[] ? U : never): string {
  if (!session) return "";
  const msgs = session.messages || [];
  if (msgs.length === 0) return "No messages yet";
  return msgs[msgs.length - 1]?.content?.slice(0, 120) || "";
}
</script>

<template>
  <div class="sessions-view">
    <div class="sessions-scroll">
      <div class="sessions-header">
        <h1 class="sessions-title">History</h1>
        <button class="new-btn" @click="chatStore.newSession(); app.navigate('chat')">
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path d="M7 1v12M1 7h12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          New Chat
        </button>
      </div>

      <div v-if="chatStore.sessions.length === 0" class="empty-state">
        <div class="empty-icon">
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none">
            <rect x="3" y="4" width="18" height="16" rx="2" stroke="currentColor" stroke-width="1.5" fill="none"/>
            <path d="M3 10h18" stroke="currentColor" stroke-width="1.5"/>
          </svg>
        </div>
        <h3 class="empty-title">No conversations yet</h3>
        <p class="empty-desc">Start a chat and it will appear here</p>
      </div>

      <div v-else class="session-list">
        <div
          v-for="s in chatStore.sessions"
          :key="s.id"
          class="session-card"
          :class="{ active: s.id === chatStore.activeSessionId }"
          @click="openSession(s.id)"
        >
          <div class="sc-icon">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <path d="M3 2.5h10v9H8l-3 2.5V11.5H3V2.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" fill="none"/>
            </svg>
          </div>
          <div class="sc-content">
            <div class="sc-name">{{ s.name }}</div>
            <div class="sc-preview">{{ getPreview(s as any) }}</div>
          </div>
          <div class="sc-meta">
            <span class="sc-date">{{ formatDate(s.updatedAt) }}</span>
            <span class="sc-count">{{ s.messages.length }} msgs</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.sessions-view {
  height: 100%;
  overflow-y: auto;
}

.sessions-scroll {
  max-width: 600px;
  margin: 0 auto;
  padding: var(--space-8) var(--space-6);
}

.sessions-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-6);
}

.sessions-title {
  font-size: var(--fs-lg);
  font-weight: 700;
  color: var(--text-primary);
}

.new-btn {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: var(--brand-500);
  color: #fff;
  font-size: var(--fs-sm);
  font-weight: 600;
  border-radius: var(--radius-xs);
  transition: background var(--duration-fast) var(--ease-out);
}

.new-btn:hover {
  background: var(--brand-600);
}

/* ── Empty state ── */
.empty-state {
  text-align: center;
  padding: var(--space-12) 0;
}

.empty-icon {
  width: 48px;
  height: 48px;
  margin: 0 auto var(--space-4);
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-surface);
  border-radius: var(--radius-md);
  color: var(--text-tertiary);
}

.empty-title {
  font-size: var(--fs-base);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: var(--space-1);
}

.empty-desc {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
}

/* ── Session list ── */
.session-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.session-card {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-4);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
}

.session-card:hover {
  border-color: var(--border-strong);
  background: var(--bg-hover);
}

.session-card.active {
  border-color: var(--brand-500);
  background: var(--info-bg);
}

.sc-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-elevated);
  border-radius: var(--radius-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.sc-content {
  flex: 1;
  min-width: 0;
}

.sc-name {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 2px;
}

.sc-preview {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sc-meta {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  flex-shrink: 0;
}

.sc-date {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
}

.sc-count {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  opacity: 0.7;
}
</style>
