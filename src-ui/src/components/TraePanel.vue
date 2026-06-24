<script setup lang="ts">
import { ref } from "vue";

const expandedSections = ref({
  explorer: true,
  rustDeps: false,
});

function toggleSection(section: string) {
  expandedSections.value[section as keyof typeof expandedSections.value] = 
    !expandedSections.value[section as keyof typeof expandedSections.value];
}
</script>

<template>
  <div class="trae-panel">
    <!-- Top toolbar - compact icon bar -->
    <div class="trae-toolbar">
      <button class="tb-btn active" title="资源管理器">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <rect x="1" y="2" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <rect x="1" y="9" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <rect x="9" y="2" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
          <rect x="9" y="9" width="6" height="5" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
        </svg>
      </button>
      <button class="tb-btn" title="搜索">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
          <path d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
      <button class="tb-btn" title="运行和调试">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <circle cx="8" cy="8" r="3" fill="currentColor"/>
          <path d="M8 1v2M8 13v2M1 8h2M13 8h2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        <span class="badge">3</span>
      </button>
      <span class="toolbar-spacer" />
      <button class="tb-btn collapse-btn" title="折叠面板">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <path d="M10 3L5 8l5 5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>
    </div>

    <!-- Content area -->
    <div class="trae-content">
      <!-- Explorer section -->
      <div class="panel-section">
        <button 
          class="section-header" 
          @click="toggleSection('explorer')"
        >
          <svg 
            class="expand-icon" 
            width="12" 
            height="12" 
            viewBox="0 0 24 24" 
            fill="none"
            :class="{ rotated: expandedSections.explorer }"
          >
            <path d="M6 9l6 6 6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <span class="section-title">资源管理器</span>
        </button>
        
        <div v-show="expandedSections.explorer" class="section-content">
          <!-- Explorer tree items -->
          <div class="tree-item">
            <svg class="tree-expand" width="12" height="12" viewBox="0 0 24 24" fill="none">
              <path d="M6 9l6 6 6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
            <svg class="tree-icon" width="14" height="14" viewBox="0 0 16 16" fill="none">
              <rect x="2" y="2" width="12" height="12" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/>
              <path d="M5 5h6M5 8h6M5 11h3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
            <span class="tree-label">文件</span>
          </div>
          
          <div class="tree-item indent-1">
            <svg class="tree-expand" width="12" height="12" viewBox="0 0 24 24" fill="none">
              <path d="M6 9l6 6 6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
            <svg class="tree-icon" width="14" height="14" viewBox="0 0 16 16" fill="none">
              <rect x="2" y="3" width="12" height="10" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/>
              <path d="M2 6h12" stroke="currentColor" stroke-width="1.2"/>
            </svg>
            <span class="tree-label">大纲</span>
          </div>
          
          <div class="tree-item indent-1">
            <svg class="tree-expand" width="12" height="12" viewBox="0 0 24 24" fill="none">
              <path d="M6 9l6 6 6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
            <svg class="tree-icon" width="14" height="14" viewBox="0 0 16 16" fill="none">
              <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.2" fill="none"/>
              <path d="M8 4v4h4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
            <span class="tree-label">时间线</span>
          </div>
          
          <!-- Cue-Pro section -->
          <div class="cue-pro-section">
            <div class="cue-pro-header">
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                <path d="M2 8a6 6 0 1 1 12 0" stroke="currentColor" stroke-width="1.3" fill="none"/>
                <path d="M6 8h4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
              </svg>
              <span class="cue-pro-title">Cue-Pro</span>
            </div>
            <div class="cue-pro-empty">
              暂无编辑建议，请先进行编辑操作…
            </div>
            <div class="cue-pro-status">
              <span class="status-icon">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none">
                  <path d="M20 6L9 17l-5-5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
              </span>
              <span class="status-text">已处理 0/0 个变更点</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Rust Dependencies section -->
      <div class="panel-section">
        <button 
          class="section-header" 
          @click="toggleSection('rustDeps')"
        >
          <svg 
            class="expand-icon" 
            width="12" 
            height="12" 
            viewBox="0 0 24 24" 
            fill="none"
            :class="{ rotated: expandedSections.rustDeps }"
          >
            <path d="M6 9l6 6 6-6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <span class="section-title">Rust Dependencies</span>
        </button>
        
        <div v-show="expandedSections.rustDeps" class="section-content">
          <div class="deps-empty">
            暂无依赖项
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.trae-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-panel);
  overflow: hidden;
}

/* ── Toolbar ── */
.trae-toolbar {
  display: flex;
  align-items: center;
  padding: 4px 4px;
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
  gap: 2px;
}

.toolbar-spacer {
  flex: 1;
}

.tb-btn {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: var(--text-muted);
  cursor: pointer;
}

.tb-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.tb-btn.active {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.badge {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  font-size: 10px;
  font-weight: 600;
  color: #fff;
  background: var(--brand);
  border-radius: 7px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.collapse-btn {
  color: var(--text-muted);
}

.collapse-btn:hover {
  color: var(--text-primary);
}

/* ── Content area ── */
.trae-content {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

.trae-content::-webkit-scrollbar {
  width: 4px;
}

.trae-content::-webkit-scrollbar-track {
  background: transparent;
}

.trae-content::-webkit-scrollbar-thumb {
  background: transparent;
  border-radius: 2px;
}

.trae-content:hover::-webkit-scrollbar-thumb {
  background: var(--scroll-thumb);
}

/* ── Panel section ── */
.panel-section {
  border-bottom: 1px solid var(--border-light);
}

.section-header {
  display: flex;
  align-items: center;
  gap: 4px;
  width: 100%;
  padding: 6px 8px;
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-secondary);
  letter-spacing: var(--ls-wide);
}

.section-header:hover {
  background: var(--bg-hover);
}

.expand-icon {
  transition: transform 150ms ease;
  flex-shrink: 0;
  color: var(--text-muted);
}

.expand-icon.rotated {
  transform: rotate(90deg);
}

.section-title {
  flex: 1;
  text-align: left;
}

.section-content {
  padding: 2px 0;
  animation: slideDown 150ms ease;
}

@keyframes slideDown {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* ── Tree items ── */
.tree-item {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px 3px 12px;
  cursor: pointer;
  color: var(--text-secondary);
}

.tree-item.indent-1 {
  padding-left: 24px;
}

.tree-item:hover {
  background: var(--bg-hover);
}

.tree-expand {
  color: var(--text-muted);
  flex-shrink: 0;
}

.tree-icon {
  opacity: 0.6;
  flex-shrink: 0;
}

.tree-label {
  font-size: var(--fs-xs);
  flex: 1;
}

/* ── Cue-Pro section ── */
.cue-pro-section {
  margin: 4px 8px;
  padding: 8px;
  background: var(--bg-hover);
  border-radius: var(--radius-sm);
}

.cue-pro-header {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-bottom: 6px;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-primary);
}

.cue-pro-empty {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  padding: 8px 0;
  text-align: center;
}

.cue-pro-status {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding-top: 6px;
  border-top: 1px solid var(--border-light);
}

.status-icon {
  color: var(--success);
}

.status-text {
  font-size: var(--fs-xs);
  color: var(--success);
  font-weight: 500;
}

/* ── Dependencies section ── */
.deps-empty {
  padding: 12px 8px;
  font-size: var(--fs-xs);
  color: var(--text-muted);
  text-align: center;
}
</style>