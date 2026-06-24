<script setup lang="ts">
import { ref, onMounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import TreeNodeItem, { type TreeNode } from "./TreeNodeItem.vue";

const tree = ref<TreeNode[]>([]);
const collapsed = ref<Set<string>>(new Set());
const contextMenu = ref<{ x: number; y: number; node: TreeNode } | null>(null);
const renaming = ref<{ node: TreeNode; name: string } | null>(null);
const creating = ref<{ parentPath: string; isDir: boolean; name: string } | null>(null);
const selectedPath = ref<string | null>(null);

const emit = defineEmits<{ "file-select": [node: TreeNode] }>();

onMounted(async () => {
  await loadTree();
  document.addEventListener("click", closeContextMenu);
});

async function loadTree(dir: string = ".") {
  try {
    tree.value = await invoke<TreeNode[]>("file_read_tree", { dir });
  } catch (e) {
    console.error("Failed to load file tree:", e);
  }
}

defineExpose({ loadTree });

function toggleCollapse(nodePath: string) {
  if (collapsed.value.has(nodePath)) collapsed.value.delete(nodePath);
  else collapsed.value.add(nodePath);
}

function handleClick(node: TreeNode) {
  selectedPath.value = node.path;
  if (node.is_dir) {
    toggleCollapse(node.path);
  } else {
    emit("file-select", node);
  }
}

function openContextMenu(e: MouseEvent, node: TreeNode) {
  e.preventDefault();
  e.stopPropagation();
  contextMenu.value = { x: e.clientX, y: e.clientY, node };
}

function closeContextMenu() { contextMenu.value = null; }

function parentPathOf(node: TreeNode): string {
  if (node.is_dir) return node.path;
  const idx = node.path.lastIndexOf("/");
  return idx > 0 ? node.path.substring(0, idx) : ".";
}

async function doCreateFile() {
  if (!contextMenu.value) return;
  const p = parentPathOf(contextMenu.value.node);
  creating.value = { parentPath: p, isDir: false, name: "" };
  collapsed.value.delete(contextMenu.value.node.is_dir ? contextMenu.value.node.path : p);
  contextMenu.value = null;
  await nextTick();
  focusInlineInput();
}

async function doCreateFolder() {
  if (!contextMenu.value) return;
  const p = parentPathOf(contextMenu.value.node);
  creating.value = { parentPath: p, isDir: true, name: "" };
  collapsed.value.delete(contextMenu.value.node.is_dir ? contextMenu.value.node.path : p);
  contextMenu.value = null;
  await nextTick();
  focusInlineInput();
}

async function doDelete() {
  if (!contextMenu.value) return;
  const node = contextMenu.value.node;
  contextMenu.value = null;
  try {
    await invoke("file_delete", { req: { path: node.path, isDir: node.is_dir } });
    await loadTree();
  } catch (e) { console.error("Delete failed:", e); }
}

function doRename() {
  if (!contextMenu.value) return;
  const node = contextMenu.value.node;
  renaming.value = { node, name: node.name };
  contextMenu.value = null;
  nextTick(() => {
    const input = document.querySelector<HTMLInputElement>(".tree-body input.inline-input");
    input?.focus(); input?.select();
  });
}

function focusInlineInput() {
  const input = document.querySelector<HTMLInputElement>(".tree-body input.inline-input");
  input?.focus();
}

async function submitCreate() {
  if (!creating.value || !creating.value.name.trim()) { creating.value = null; return; }
  try {
    await invoke("file_create", {
      req: { parentDir: creating.value.parentPath, name: creating.value.name.trim(), isDir: creating.value.isDir },
    });
    await loadTree();
  } catch (e) { console.error("Create failed:", e); }
  creating.value = null;
}

async function submitRename() {
  if (!renaming.value || !renaming.value.name.trim()) { renaming.value = null; return; }
  try {
    await invoke("file_rename", {
      req: { oldPath: renaming.value.node.path, newName: renaming.value.name.trim() },
    });
    await loadTree();
  } catch (e) { console.error("Rename failed:", e); }
  renaming.value = null;
}

function handleInlineKeydown(e: KeyboardEvent) {
  if (e.key === "Enter") { creating.value ? submitCreate() : submitRename(); }
  else if (e.key === "Escape") { creating.value = null; renaming.value = null; }
}

function handleInlineBlur() {
  creating.value ? submitCreate() : submitRename();
}
</script>

<template>
  <div class="file-tree">
    <div class="tree-header">资源管理器</div>
    <div class="tree-body" @click="closeContextMenu">
      <TreeNodeItem
        v-for="node in tree"
        :key="node.path"
        :node="node"
        :depth="0"
        :collapsed="collapsed"
        :selected-path="selectedPath"
        :renaming="renaming"
        :creating="creating"
        @click="handleClick"
        @contextmenu="openContextMenu"
        @inline-keydown="handleInlineKeydown"
        @inline-blur="handleInlineBlur"
      />
    </div>

    <Teleport to="body">
      <div
        v-if="contextMenu"
        class="ctx-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
      >
        <div class="ctx-item" @click="doCreateFile">New File</div>
        <div class="ctx-item" @click="doCreateFolder">New Folder</div>
        <div class="ctx-sep" />
        <div class="ctx-item" @click="doRename">Rename</div>
        <div class="ctx-item ctx-danger" @click="doDelete">Delete</div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.file-tree {
  height: 100%;
  display: flex;
  flex-direction: column;
  user-select: none;
}

.tree-header {
  padding: 12px 12px 10px;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: var(--ls-caps);
  flex-shrink: 0;
}

.tree-body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 4px 0 8px;
}

.tree-body::-webkit-scrollbar {
  width: 5px;
}

.tree-body::-webkit-scrollbar-track {
  background: transparent;
}

.tree-body::-webkit-scrollbar-thumb {
  background: var(--scroll-thumb);
  border-radius: 3px;
  opacity: 0;
  transition: opacity 150ms ease;
}

.tree-body:hover::-webkit-scrollbar-thumb {
  opacity: 1;
}

.tree-body::-webkit-scrollbar-thumb:hover {
  background: var(--scroll-thumb-hover);
}

.ctx-menu {
  position: fixed;
  z-index: 9999;
  background: var(--bg-panel);
  border: 1px solid var(--border-default);
  border-radius: 8px;
  padding: 4px 0;
  min-width: 140px;
  box-shadow: var(--shadow-lg);
}

.ctx-item {
  padding: 6px 14px;
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  cursor: pointer;
}

.ctx-item:hover {
  background: var(--brand);
  color: #fff;
}

.ctx-sep {
  height: 1px;
  background: var(--border-default);
  margin: 4px 0;
}

.ctx-danger:hover {
  background: var(--error);
  color: #fff;
}
</style>
