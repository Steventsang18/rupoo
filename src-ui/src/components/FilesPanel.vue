<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../stores/app";

const app = useAppStore();

interface FileItem {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileItem[];
}

const tree = ref<FileItem[]>([]);
const collapsedPaths = ref<Set<string>>(new Set());
const selectedPath = ref<string | null>(null);
const loading = ref(true);

onMounted(async () => {
  await loadTree();
  loading.value = false;
});

async function loadTree(dir?: string) {
  try {
    tree.value = await invoke<FileItem[]>("file_read_tree", { dir: dir || "." });
  } catch {
    tree.value = [];
  }
}

function toggleCollapse(path: string) {
  const p = collapsedPaths.value;
  if (p.has(path)) p.delete(path);
  else p.add(path);
}

function isCollapsed(path: string): boolean {
  return collapsedPaths.value.has(path);
}

function handleClick(item: FileItem) {
  selectedPath.value = item.path;
  if (item.is_dir) {
    toggleCollapse(item.path);
  }
}

function getFileIcon(item: FileItem): string {
  if (item.is_dir) {
    return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none"><rect x="2" y="2.5" width="10" height="9" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/></svg>`;
  }
  return `<svg width="14" height="14" viewBox="0 0 14 14" fill="none"><rect x="2" y="2" width="10" height="10" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/></svg>`;
}
</script>

<template>
  <div class="files-view">
    <div class="files-header">
      <h3 class="files-title">Files</h3>
      <button class="files-refresh" @click="loadTree()" title="Refresh">
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <path d="M1 3v4h4M13 11V7H9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M2 11a6 6 0 015.2-9 6 6 0 014.8 2" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
    <div class="files-tree">
      <div v-if="loading" class="files-loading">Loading...</div>
      <div v-else-if="tree.length === 0" class="files-empty">No files found</div>

      <!-- Recursive tree rendering -->
      <template v-for="item in tree" :key="item.path">
        <div
          class="tree-node"
          :class="{ selected: selectedPath === item.path }"
          :style="{ paddingLeft: '12px' }"
          @click="handleClick(item)"
        >
          <span class="tree-chevron" :class="{ expanded: item.is_dir && !isCollapsed(item.path) }">
            <svg v-if="item.is_dir" width="10" height="10" viewBox="0 0 10 10" fill="none">
              <path d="M3 2l4 3-4 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </span>
          <span class="tree-icon" v-html="getFileIcon(item)" />
          <span class="tree-label">{{ item.name }}</span>
        </div>
        <!-- Recursively render children -->
        <template v-if="item.is_dir && !isCollapsed(item.path) && item.children">
          <div v-for="child in item.children" :key="child.path">
            <div
              class="tree-node"
              :class="{ selected: selectedPath === child.path }"
              :style="{ paddingLeft: '28px' }"
              @click="handleClick(child)"
            >
              <span class="tree-chevron" :class="{ expanded: child.is_dir && !isCollapsed(child.path) }">
                <svg v-if="child.is_dir" width="10" height="10" viewBox="0 0 10 10" fill="none">
                  <path d="M3 2l4 3-4 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
              </span>
              <span class="tree-icon" v-html="getFileIcon(child)" />
              <span class="tree-label">{{ child.name }}</span>
            </div>
          </div>
        </template>
      </template>
    </div>
  </div>
</template>

<style scoped>
.files-view {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.files-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
}

.files-title {
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: var(--ls-caps);
}

.files-refresh {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-2xs);
  color: var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-out);
}

.files-refresh:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

.files-tree {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-1) 0;
}

.files-loading,
.files-empty {
  padding: var(--space-6);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--fs-sm);
}

.tree-node {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 3px 12px;
  cursor: pointer;
  color: var(--text-secondary);
  font-size: var(--fs-sm);
  transition: all var(--duration-fast) var(--ease-out);
}

.tree-node:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.tree-node.selected {
  background: var(--info-bg);
  color: var(--text-primary);
}

.tree-chevron {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  color: var(--text-tertiary);
  transition: transform var(--duration-fast) var(--ease-out);
}

.tree-chevron.expanded {
  transform: rotate(90deg);
}

.tree-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  flex-shrink: 0;
}

.tree-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
