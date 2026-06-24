<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from "vue";

type UnlistenFn = () => void;

interface LogEntry {
  id: number;
  time: string;
  level: "info" | "warn" | "error";
  category: string;
  message: string;
  detail: string;
}

const logs = ref<LogEntry[]>([]);
let seq = 0;
const MAX_LOGS = 800;

const levelFilter = ref<"all" | LogEntry["level"]>("all");
const categoryFilter = ref<string | null>(null);
const searchText = ref("");
const collapsed = ref(false);
const expandedId = ref<number | null>(null);
const userScrolling = ref(false);

const logContainer = ref<HTMLElement | null>(null);

let unlistenLog: UnlistenFn | null = null;
let unlistenAgent: UnlistenFn | null = null;

defineProps<{ embedded?: boolean }>();
defineEmits<{ toggle: [] }>();

function addLog(level: LogEntry["level"], category: string, message: string, detail: string) {
  const now = new Date().toLocaleTimeString("en-US", { hour12: false, fractionalSecondDigits: 3 });
  logs.value.push({ id: ++seq, time: now, level, category, message, detail });
  if (logs.value.length > MAX_LOGS) logs.value.splice(0, logs.value.length - MAX_LOGS);
  scrollToBottom();
}

async function scrollToBottom() {
  await nextTick();
  if (logContainer.value && !userScrolling.value) {
    logContainer.value.scrollTop = logContainer.value.scrollHeight;
  }
}

function onUserScroll() {
  if (!logContainer.value) return;
  const el = logContainer.value;
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
  userScrolling.value = !atBottom;
}

onMounted(async () => {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    unlistenLog = await listen("log-event", (e: any) => {
      const p = e.payload;
      addLog(p.level || "info", p.category || "system", p.message || "", p.detail || "");
    });

    unlistenAgent = await listen("agent-event", (e: any) => {
      const p = e.payload;
      if (p.event === "agent_initialized") addLog("info", "agent", "Agent engine ready", "");
      else if (p.event === "agent_init_failed") addLog("error", "agent", `Agent init failed: ${p.error}`, "");
      else addLog("info", "agent", `Agent event: ${p.event || JSON.stringify(p)}`, "");
    });
  } catch {}

  addLog("info", "system", "Log terminal ready", "");
});

onUnmounted(() => { unlistenLog?.(); unlistenAgent?.(); });

const ALL_CATEGORIES = ["llm", "tool", "file", "agent", "plan", "system"];

const filteredLogs = computed(() => {
  let result = logs.value;
  if (levelFilter.value !== "all") result = result.filter((l) => l.level === levelFilter.value);
  if (categoryFilter.value) result = result.filter((l) => l.category === categoryFilter.value);
  const q = searchText.value.trim().toLowerCase();
  if (q) result = result.filter((l) => l.message.toLowerCase().includes(q) || l.detail.toLowerCase().includes(q) || l.category.toLowerCase().includes(q));
  return result;
});

const filteredCount = computed(() => filteredLogs.value.length);
const totalCount = computed(() => logs.value.length);

function toggleCategory(cat: string) {
  categoryFilter.value = categoryFilter.value === cat ? null : cat;
}

function copyAllFiltered() {
  const text = filteredLogs.value
    .map((l) => `[${l.time}] [${l.level.toUpperCase()}] [${l.category}] ${l.message}${l.detail ? " | " + l.detail : ""}`)
    .join("\n");
  navigator.clipboard.writeText(text);
}

function copySingle(id: number) {
  const l = logs.value.find((x) => x.id === id);
  if (!l) return;
  const text = `[${l.time}] [${l.level.toUpperCase()}] [${l.category}] ${l.message}${l.detail ? " | " + l.detail : ""}`;
  navigator.clipboard.writeText(text);
}

function toggleExpand(id: number) {
  expandedId.value = expandedId.value === id ? null : id;
}

function hasDetail(entry: LogEntry): boolean { return entry.detail.length > 0; }

function formatDetail(detail: string): string {
  try { const obj = JSON.parse(detail); return JSON.stringify(obj, null, 2); }
  catch { return detail; }
}

function clearLogs() { logs.value = []; expandedId.value = null; }
</script>

<template>
  <div class="log-terminal">
    <div class="log-header">
      <span class="log-title" @click="collapsed = !collapsed" title="Toggle log panel">
        <span class="collapse-arrow">{{ collapsed ? "▸" : "▾" }}</span>
        LOG
        <span v-if="totalCount > 0" class="log-badge">{{ filteredCount }}/{{ totalCount }}</span>
      </span>

      <div v-if="!collapsed" class="category-chips">
        <button
          v-for="cat in ALL_CATEGORIES" :key="cat"
          class="cat-chip" :class="{ active: categoryFilter === cat }"
          @click="toggleCategory(cat)"
        >{{ cat.toUpperCase() }}</button>
      </div>

      <span class="flex-spacer" />

      <div v-if="!collapsed" class="log-actions">
        <select v-model="levelFilter" class="filter-select">
          <option value="all">ALL</option>
          <option value="info">INFO</option>
          <option value="warn">WARN</option>
          <option value="error">ERROR</option>
        </select>
        <button class="action-btn" @click="copyAllFiltered" title="Copy filtered" :disabled="filteredCount === 0">Copy</button>
        <button class="action-btn" @click="clearLogs" title="Clear all" :disabled="totalCount === 0">Clear</button>
        <button v-if="!$props.embedded" class="action-btn" @click="$emit('toggle')" title="Close terminal">&#215;</button>
      </div>
    </div>

    <div v-if="!collapsed" class="log-search">
      <input v-model="searchText" class="search-input" placeholder="Filter logs..." spellcheck="false" />
    </div>

    <div v-show="!collapsed" ref="logContainer" class="log-body" @scroll="onUserScroll">
      <div v-if="userScrolling" class="scroll-paused" @click="userScrolling = false; scrollToBottom()">
        Auto-scroll paused &mdash; click to resume
      </div>

      <template v-for="log in filteredLogs" :key="log.id">
        <div class="log-line" :class="{ 'has-detail': hasDetail(log) }">
          <span class="log-time">{{ log.time }}</span>
          <span class="log-level" :class="'lvl-' + log.level">{{ log.level.toUpperCase() }}</span>
          <span class="log-category">{{ log.category }}</span>
          <span class="log-msg">{{ log.message }}</span>
          <span class="flex-spacer" />
          <button v-if="hasDetail(log)" class="log-line-btn" :class="{ active: expandedId === log.id }" @click="toggleExpand(log.id)">{{ expandedId === log.id ? "▲" : "▼" }}</button>
          <button class="log-line-btn" @click="copySingle(log.id)" title="Copy">⎘</button>
        </div>
        <div v-if="expandedId === log.id && hasDetail(log)" class="log-detail">
          <pre class="detail-pre">{{ formatDetail(log.detail) }}</pre>
        </div>
      </template>

      <div v-if="filteredLogs.length === 0 && logs.length === 0" class="log-empty">No log entries — waiting for events...</div>
      <div v-else-if="filteredLogs.length === 0" class="log-empty">No matching log entries</div>
    </div>
  </div>
</template>

<style scoped>
.log-terminal {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-panel);
  font-family: "JetBrains Mono", "SF Mono", "Cascadia Code", "Source Code Pro", "Consolas", monospace;
}

/* ── Header ── */
.log-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
  min-height: 28px;
}

.log-title {
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: var(--ls-caps);
  cursor: pointer;
  user-select: none;
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}
.collapse-arrow { display: inline-block; width: 10px; color: var(--text-muted); }
.log-badge { font-size: 9px; color: var(--brand); }

.category-chips { display: flex; gap: 4px; }
.cat-chip {
  padding: 1px 7px;
  border: 1px solid var(--border-default);
  border-radius: 3px;
  font-size: var(--fs-xs);
  font-weight: 600;
  cursor: pointer;
  color: var(--text-muted);
  background: var(--bg-hover);
  font-family: "JetBrains Mono", "SF Mono", "Cascadia Code", monospace;
  letter-spacing: var(--ls-wide);
}
.cat-chip:hover { color: var(--text-primary); border-color: var(--border-hover); }
.cat-chip.active { color: #fff; border-color: var(--brand); background: var(--brand); }

.flex-spacer { flex: 1; }

.log-actions { display: flex; gap: 4px; align-items: center; flex-shrink: 0; }

.filter-select {
  padding: 1px 6px;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  color: var(--text-secondary);
  border-radius: 3px;
  font-size: var(--fs-xs);
  font-family: "JetBrains Mono", "SF Mono", "Cascadia Code", monospace;
  outline: none;
  cursor: pointer;
}

.action-btn {
  padding: 1px 7px;
  background: transparent;
  border: 1px solid transparent;
  color: var(--text-muted);
  border-radius: 3px;
  cursor: pointer;
  font-size: var(--fs-xs);
  font-family: "Inter", "PingFang SC", "Microsoft YaHei", sans-serif;
}
.action-btn:hover:not(:disabled) { background: var(--bg-hover); color: var(--text-primary); }
.action-btn:disabled { opacity: 0.3; cursor: default; }

/* ── Search ── */
.log-search { padding: 3px 8px; flex-shrink: 0; }
.search-input {
  width: 100%;
  padding: 3px 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: 4px;
  color: var(--text-primary);
  font-size: var(--fs-xs);
  font-family: "JetBrains Mono", "SF Mono", "Cascadia Code", monospace;
  outline: none;
}
.search-input:focus { border-color: var(--brand); }
.search-input::placeholder { color: var(--text-muted); }

/* ── Log body ── */
.log-body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 4px 0;
  font-size: var(--fs-xs);
  line-height: 1.75;
}

.scroll-paused {
  position: sticky;
  top: 0;
  padding: 3px 10px;
  background: var(--brand-soft);
  color: var(--brand);
  font-size: var(--fs-xs);
  cursor: pointer;
  text-align: center;
  z-index: 5;
}
.scroll-paused:hover { background: var(--brand); color: #fff; }

/* ── Log line ── */
.log-line {
  display: flex;
  align-items: baseline;
  gap: 6px;
  padding: 0 8px;
  min-height: 20px;
  white-space: nowrap;
}
.log-line:hover { background: var(--bg-hover); }
.log-line.has-detail { cursor: pointer; }

.log-time { color: var(--text-muted); flex-shrink: 0; font-size: 9px; }

.log-level {
  display: inline-block;
  padding: 0 4px;
  border-radius: 2px;
  font-size: 8px;
  font-weight: 700;
  letter-spacing: 0.4px;
  flex-shrink: 0;
  line-height: 1.6;
}
.lvl-info { background: var(--info-bg); color: var(--info); }
.lvl-warn { background: var(--warn-bg); color: var(--warn); }
.lvl-error { background: var(--error-bg); color: var(--error); }

.log-category {
  display: inline-block;
  padding: 0 4px;
  border-radius: 2px;
  font-size: 8px;
  font-weight: 600;
  letter-spacing: 0.3px;
  flex-shrink: 0;
  line-height: 1.6;
  color: var(--text-muted);
  border: 1px solid var(--border-default);
}

.log-msg {
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
}

.log-line-btn {
  padding: 0 4px;
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 9px;
  flex-shrink: 0;
  opacity: 0;
}
.log-line:hover .log-line-btn { opacity: 1; }
.log-line-btn:hover { color: var(--text-primary); }
.log-line-btn.active { opacity: 1; color: var(--brand); }

/* ── Detail ── */
.log-detail {
  padding: 4px 8px 6px 100px;
  border-bottom: 1px solid var(--border-default);
  background: var(--bg-hover);
}

.detail-pre {
  margin: 0;
  font-family: "JetBrains Mono", "SF Mono", Monaco, monospace;
  font-size: 10px;
  line-height: 1.5;
  color: var(--text-secondary);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 200px;
  overflow-y: auto;
}

.log-empty {
  color: var(--text-muted);
  font-style: italic;
  padding: 12px 10px;
  text-align: center;
  font-size: 10px;
}
</style>
