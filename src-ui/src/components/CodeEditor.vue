<script setup lang="ts">
import { ref, shallowRef, onMounted, onBeforeUnmount, watch, onUnmounted } from "vue";
import * as monaco from "monaco-editor";
import LogTerminal from "./LogTerminal.vue";

// Tauri invoke wrapper - only available in Tauri environment
async function tauriInvoke<T>(command: string, args?: Record<string, any>): Promise<T> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke(command, args);
  } catch (e) {
    console.warn(`Tauri invoke not available: ${command}`, e);
    throw e;
  }
}

interface Tab {
  id: string;
  title: string;
  path: string;
  content: string;
  language: string;
  dirty: boolean;
}

const tabs = ref<Tab[]>([]);
const activeTabId = ref<string | null>(null);
const editorContainer = ref<HTMLElement | null>(null);
let editor: monaco.editor.IStandaloneCodeEditor | null = null;
let dirtyListener: monaco.IDisposable | null = null;

// --- Console tabs ---
const consoleTabs = ["问题", "输出", "终端", "调试控制台"] as const;
type ConsoleTab = (typeof consoleTabs)[number];
const activeConsoleTab = ref<ConsoleTab>("问题");
const consoleHeight = ref(200);
const isDraggingConsole = ref(false);

// --- Mock problems data ---
const problems = ref([
  {
    id: 1,
    file: "lib.rs",
    path: "src-agent/src",
    line: 37,
    column: 7,
    severity: "warn",
    message: "expected values for `feature` are: 'default', 'keyring', and ...",
    detail: "consider adding `'gui'` as a feature in `Cargo.toml`\nsee <https://doc.rust-lang.org/nightly/rustc/check-cfg/cargo...>\n#[warn(unexpected_cfgs)] on by default",
  },
  {
    id: 2,
    file: "lib.rs",
    path: "src-agent/src",
    line: 40,
    column: 7,
    severity: "warn",
    message: "expected values for `feature` are: 'default', 'keyring', and ...",
    detail: "consider adding `'gui'` as a feature in `Cargo.toml`\nsee <https://doc.rust-lang.org/nightly/rustc/check-cfg/cargo...>\n#[warn(unexpected_cfgs)] on by default",
  },
]);

defineExpose({ openFile, tabs, activeTabId, newBlankTab, closeTab, switchTab });

const LANG_MAP: Record<string, string> = {
  rs: "rust", ts: "typescript", tsx: "typescriptreact", js: "javascript",
  jsx: "javascriptreact", vue: "html", html: "html", css: "css",
  scss: "scss", less: "less", json: "json", toml: "toml",
  md: "markdown", yaml: "yaml", yml: "yaml", py: "python", go: "go",
  java: "java", c: "c", cpp: "cpp", h: "c", hpp: "cpp",
  xml: "xml", sql: "sql", sh: "shell", bash: "shell", zsh: "shell",
  dockerfile: "dockerfile", gitignore: "plaintext", env: "plaintext",
  lock: "plaintext", svg: "xml",
};

function detectLanguage(name: string): string {
  const lower = name.toLowerCase();
  if (lower === "dockerfile") return "dockerfile";
  if (lower === "makefile") return "makefile";
  const ext = name.includes(".") ? name.split(".").pop()?.toLowerCase() || "" : "";
  return LANG_MAP[ext] || "plaintext";
}

async function openFile(path: string, name: string) {
  const existing = tabs.value.find((t) => t.path === path);
  if (existing) { activeTabId.value = existing.id; return; }
  try {
    const content = await tauriInvoke<string>("file_read_large", { filePath: path, offset: 0, limit: 0 });
    tabs.value.push({ id: `tab-${Date.now()}`, title: name, path, content, language: detectLanguage(name), dirty: false });
    activeTabId.value = tabs.value[tabs.value.length - 1].id;
  } catch (e) { console.error("Failed to open file:", e); }
}

function newBlankTab() {
  const tab: Tab = {
    id: `tab-${Date.now()}`,
    title: "Untitled",
    path: "",
    content: "",
    language: "plaintext",
    dirty: false,
  };
  tabs.value.push(tab);
  activeTabId.value = tab.id;
}

function switchTab(tabId: string) {
  activeTabId.value = tabId;
}

async function saveFile() {
  const tab = tabs.value.find((t) => t.id === activeTabId.value);
  if (!tab || !editor) return;
  const content = editor.getValue();
  try {
    await tauriInvoke("file_write", { req: { path: tab.path, content } });
    tab.content = content;
    tab.dirty = false;
  } catch (e) { console.error("Failed to save:", e); }
}

function closeTab(tabId: string) {
  const idx = tabs.value.findIndex((t) => t.id === tabId);
  if (idx === -1) return;
  const tab = tabs.value[idx];
  if (tab.dirty) { if (!confirm(`"${tab.title}" has unsaved changes. Close anyway?`)) return; }
  tabs.value.splice(idx, 1);
  if (activeTabId.value === tabId) { activeTabId.value = tabs.value[Math.min(idx, tabs.value.length - 1)]?.id || null; }
}

const activeTab = () => tabs.value.find((t) => t.id === activeTabId.value);

// detect current theme
function getMonacoTheme() {
  const theme = document.documentElement.getAttribute("data-theme");
  return theme === "light" ? "vs" : "vs-dark";
}

function updateMonacoTheme() {
  editor?.updateOptions({ theme: getMonacoTheme() });
}

// --- Console resize ---
function startConsoleDrag(e: MouseEvent) {
  e.preventDefault();
  isDraggingConsole.value = true;
  const startY = e.clientY;
  const startH = consoleHeight.value;
  const handleMove = (ev: MouseEvent) => {
    const delta = startY - ev.clientY;
    consoleHeight.value = Math.max(80, Math.min(500, startH + delta));
  };
  const handleUp = () => {
    isDraggingConsole.value = false;
    document.removeEventListener("mousemove", handleMove);
    document.removeEventListener("mouseup", handleUp);
  };
  document.addEventListener("mousemove", handleMove);
  document.addEventListener("mouseup", handleUp);
}

// --- Problem filter ---
const filterText = ref("");

const filteredProblems = ref(problems.value);

watch(filterText, (val) => {
  if (!val.trim()) {
    filteredProblems.value = problems.value;
  } else {
    const q = val.toLowerCase();
    filteredProblems.value = problems.value.filter(p => 
      p.file.toLowerCase().includes(q) ||
      p.message.toLowerCase().includes(q)
    );
  }
});

onMounted(() => {
  if (!editorContainer.value) return;

  editor = monaco.editor.create(editorContainer.value, {
    theme: getMonacoTheme(),
    automaticLayout: true,
    minimap: { enabled: true, scale: 1, showSlider: "mouseover" },
    fontSize: 14,
    lineHeight: 22,
    fontFamily: '"JetBrains Mono", "SF Mono", "Cascadia Code", monospace',
    lineNumbers: "on",
    scrollBeyondLastLine: false,
    wordWrap: "on",
    tabSize: 2,
    insertSpaces: true,
    renderWhitespace: "selection",
    bracketPairColorization: { enabled: true },
    guides: { bracketPairs: true, indentation: true },
    smoothScrolling: true,
    cursorBlinking: "smooth",
    cursorSmoothCaretAnimation: "on",
    padding: { top: 8 },
  });

  dirtyListener = editor.onDidChangeModelContent(() => {
    const tab = tabs.value.find((t) => t.id === activeTabId.value);
    if (tab) tab.dirty = true;
  });

  editor.addAction({ id: "save-file", label: "Save File",
    keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS], run: () => { saveFile(); } });
  editor.addAction({ id: "close-tab", label: "Close Tab",
    keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyW],
    run: () => { if (activeTabId.value) closeTab(activeTabId.value); } });

  const observer = new MutationObserver(updateMonacoTheme);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  onUnmounted(() => observer.disconnect());
});

onBeforeUnmount(() => {
  dirtyListener?.dispose();
  editor?.dispose();
});

watch(activeTabId, async () => {
  const tab = activeTab();
  if (!editor || !tab) return;
  dirtyListener?.dispose();
  
  let model = editor.getModel();
  if (!model) {
    // Create new model if none exists
    model = monaco.editor.createModel(tab.content, tab.language);
    editor.setModel(model);
  } else {
    // Update existing model
    monaco.editor.setModelLanguage(model, tab.language);
    model.setValue(tab.content);
  }
  
  dirtyListener = editor.onDidChangeModelContent(() => {
    const t = tabs.value.find(x => x.id === activeTabId.value);
    if (t) t.dirty = true;
  });
});
</script>

<template>
  <div class="code-editor">
    <!-- Tab bar -->
    <div class="tabs-bar">
      <div
        v-for="tab in tabs"
        :key="tab.id"
        class="tab"
        :class="{ active: tab.id === activeTabId }"
        @click="activeTabId = tab.id"
        @mousedown.middle.prevent="closeTab(tab.id)"
      >
        <span class="tab-icon" :class="{ dirty: tab.dirty }">{{ tab.dirty ? "●" : "" }}</span>
        <span class="tab-title">{{ tab.title }}</span>
        <span class="tab-close" @click.stop="closeTab(tab.id)">&#215;</span>
      </div>
    </div>

    <!-- Monaco editor / Empty placeholder -->
    <div v-if="tabs.length === 0" class="editor-placeholder">
      <div class="placeholder-content">
        <div class="placeholder-icon">
          <svg width="48" height="48" viewBox="0 0 24 24" fill="none">
            <rect x="3" y="3" width="18" height="18" rx="2" stroke="currentColor" stroke-width="1.5" fill="none"/>
            <path d="M6 8h12M6 12h12M6 16h6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
        </div>
        <p class="placeholder-text">一切就绪，只等你动手</p>
        <p class="placeholder-hint">拖拽文件到此打开，或点击左侧文件树</p>
      </div>
    </div>
    <div v-else ref="editorContainer" class="editor-area" />

    <!-- Console divider -->
    <div class="console-divider" @mousedown="startConsoleDrag">
      <div class="divider-line" />
    </div>

    <!-- Bottom console -->
    <div class="console-panel" :style="{ height: consoleHeight + 'px' }">
      <div class="console-tabs">
        <button
          v-for="ct in consoleTabs" :key="ct"
          class="console-tab"
          :class="{ active: activeConsoleTab === ct }"
          @click="activeConsoleTab = ct"
        >{{ ct }}</button>
      </div>
      <div class="console-body">
        <!-- Problems tab -->
        <div v-if="activeConsoleTab === '问题'" class="problems-panel">
          <!-- Filter bar -->
          <div class="filter-bar">
            <button class="filter-btn active">全部</button>
            <button class="filter-btn">错误</button>
            <button class="filter-btn">警告</button>
            <span class="flex-spacer" />
            <div class="filter-input-wrapper">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none">
                <path d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
              <input 
                v-model="filterText" 
                class="filter-input" 
                placeholder="筛选器(例如 text、**/*.ts、!**/node_modules/**)"
              />
              <button class="filter-clear" v-if="filterText">&#215;</button>
            </div>
            <button class="filter-action">全部添加到对话</button>
          </div>
          
          <!-- Problems list -->
          <div class="problems-list">
            <div 
              v-for="problem in filteredProblems" 
              :key="problem.id" 
              class="problem-item"
            >
              <div class="problem-header">
                <span class="problem-severity" :class="problem.severity">
                  <svg v-if="problem.severity === 'warn'" width="12" height="12" viewBox="0 0 24 24" fill="none">
                    <path d="M12 2L1 21h22L12 2z" stroke="currentColor" stroke-width="1.5" fill="none"/>
                    <path d="M12 9v5M12 17h.01" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                  </svg>
                  <svg v-else width="12" height="12" viewBox="0 0 24 24" fill="none">
                    <circle cx="12" cy="12" r="10" stroke="currentColor" stroke-width="1.5"/>
                    <path d="M15 9l-6 6M9 9l6 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
                  </svg>
                </span>
                <span class="problem-file">{{ problem.file }}:{{ problem.line }}:{{ problem.column }}</span>
                <span class="problem-path">{{ problem.path }}</span>
              </div>
              <div class="problem-message">{{ problem.message }}</div>
              <div class="problem-detail">{{ problem.detail }}</div>
            </div>
            
            <div v-if="filteredProblems.length === 0" class="problems-empty">
              当前工作区无检测到问题
            </div>
          </div>
        </div>
        
        <!-- Output tab -->
        <div v-else-if="activeConsoleTab === '输出'" class="console-placeholder">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
            <rect x="3" y="5" width="18" height="14" rx="2" stroke="var(--text-muted)" stroke-width="1.5" />
            <path d="M8 10l3 3-3 3M13 15h3" stroke="var(--text-muted)" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          <span>暂无输出内容</span>
        </div>
        
        <!-- Terminal tab -->
        <div v-else-if="activeConsoleTab === '终端'" class="terminal-wrapper">
          <LogTerminal :embedded="true" />
        </div>
        
        <!-- Debug Console tab -->
        <div v-else class="console-placeholder">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none">
            <circle cx="12" cy="12" r="10" stroke="var(--text-muted)" stroke-width="1.5" />
            <rect x="8" y="8" width="8" height="8" rx="1" stroke="var(--text-muted)" stroke-width="1.5" />
            <circle cx="12" cy="12" r="2" fill="var(--text-muted)" />
          </svg>
          <span>调试控制台 — 等待调试会话启动</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.code-editor {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* ── Tab bar ── */
.tabs-bar {
  display: flex;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-default);
  overflow-x: auto;
  flex-shrink: 0;
  position: relative;
}
.tabs-bar::-webkit-scrollbar { height: 0; }

.tab-scroll-btn {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 24px;
  background: var(--bg-panel);
  border: none;
  border-left: 1px solid var(--border-default);
  color: var(--text-muted);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1;
  transition: background-color 120ms ease, color 120ms ease;
}

.tab-scroll-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.tab-scroll-btn.left {
  left: 0;
  border-left: none;
  border-right: 1px solid var(--border-default);
}

.tab-scroll-btn.right {
  right: 0;
}

.tab {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 7px 16px;
  font-size: var(--fs-sm);
  color: var(--text-muted);
  border-right: 1px solid var(--border-light);
  cursor: pointer;
  white-space: nowrap;
  min-width: 0;
  background: transparent;
  border-bottom: 2px solid transparent;
  border-top: none;
  border-left: none;
  transition: background-color 120ms ease, color 120ms ease;
}
.tab.active {
  background: var(--bg-root);
  color: var(--text-primary);
  border-bottom: 2px solid var(--brand);
}
.tab:hover:not(.active) { 
  background: var(--bg-hover); 
  color: var(--text-secondary);
}
.tab-icon { font-size: 9px; color: transparent; min-width: 8px; flex-shrink: 0; }
.tab-icon.dirty { color: var(--brand); }
.tab-title { overflow: hidden; text-overflow: ellipsis; }
.tab-close {
  font-size: 12px; 
  color: var(--text-muted); 
  padding: 2px 4px;
  border-radius: 4px; 
  flex-shrink: 0; 
  line-height: 1;
  opacity: 0;
  transition: opacity 100ms ease, color 100ms ease, background-color 100ms ease;
}
.tab:hover .tab-close {
  opacity: 1;
}
.tab-close:hover { 
  color: var(--error); 
  background: var(--bg-active); 
}

/* ── Editor placeholder ── */
.editor-placeholder {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-root);
  position: relative;
}

.editor-placeholder::after {
  content: "";
  position: absolute;
  inset: 24px;
  border: 2px dashed var(--border-default);
  border-radius: 12px;
  pointer-events: none;
  opacity: 0;
  transition: opacity 200ms ease, border-color 200ms ease;
}

.editor-placeholder.drag-over::after {
  opacity: 1;
  border-color: var(--brand);
  background: var(--brand-soft);
}

.placeholder-content {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--text-muted);
  transition: opacity 200ms ease;
}

.editor-placeholder.drag-over .placeholder-content {
  opacity: 0.7;
}

.placeholder-icon {
  opacity: 0.4;
  transform: scale(0.85);
}

.placeholder-text {
  font-size: var(--fs-sm);
  font-weight: 500;
  margin: 0;
  color: var(--text-muted);
}

.placeholder-hint {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  opacity: 0;
  transform: translateY(-8px);
  transition: opacity 200ms ease, transform 200ms ease;
}

.editor-placeholder:hover .placeholder-hint,
.editor-placeholder.drag-over .placeholder-hint {
  opacity: 1;
  transform: translateY(0);
}

/* ── Editor ── */
.editor-area { flex: 1; overflow: hidden; min-height: 80px; }

/* ── Console divider ── */
.console-divider {
  width: 100%;
  height: 6px;
  cursor: row-resize;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  z-index: 2;
  transition: background-color 120ms ease;
}
.console-divider:hover {
  background: var(--bg-hover);
}
.console-divider:hover .divider-line { 
  background: var(--brand); 
  width: 60px;
}
.divider-line {
  width: 40px;
  height: 3px;
  border-radius: 2px;
  background: var(--border-default);
  transition: background-color 120ms ease, width 150ms ease;
}

/* ── Console panel ── */
.console-panel {
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  border-top: 1px solid var(--border-default);
  background: var(--bg-panel);
  overflow: hidden;
}

.console-tabs {
  display: flex;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
  padding: 0 6px;
  gap: 0;
}

.console-tab {
  padding: 6px 14px;
  font-size: var(--fs-xs);
  font-weight: 500;
  color: var(--text-muted);
  background: transparent;
  border: none;
  border-bottom: 2px solid transparent;
  cursor: pointer;
  letter-spacing: var(--ls-wide);
  border-radius: 4px 4px 0 0;
  transition: background-color 120ms ease, color 120ms ease;
}
.console-tab:hover { 
  color: var(--text-secondary); 
  background: var(--bg-hover); 
}
.console-tab.active {
  color: var(--brand);
  background: var(--bg-root);
  border-bottom-color: var(--brand);
}

.console-body {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.terminal-wrapper {
  flex: 1;
  overflow: hidden;
}

/* ── Console placeholders ── */
.console-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  flex: 1;
  color: var(--text-muted);
  font-size: var(--fs-sm);
  font-family: "Inter", "PingFang SC", "Microsoft YaHei", sans-serif;
}

/* ── Problems panel ── */
.problems-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.filter-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
}

.filter-btn {
  padding: 2px 8px;
  font-size: var(--fs-xs);
  font-weight: 500;
  color: var(--text-muted);
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
}

.filter-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.filter-btn.active {
  color: #fff;
  background: var(--brand);
}

.flex-spacer {
  flex: 1;
}

.filter-input-wrapper {
  display: flex;
  align-items: center;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: 6px;
  padding: 4px 10px;
  transition: border-color 120ms ease, box-shadow 120ms ease;
}

.filter-input-wrapper:focus-within {
  border-color: var(--brand);
  box-shadow: 0 0 0 2px var(--brand-soft);
}

.filter-search-icon {
  color: var(--text-muted);
  font-size: 12px;
  margin-right: 8px;
  flex-shrink: 0;
}

.filter-input {
  border: none;
  background: transparent;
  color: var(--text-primary);
  font-size: var(--fs-xs);
  font-family: "JetBrains Mono", "SF Mono", monospace;
  outline: none;
  width: 180px;
}

.filter-input::placeholder {
  color: var(--text-muted);
}

.filter-clear {
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 12px;
  padding: 2px 5px;
  border-radius: 4px;
  transition: color 120ms ease, background-color 120ms ease;
}

.filter-clear:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.filter-action {
  padding: 2px 8px;
  font-size: var(--fs-xs);
  font-weight: 500;
  color: var(--brand);
  background: var(--brand-soft);
  border: none;
  border-radius: 4px;
  cursor: pointer;
  margin-left: 4px;
}

.filter-action:hover {
  background: var(--brand);
  color: #fff;
}

.problems-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px 8px;
  white-space: nowrap;
}

.problems-list::-webkit-scrollbar {
  width: 4px;
}

.problems-list::-webkit-scrollbar-track {
  background: transparent;
}

.problems-list::-webkit-scrollbar-thumb {
  background: var(--scroll-thumb);
  border-radius: 2px;
}

.problem-item {
  padding: 5px 8px;
  border-radius: 4px;
  margin-bottom: 2px;
  cursor: pointer;
  transition: background-color 120ms ease;
  position: relative;
  overflow: hidden;
}

.problem-item::before {
  content: "";
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 2px;
  background: transparent;
  transition: background-color 120ms ease;
}

.problem-item:hover {
  background: var(--bg-hover);
}

.problem-item.warn::before {
  background: var(--warn);
}

.problem-item.error::before {
  background: var(--error);
}

.problem-item.info::before {
  background: var(--info);
}

.problem-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 2px;
}

.problem-severity {
  flex-shrink: 0;
  font-size: 10px;
  font-weight: 600;
  padding: 1px 4px;
  border-radius: 3px;
  text-transform: uppercase;
}

.problem-severity.info {
  color: var(--info);
  background: rgba(59, 130, 246, 0.1);
}

.problem-severity.warn {
  color: var(--warn);
  background: rgba(234, 179, 8, 0.1);
}

.problem-severity.error {
  color: var(--error);
  background: rgba(239, 68, 68, 0.1);
}

.problem-file {
  font-family: "JetBrains Mono", "SF Mono", monospace;
  font-size: var(--fs-xs);
  color: var(--text-primary);
  font-weight: 500;
}

.problem-path {
  font-size: var(--fs-xs);
  color: var(--text-muted);
}

.problem-message {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  margin-bottom: 2px;
}

.problem-detail {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  font-family: "JetBrains Mono", "SF Mono", monospace;
  white-space: pre-wrap;
}

.problems-empty {
  padding: 16px;
  text-align: center;
  color: var(--text-muted);
  font-size: var(--fs-xs);
}
</style>
