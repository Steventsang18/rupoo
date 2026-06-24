<script lang="ts">
import { defineComponent, type PropType } from "vue";

export interface TreeNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: TreeNode[];
}

export default defineComponent({
  name: "TreeNodeItem",
  props: {
    node: { type: Object as PropType<TreeNode>, required: true },
    depth: { type: Number, required: true },
    collapsed: { type: Object as PropType<Set<string>>, required: true },
    selectedPath: { type: String as PropType<string | null>, default: null },
    renaming: { type: Object as PropType<{ node: TreeNode; name: string } | null>, default: null },
    creating: { type: Object as PropType<{ parentPath: string; isDir: boolean; name: string } | null>, default: null },
  },
  emits: ["click", "contextmenu", "inline-keydown", "inline-blur"],
  setup(props, { emit }) {
    const isCollapsed = () => props.collapsed.has(props.node.path);
    const isSelected = () => props.selectedPath === props.node.path;
    const indent = (d: number) => `${d * 16 + 8}px`;

    function fileExt(): string {
      if (props.node.is_dir) return "dir";
      const dot = props.node.name.lastIndexOf(".");
      if (dot === -1) return "file";
      return props.node.name.substring(dot + 1).toLowerCase();
    }

    function onClick() { emit("click", props.node); }
    function onDoubleClick() {
      if (props.node.is_dir) {
        props.collapsed.has(props.node.path) 
          ? props.collapsed.delete(props.node.path)
          : props.collapsed.add(props.node.path);
      } else {
        emit("click", props.node);
      }
    }
    function onContextMenu(e: MouseEvent) { emit("contextmenu", e, props.node); }

    return { isCollapsed, isSelected, indent, fileExt, onClick, onDoubleClick, onContextMenu };
  },
});
</script>

<template>
  <div :data-path="node.path">
    <div
      v-if="!renaming || renaming.node.path !== node.path"
      class="tree-item"
      :class="{ selected: isSelected(), 'is-dir': node.is_dir }"
      :style="{ paddingLeft: indent(depth) }"
      @click="onClick"
      @dblclick="onDoubleClick"
      @contextmenu="onContextMenu"
    >
      <span v-if="node.is_dir" class="arrow" :class="{ expanded: !isCollapsed() }">
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
          <path d="M4 6l4 4 4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </span>
      <span v-else class="arrow-spacer" />
      <span class="icon">
        <!-- Folder icons -->
        <template v-if="node.is_dir">
          <svg v-if="isCollapsed()" width="15" height="15" viewBox="0 0 16 16" fill="none">
            <path d="M1.5 4h5l2 2h6v7H1.5V4z" fill="var(--brand-soft)" stroke="var(--text-muted)" stroke-width="1.1"/>
          </svg>
          <svg v-else width="15" height="15" viewBox="0 0 16 16" fill="none">
            <path d="M1.5 4h5l2 2h6v7H1.5V4z" fill="var(--brand-soft)" stroke="var(--brand)" stroke-width="1.1"/>
            <path d="M1.5 6.5h13" stroke="var(--brand)" stroke-width="0.8"/>
          </svg>
        </template>
        <!-- .vue file -->
        <template v-else-if="fileExt() === 'vue'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <path d="M8 1.5L2 4v5.3c0 2.9 2.5 5.2 6 5.2s6-2.3 6-5.2V4L8 1.5z" fill="rgba(66,184,131,0.15)" stroke="#42b883" stroke-width="1.1"/>
            <path d="M8 1.5L2 4l6 2.5L14 4 8 1.5z" fill="none" stroke="#42b883" stroke-width="0.8"/>
          </svg>
        </template>
        <!-- .rs file -->
        <template v-else-if="fileExt() === 'rs'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(222,165,132,0.12)" stroke="#dea584" stroke-width="1.1"/>
            <text x="8" y="12" text-anchor="middle" font-size="7" font-weight="700" fill="#dea584" font-family="sans-serif">RS</text>
          </svg>
        </template>
        <!-- .ts file -->
        <template v-else-if="['ts','tsx'].includes(fileExt())">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(49,120,198,0.12)" stroke="#3178c6" stroke-width="1.1"/>
            <text x="8" y="12" text-anchor="middle" font-size="7" font-weight="700" fill="#3178c6" font-family="sans-serif">TS</text>
          </svg>
        </template>
        <!-- .json file -->
        <template v-else-if="fileExt() === 'json'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(245,201,49,0.12)" stroke="#f5c931" stroke-width="1.1"/>
            <text x="8" y="12" text-anchor="middle" font-size="6" font-weight="700" fill="#c9a825" font-family="sans-serif">{ }</text>
          </svg>
        </template>
        <!-- .toml file -->
        <template v-else-if="fileExt() === 'toml'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(156,164,178,0.12)" stroke="#9ca4b2" stroke-width="1.1"/>
            <text x="8" y="12" text-anchor="middle" font-size="6" font-weight="700" fill="#9ca4b2" font-family="sans-serif">TOML</text>
          </svg>
        </template>
        <!-- .css file -->
        <template v-else-if="fileExt() === 'css'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(86,156,214,0.12)" stroke="#569cd6" stroke-width="1.1"/>
            <text x="8" y="12" text-anchor="middle" font-size="6" font-weight="700" fill="#569cd6" font-family="sans-serif">CSS</text>
          </svg>
        </template>
        <!-- .png file -->
        <template v-else-if="fileExt() === 'png'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(239,68,68,0.1)" stroke="#ef4444" stroke-width="1.1"/>
            <circle cx="6" cy="6" r="2" fill="#ef4444"/>
            <circle cx="10" cy="10" r="2" fill="#3b82f6"/>
          </svg>
        </template>
        <!-- .md file -->
        <template v-else-if="fileExt() === 'md'">
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <rect x="2" y="2" width="12" height="12" rx="2" fill="rgba(64,128,128,0.1)" stroke="#408080" stroke-width="1.1"/>
            <path d="M4 5h6M4 8h4M4 11h5" stroke="#408080" stroke-width="1.2" stroke-linecap="round"/>
          </svg>
        </template>
        <!-- Generic file -->
        <template v-else>
          <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
            <path d="M3 2h6l4 4v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z" fill="none" stroke="var(--text-muted)" stroke-width="1.1"/>
            <path d="M9 2v4h4" fill="none" stroke="var(--text-muted)" stroke-width="1.1"/>
          </svg>
        </template>
      </span>
      <span class="name" :title="node.name">{{ node.name }}</span>
    </div>

    <div
      v-if="renaming && renaming.node.path === node.path"
      class="tree-item"
      :style="{ paddingLeft: indent(depth) }"
    >
      <span class="arrow-spacer" />
      <span class="icon">
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M3 2h6l4 4v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z" fill="none" stroke="var(--text-muted)" stroke-width="1.1"/><path d="M9 2v4h4" fill="none" stroke="var(--text-muted)" stroke-width="1.1"/></svg>
      </span>
      <input
        v-model="renaming.name"
        class="inline-input"
        @keydown="$emit('inline-keydown', $event)"
        @blur="$emit('inline-blur')"
      />
    </div>

    <template v-if="node.is_dir && !isCollapsed() && node.children">
      <TreeNodeItem
        v-for="child in node.children"
        :key="child.path"
        :node="child"
        :depth="depth + 1"
        :collapsed="collapsed"
        :selected-path="selectedPath"
        :renaming="renaming"
        :creating="creating"
        @click="(n: any) => $emit('click', n)"
        @contextmenu="(e: any, n: any) => $emit('contextmenu', e, n)"
        @inline-keydown="(e: any) => $emit('inline-keydown', e)"
        @inline-blur="() => $emit('inline-blur')"
      />
    </template>

    <div
      v-if="creating && creating.parentPath === node.path && node.is_dir"
      class="inline-input-row"
      :style="{ paddingLeft: indent(depth + 1) }"
    >
      <span class="icon">
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none"><path d="M3 2h6l4 4v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z" fill="none" stroke="var(--text-muted)" stroke-width="1.1"/><path d="M9 2v4h4" fill="none" stroke="var(--text-muted)" stroke-width="1.1"/></svg>
      </span>
      <input
        v-model="creating.name"
        class="inline-input"
        :placeholder="creating.isDir ? 'folder name' : 'file name'"
        @keydown="$emit('inline-keydown', $event)"
        @blur="$emit('inline-blur')"
      />
    </div>
  </div>
</template>

<style scoped>
.tree-item {
  display: flex;
  align-items: center;
  height: 26px;
  cursor: pointer;
  font-size: var(--fs-sm);
  white-space: nowrap;
  overflow: hidden;
  border-radius: 4px;
  margin: 0 4px;
  position: relative;
  transition: background-color 120ms ease, color 120ms ease;
}

.tree-item::before {
  content: "";
  position: absolute;
  left: 0;
  top: 50%;
  transform: translateY(-50%);
  width: 2px;
  height: 14px;
  background: transparent;
  border-radius: 0 1px 1px 0;
  transition: background-color 120ms ease;
}

.tree-item:hover {
  background: var(--bg-hover);
}

.tree-item.selected {
  background: var(--brand-soft);
}

.tree-item.selected::before {
  background: var(--brand);
}

.tree-item.selected .name {
  color: var(--brand);
}

.arrow {
  width: 14px;
  color: var(--text-muted);
  font-size: var(--fs-xs);
  flex-shrink: 0;
  text-align: center;
  transition: transform 150ms ease;
}

.arrow.expanded {
  transform: rotate(90deg);
}

.arrow-spacer {
  width: 14px;
  flex-shrink: 0;
}

.icon {
  margin-right: 6px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
}

.name {
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: var(--fs-sm);
  line-height: var(--lh-tight);
  transition: color 120ms ease;
}

.inline-input-row {
  display: flex;
  align-items: center;
  height: 26px;
  margin: 0 4px;
}

.inline-input {
  flex: 1;
  height: 22px;
  background: var(--bg-input);
  border: 1px solid var(--brand);
  color: var(--text-primary);
  font-size: var(--fs-sm);
  padding: 0 6px;
  border-radius: 4px;
  outline: none;
  font-family: "Inter", "PingFang SC", "Microsoft YaHei", sans-serif;
}
</style>
