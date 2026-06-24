<script setup lang="ts">
import { ref } from "vue";
import { useAppStore } from "../stores/app";

const app = useAppStore();

/* Quick action cards */
interface QuickAction {
  icon: string;
  title: string;
  desc: string;
  action: () => void;
}

const showSetupModal = ref(false);

const quickActions: QuickAction[] = [
  {
    icon: `<svg width="28" height="28" viewBox="0 0 28 28" fill="none"><path d="M14 3C7.925 3 3 7.925 3 14s4.925 11 11 11 11-4.925 11-11S20.075 3 14 3z" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M10 14l3 3 5-5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
    title: "Connect AI Provider",
    desc: "Set up your AI model — OpenAI, Anthropic, DeepSeek, Ollama, and more",
    action: () => app.navigate("settings"),
  },
  {
    icon: `<svg width="28" height="28" viewBox="0 0 28 28" fill="none"><rect x="4" y="4" width="20" height="20" rx="3" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M8 10h12M8 14h10M8 18h8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`,
    title: "Start a Chat",
    desc: "Ask questions, get code reviews, generate plans, or explore your project",
    action: () => app.navigate("chat"),
  },
  {
    icon: `<svg width="28" height="28" viewBox="0 0 28 28" fill="none"><circle cx="14" cy="14" r="11" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M10 12l3 3 5-5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M7 20l4-4M17 12l4-4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" opacity="0.5"/></svg>`,
    title: "Explore Workspace",
    desc: "Browse files, search code, and let the AI understand your project",
    action: () => app.navigate("files"),
  },
  {
    icon: `<svg width="28" height="28" viewBox="0 0 28 28" fill="none"><path d="M14 3C7.925 3 3 7.925 3 14s4.925 11 11 11 11-4.925 11-11S20.075 3 14 3z" stroke="currentColor" stroke-width="1.5" fill="none"/><path d="M14 8v6l4 2" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
    title: "View Agent Status",
    desc: "Monitor AI agent activity, tool calls, plans, and memory usage",
    action: () => app.navigate("agent"),
  },
];

/* Setup wizard mock — in production this would be a multi-step flow */
const providerOptions = [
  { id: "openai", label: "OpenAI", models: ["gpt-4o", "gpt-4o-mini", "o3-mini"] },
  { id: "anthropic", label: "Anthropic", models: ["claude-sonnet-4", "claude-haiku-4"] },
  { id: "deepseek", label: "DeepSeek", models: ["deepseek-chat", "deepseek-reasoner"] },
  { id: "ollama", label: "Ollama (Local)", models: ["llama3", "mistral", "codellama"] },
  { id: "openrouter", label: "OpenRouter", models: ["anthropic/claude-sonnet-4", "openai/gpt-4o"] },
  { id: "custom", label: "Custom", models: ["custom"] },
];

const selectedProvider = ref("openai");
</script>

<template>
  <div class="welcome">
    <div class="welcome-scroll">
      <!-- Hero section -->
      <div class="hero">
        <div class="hero-brand">
          <div class="hero-logo">R</div>
        </div>
        <h1 class="hero-title">Your AI Agent Desktop</h1>
        <p class="hero-subtitle">
          rupoo brings powerful AI agents to your desktop.
          Chat, code, explore, and automate — all in one place.
        </p>

        <!-- Quick start CTA -->
        <div v-if="app.needsSetup" class="hero-cta" @click="showSetupModal = true">
          <div class="cta-icon">
            <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
              <path d="M10 3v14M3 10h14" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
            </svg>
          </div>
          <div class="cta-text">
            <div class="cta-title">Connect to get started</div>
            <div class="cta-desc">Set up your AI provider in one click</div>
          </div>
          <div class="cta-arrow">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <path d="M6 3l5 5-5 5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </div>
        </div>
      </div>

      <!-- Quick actions grid -->
      <div class="actions-grid">
        <button
          v-for="(action, i) in quickActions"
          :key="i"
          class="action-card"
          @click="action.action"
        >
          <div class="action-icon" v-html="action.icon" />
          <div class="action-info">
            <div class="action-title">{{ action.title }}</div>
            <div class="action-desc">{{ action.desc }}</div>
          </div>
        </button>
      </div>

      <!-- Features overview -->
      <div class="features">
        <div class="feature">
          <div class="feature-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
              <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/>
            </svg>
          </div>
          <div>
            <div class="feature-title">Smart Conversations</div>
            <div class="feature-desc">Context-aware AI that understands your codebase and remembers history</div>
          </div>
        </div>
        <div class="feature">
          <div class="feature-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
              <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/>
              <path d="M14 2v6h6M16 13H8M16 17H8M10 9H8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
            </svg>
          </div>
          <div>
            <div class="feature-title">Code & Files</div>
            <div class="feature-desc">Browse, edit, and review files with AI-powered assistance</div>
          </div>
        </div>
        <div class="feature">
          <div class="feature-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
              <rect x="3" y="3" width="18" height="18" rx="2" stroke="currentColor" stroke-width="1.5" fill="none"/>
              <rect x="7" y="7" width="10" height="10" rx="1" stroke="currentColor" stroke-width="1.3" fill="none"/>
              <path d="M7 10h10M7 14h6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
            </svg>
          </div>
          <div>
            <div class="feature-title">Agent Workflows</div>
            <div class="feature-desc">Plan execution, tool orchestration, and automated task processing</div>
          </div>
        </div>
        <div class="feature">
          <div class="feature-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
              <path d="M12 2L2 7l10 5 10-5-10-5z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/>
              <path d="M2 17l10 5 10-5M2 12l10 5 10-5" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" fill="none"/>
            </svg>
          </div>
          <div>
            <div class="feature-title">Extensible Platform</div>
            <div class="feature-desc">MCP tools, custom skills, and multi-model support</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Setup Modal -->
    <Transition name="fade">
      <div v-if="showSetupModal" class="modal-overlay" @click.self="showSetupModal = false">
        <div class="modal-card" @click.stop>
          <div class="modal-header">
            <div class="modal-title">Connect AI Provider</div>
            <button class="modal-close" @click="showSetupModal = false">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
              </svg>
            </button>
          </div>

          <div class="modal-body">
            <label class="field-label">Provider</label>
            <div class="provider-grid">
              <button
                v-for="p in providerOptions"
                :key="p.id"
                class="provider-option"
                :class="{ selected: selectedProvider === p.id }"
                @click="selectedProvider = p.id"
              >
                <div class="po-dot" :class="{ filled: selectedProvider === p.id }" />
                <span>{{ p.label }}</span>
              </button>
            </div>

            <label class="field-label" style="margin-top: 16px;">API Key</label>
            <input
              v-model="app.settings.apiKey"
              type="password"
              class="field-input"
              placeholder="sk-..."
              @input="app.persistSettings()"
            />

            <label class="field-label" style="margin-top: 14px;">Model</label>
            <select v-model="app.settings.model" class="field-input" @change="app.persistSettings()">
              <option v-for="m in (providerOptions.find(p => p.id === selectedProvider)?.models || [])" :key="m" :value="m">{{ m }}</option>
            </select>
          </div>

          <div class="modal-footer">
            <button class="btn btn-secondary" @click="showSetupModal = false">Cancel</button>
            <button
              class="btn btn-primary"
              :disabled="!app.settings.apiKey.trim()"
              @click="
                app.updateSettings({ provider: selectedProvider });
                app.completeSetup();
                showSetupModal = false;
                app.navigate('chat');
              "
            >
              Save & Start Chatting
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.welcome {
  height: 100%;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.welcome-scroll {
  width: 100%;
  max-width: 680px;
  padding: var(--space-12) var(--space-8) var(--space-10);
}

/* ── Hero ── */
.hero {
  text-align: center;
  margin-bottom: var(--space-10);
}

.hero-brand {
  margin-bottom: var(--space-4);
}

.hero-logo {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  height: 52px;
  background: var(--accent-gradient);
  border-radius: var(--radius-lg);
  color: #fff;
  font-size: 26px;
  font-weight: 700;
  box-shadow: 0 8px 32px rgba(59,130,246,0.25);
}

.hero-title {
  font-size: var(--fs-2xl);
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: var(--space-2);
  letter-spacing: var(--ls-tight);
}

.hero-subtitle {
  font-size: var(--fs-base);
  color: var(--text-secondary);
  max-width: 420px;
  margin: 0 auto var(--space-6);
  line-height: var(--lh-relaxed);
}

/* ── CTA button ── */
.hero-cta {
  display: inline-flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-5);
  background: var(--bg-surface);
  border: 1px solid var(--border-accent);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all var(--duration-normal) var(--ease-out);
  text-align: left;
}

.hero-cta:hover {
  background: var(--bg-hover);
  border-color: var(--brand-500);
  transform: translateY(-1px);
  box-shadow: var(--shadow-md);
}

.cta-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--brand-500);
  border-radius: var(--radius-sm);
  color: #fff;
  flex-shrink: 0;
}

.cta-text {
  flex: 1;
}

.cta-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
}

.cta-desc {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  margin-top: 2px;
}

.cta-arrow {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

/* ── Quick actions grid ── */
.actions-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-3);
  margin-bottom: var(--space-10);
}

.action-card {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
  padding: var(--space-4);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all var(--duration-normal) var(--ease-out);
  text-align: left;
  width: 100%;
}

.action-card:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  transform: translateY(-1px);
  box-shadow: var(--shadow-sm);
}

.action-icon {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-elevated);
  border-radius: var(--radius-sm);
  color: var(--brand-500);
  flex-shrink: 0;
}

.action-info {
  flex: 1;
  min-width: 0;
}

.action-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 2px;
}

.action-desc {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  line-height: var(--lh-base);
}

/* ── Features ── */
.features {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-4);
}

.feature {
  display: flex;
  gap: var(--space-3);
  padding: var(--space-3);
  border-radius: var(--radius-sm);
}

.feature:hover {
  background: var(--bg-surface);
}

.feature-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-elevated);
  border-radius: var(--radius-sm);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.feature-title {
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 2px;
}

.feature-desc {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  line-height: var(--lh-base);
}

/* ── Modal ── */
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  backdrop-filter: blur(4px);
}

.modal-card {
  width: 460px;
  max-width: 90vw;
  background: var(--bg-raised);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--border-default);
}

.modal-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}

.modal-close {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-2xs);
  color: var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-out);
}

.modal-close:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.modal-body {
  padding: var(--space-5);
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-4) var(--space-5);
  border-top: 1px solid var(--border-default);
}

/* ── Form elements ── */
.field-label {
  display: block;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: var(--space-2);
  letter-spacing: var(--ls-caps);
  text-transform: uppercase;
}

.field-input {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  font-size: var(--fs-sm);
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-xs);
  color: var(--text-primary);
  outline: none;
  transition: all var(--duration-fast) var(--ease-out);
}

.field-input:focus {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(59,130,246,0.15);
}

.provider-grid {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: var(--space-2);
}

.provider-option {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-xs);
  color: var(--text-secondary);
  font-size: var(--fs-xs);
  font-weight: 500;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
}

.provider-option:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
}

.provider-option.selected {
  border-color: var(--brand-500);
  color: var(--text-primary);
  background: var(--info-bg);
}

.po-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  border: 1.5px solid var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-out);
}

.po-dot.filled {
  background: var(--brand-500);
  border-color: var(--brand-500);
}

/* ── Buttons ── */
.btn {
  padding: var(--space-2) var(--space-4);
  font-size: var(--fs-sm);
  font-weight: 600;
  border-radius: var(--radius-xs);
  transition: all var(--duration-fast) var(--ease-out);
  white-space: nowrap;
}

.btn-primary {
  background: var(--brand-500);
  color: #fff;
}

.btn-primary:hover:not(:disabled) {
  background: var(--brand-600);
}

.btn-secondary {
  background: var(--bg-hover);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
}

.btn-secondary:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}
</style>
