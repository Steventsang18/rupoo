<script setup lang="ts">
import { ref } from "vue";
import { useAppStore } from "../stores/app";

const app = useAppStore();

const providers = [
  { id: "openai", label: "OpenAI", url: "https://api.openai.com/v1", doc: "https://platform.openai.com/api-keys" },
  { id: "anthropic", label: "Anthropic", url: "https://api.anthropic.com", doc: "https://console.anthropic.com/keys" },
  { id: "deepseek", label: "DeepSeek", url: "https://api.deepseek.com", doc: "https://platform.deepseek.com/api_keys" },
  { id: "ollama", label: "Ollama (Local)", url: "http://localhost:11434", doc: "" },
  { id: "openrouter", label: "OpenRouter", url: "https://openrouter.ai/api/v1", doc: "https://openrouter.ai/keys" },
  { id: "custom", label: "Custom", url: "", doc: "" },
];

const apiKeyVisible = ref(false);

function selectProvider(id: string) {
  app.updateSettings({ provider: id });
}
</script>

<template>
  <div class="settings-view">
    <div class="settings-scroll">
      <h1 class="settings-title">Settings</h1>
      <p class="settings-desc">Configure your AI provider and agent preferences</p>

      <!-- Provider Selection -->
      <section class="settings-section">
        <h2 class="section-title">AI Provider</h2>

        <div class="provider-list">
          <button
            v-for="p in providers"
            :key="p.id"
            class="provider-card"
            :class="{ active: app.settings.provider === p.id }"
            @click="selectProvider(p.id)"
          >
            <div class="pc-radio">
              <div v-if="app.settings.provider === p.id" class="pc-radio-dot" />
            </div>
            <div class="pc-info">
              <div class="pc-name">{{ p.label }}</div>
              <div v-if="p.url" class="pc-url">{{ p.url }}</div>
              <div v-if="!p.url" class="pc-url">Custom endpoint</div>
            </div>
          </button>
        </div>
      </section>

      <!-- API Configuration -->
      <section class="settings-section">
        <h2 class="section-title">API Configuration</h2>

        <div class="field-group">
          <label class="field-label">API Key</label>
          <div class="input-reveal">
            <input
              :type="apiKeyVisible ? 'text' : 'password'"
              v-model="app.settings.apiKey"
              class="field-input mono"
              placeholder="sk-..."
              @input="app.persistSettings()"
            />
            <button class="reveal-btn" @click="apiKeyVisible = !apiKeyVisible">
              <svg v-if="!apiKeyVisible" width="16" height="16" viewBox="0 0 16 16" fill="none">
                <path d="M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z" stroke="currentColor" stroke-width="1.3" fill="none"/>
                <circle cx="8" cy="8" r="2" stroke="currentColor" stroke-width="1.3" fill="none"/>
              </svg>
              <svg v-else width="16" height="16" viewBox="0 0 16 16" fill="none">
                <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
              </svg>
            </button>
          </div>
          <p class="field-hint">
            Your key is stored locally and never sent to our servers.
            <a :href="providers.find(p => p.id === app.settings.provider)?.doc" target="_blank" v-if="providers.find(p => p.id === app.settings.provider)?.doc">
              Get your API key
            </a>
          </p>
        </div>

        <div class="field-group">
          <label class="field-label">Model</label>
          <select
            v-model="app.settings.model"
            class="field-input"
            @change="app.persistSettings()"
          >
            <option value="gpt-4o">GPT-4o</option>
            <option value="gpt-4o-mini">GPT-4o Mini</option>
            <option value="o3-mini">o3-mini</option>
            <option value="claude-sonnet-4">Claude Sonnet 4</option>
            <option value="deepseek-chat">DeepSeek V3</option>
            <option value="deepseek-reasoner">DeepSeek R1</option>
            <option value="llama3.2">Llama 3.2 (Ollama)</option>
            <option value="custom">Custom</option>
          </select>
        </div>

        <div class="field-row">
          <div class="field-group flex-1">
            <label class="field-label">Temperature</label>
            <div class="slider-group">
              <input
                type="range"
                min="0" max="2" step="0.05"
                :value="app.settings.temperature"
                @input="app.updateSettings({ temperature: parseFloat(($event.target as HTMLInputElement).value) })"
                class="slider"
              />
              <span class="slider-value">{{ app.settings.temperature.toFixed(1) }}</span>
            </div>
          </div>
          <div class="field-group flex-1">
            <label class="field-label">Max Tokens</label>
            <div class="slider-group">
              <input
                type="range"
                min="512" max="32768" step="512"
                :value="app.settings.maxTokens"
                @input="app.updateSettings({ maxTokens: parseInt(($event.target as HTMLInputElement).value) })"
                class="slider"
              />
              <span class="slider-value">{{ app.settings.maxTokens.toLocaleString() }}</span>
            </div>
          </div>
        </div>
      </section>

      <!-- Model Info -->
      <section class="settings-section section-muted">
        <div class="status-row">
          <div class="status-row-icon">
            <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
              <path d="M10 3C6.134 3 3 6.134 3 10s3.134 7 7 7 7-3.134 7-7-3.134-7-7-7z" stroke="currentColor" stroke-width="1.5" fill="none"/>
              <path d="M10 6.5v4l2.5 1.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </div>
          <div>
            <div class="status-row-label">Agent Version</div>
            <div class="status-row-value">{{ app.version }}</div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.settings-view {
  height: 100%;
  overflow-y: auto;
}

.settings-scroll {
  max-width: 600px;
  margin: 0 auto;
  padding: var(--space-8) var(--space-6);
}

.settings-title {
  font-size: var(--fs-lg);
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: var(--space-1);
}

.settings-desc {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  margin-bottom: var(--space-8);
}

/* ── Section ── */
.settings-section {
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

.section-muted {
  padding: var(--space-4);
  background: var(--bg-surface);
  border-radius: var(--radius-sm);
}

/* ── Provider list ── */
.provider-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.provider-card {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
  text-align: left;
  width: 100%;
}

.provider-card:hover {
  border-color: var(--border-strong);
  background: var(--bg-hover);
}

.provider-card.active {
  border-color: var(--brand-500);
  background: var(--info-bg);
}

.pc-radio {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 2px solid var(--text-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  transition: border-color var(--duration-fast) var(--ease-out);
}

.provider-card.active .pc-radio {
  border-color: var(--brand-500);
}

.pc-radio-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--brand-500);
}

.pc-info {
  flex: 1;
}

.pc-name {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
}

.pc-url {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  margin-top: 1px;
}

/* ── Form fields ── */
.field-group {
  margin-bottom: var(--space-4);
}

.field-row {
  display: flex;
  gap: var(--space-3);
}

.flex-1 { flex: 1; }

.field-label {
  display: block;
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: var(--space-2);
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

.field-input.mono {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
}

.field-hint {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  margin-top: var(--space-1);
  line-height: var(--lh-base);
}

.field-hint a {
  color: var(--text-link);
}

.input-reveal {
  position: relative;
}

.input-reveal .field-input {
  padding-right: 44px;
}

.reveal-btn {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-2xs);
  color: var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-out);
}

.reveal-btn:hover {
  color: var(--text-primary);
  background: var(--bg-hover);
}

/* ── Slider ── */
.slider-group {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.slider {
  flex: 1;
  -webkit-appearance: none;
  appearance: none;
  height: 4px;
  background: var(--bg-elevated);
  border-radius: 2px;
  outline: none;
  border: none;
  padding: 0;
}

.slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--brand-500);
  cursor: pointer;
  border: 2px solid var(--bg-raised);
  box-shadow: var(--shadow-xs);
}

.slider-value {
  min-width: 48px;
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  font-family: var(--font-mono);
  text-align: right;
}

/* ── Status row ── */
.status-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.status-row-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-elevated);
  border-radius: var(--radius-sm);
  color: var(--text-tertiary);
}

.status-row-label {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
}

.status-row-value {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  font-family: var(--font-mono);
}

/* ── Select ── */
select.field-input {
  cursor: pointer;
}
</style>
