<script setup lang="ts">
import { ref, nextTick, watch, computed, onMounted } from "vue";
import { useChatStore } from "../stores/chat";
import { useAppStore } from "../stores/app";

const chatStore = useChatStore();
const appStore = useAppStore();

const inputText = ref("");
const showHistory = ref(false);
const chatContainer = ref<HTMLElement | null>(null);
const textareaRef = ref<HTMLTextAreaElement | null>(null);

async function scrollToBottom() {
  await nextTick();
  if (chatContainer.value) {
    chatContainer.value.scrollTop = chatContainer.value.scrollHeight;
  }
}

watch(() => chatStore.streamingContent, () => scrollToBottom());
watch(() => chatStore.messages.length, () => scrollToBottom());

function send() {
  const text = inputText.value.trim();
  if (!text || chatStore.streaming) return;
  inputText.value = "";
  chatStore.sendMessage(text, true);
  // Auto-resize textarea
  if (textareaRef.value) {
    textareaRef.value.style.height = "auto";
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
  }
  // Auto-resize
  const ta = e.target as HTMLTextAreaElement;
  ta.style.height = "auto";
  ta.style.height = Math.min(ta.scrollHeight, 160) + "px";
}

function autoResize() {
  if (textareaRef.value) {
    textareaRef.value.style.height = "auto";
    textareaRef.value.style.height = Math.min(textareaRef.value.scrollHeight, 160) + "px";
  }
}

// Markdown rendering helper
function renderContent(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/```(\w*)\n([\s\S]*?)```/g,
      '<div class="code-block"><div class="code-hdr"><span class="code-lang">$1</span><button class="code-copy" data-code="$2">Copy</button></div><pre><code>$2</code></pre></div>')
    .replace(/`([^`]+)`/g, '<code class="inline-code">$1</code>')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/\n/g, "<br>");
}

function handleMessageClick(e: MouseEvent) {
  const target = e.target as HTMLElement;
  if (target.classList.contains("code-copy")) {
    const code = target.getAttribute("data-code") || "";
    navigator.clipboard.writeText(code).then(() => {
      target.textContent = "Copied!";
      setTimeout(() => { target.textContent = "Copy"; }, 1500);
    });
  }
}

function fmtTime(ts: number): string {
  return new Date(ts).toLocaleTimeString("en-US", { hour12: false, hour: "2-digit", minute: "2-digit" });
}

const hasMessages = computed(() => chatStore.messages.length > 0);

// Focus input on mount
onMounted(() => {
  setTimeout(() => textareaRef.value?.focus(), 300);
});
</script>

<template>
  <div class="chat-view">
    <!-- Messages area -->
    <div ref="chatContainer" class="chat-messages" @click="handleMessageClick">
      <!-- Empty state -->
      <div v-if="!hasMessages && !chatStore.streaming" class="chat-empty">
        <div class="empty-icon">
          <svg width="32" height="32" viewBox="0 0 32 32" fill="none">
            <rect x="3" y="4" width="26" height="20" rx="3" stroke="currentColor" stroke-width="1.5" fill="none"/>
            <path d="M11 27l5-3 5 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>
          </svg>
        </div>
        <h2 class="empty-title">What can I help you with?</h2>
        <p class="empty-sub">Ask anything — code review, planning, file ops, or general questions</p>
        <div class="quick-prompts">
          <button
            v-for="q in ['Review my project', 'Generate a plan', 'Explain my code', 'Write a test']"
            :key="q"
            class="quick-btn"
            @click="inputText = q; autoResize()"
          >{{ q }}</button>
        </div>
      </div>

      <!-- Messages -->
      <div v-if="hasMessages" class="messages-list">
        <template v-for="msg in chatStore.messages" :key="msg.id">
          <div v-if="msg.role !== 'system'" class="msg-row" :class="msg.role">
            <div class="msg-avatar" :class="msg.role">
              <span v-if="msg.role === 'user'">U</span>
              <span v-else>R</span>
            </div>
            <div class="msg-bubble">
              <div class="msg-text" v-html="renderContent(msg.content)" />
              <div class="msg-time">{{ fmtTime(msg.timestamp) }}</div>
            </div>
          </div>
        </template>

        <!-- Streaming -->
        <div v-if="chatStore.streaming" class="msg-row assistant">
          <div class="msg-avatar assistant">R</div>
          <div class="msg-bubble">
            <div class="msg-text" v-html="renderContent(chatStore.streamingContent) + '<span class=\'cursor-blink\'>|</span>'" />
            <div class="msg-time streaming-label">Generating...</div>
          </div>
        </div>
      </div>
    </div>

    <!-- Error toast -->
    <Transition name="fade">
      <div v-if="chatStore.error" class="error-toast">
        <div class="error-toast-content">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
            <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.5" fill="none"/>
            <path d="M8 5v3M8 11h0" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <span>{{ chatStore.error }}</span>
        </div>
        <button class="error-dismiss" @click="chatStore.clearError()">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          </svg>
        </button>
      </div>
    </Transition>

    <!-- Input area -->
    <div class="chat-input-area">
      <div class="input-container">
        <textarea
          ref="textareaRef"
          v-model="inputText"
          class="chat-input"
          placeholder="Message rupoo... (Enter to send, Shift+Enter for new line)"
          rows="1"
          @keydown="handleKeydown"
          @input="autoResize"
          :disabled="chatStore.streaming"
        />
        <div class="input-actions">
          <button
            v-if="!chatStore.streaming"
            class="send-btn"
            :disabled="!inputText.trim()"
            @click="send"
            title="Send"
          >
            <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
              <path d="M3 3l13 6-13 6 3-6-3-6z" fill="currentColor"/>
            </svg>
          </button>
          <button
            v-else
            class="stop-btn"
            @click="chatStore.stopStreaming()"
            title="Stop"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <rect x="2" y="2" width="10" height="10" rx="1" fill="currentColor"/>
            </svg>
          </button>
        </div>
      </div>
      <div class="input-legal">
        rupoo may produce inaccurate information. Verify important facts.
      </div>
    </div>
  </div>
</template>

<style scoped>
.chat-view {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-root);
  position: relative;
}

/* ── Messages area ── */
.chat-messages {
  flex: 1;
  overflow-y: auto;
  padding: 0;
  min-height: 0;
}

/* Empty state */
.chat-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100%;
  padding: var(--space-8);
  text-align: center;
}

.empty-icon {
  width: 56px;
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-surface);
  border-radius: var(--radius-lg);
  color: var(--text-tertiary);
  margin-bottom: var(--space-4);
}

.empty-title {
  font-size: var(--fs-xl);
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: var(--space-2);
}

.empty-sub {
  font-size: var(--fs-sm);
  color: var(--text-secondary);
  margin-bottom: var(--space-6);
  max-width: 360px;
}

.quick-prompts {
  display: flex;
  gap: var(--space-2);
  flex-wrap: wrap;
  justify-content: center;
}

.quick-btn {
  padding: var(--space-2) var(--space-4);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-full);
  color: var(--text-secondary);
  font-size: var(--fs-xs);
  font-weight: 500;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
}

.quick-btn:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  color: var(--text-primary);
}

/* Messages list */
.messages-list {
  padding: var(--space-6) var(--space-4);
  max-width: 800px;
  margin: 0 auto;
  width: 100%;
}

.msg-row {
  display: flex;
  gap: var(--space-3);
  margin-bottom: var(--space-5);
  animation: msgIn var(--duration-slow) var(--ease-out);
}

@keyframes msgIn {
  from { opacity: 0; transform: translateY(8px); }
  to   { opacity: 1; transform: translateY(0); }
}

.msg-row.user {
  flex-direction: row-reverse;
}

.msg-avatar {
  width: 32px;
  height: 32px;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
  flex-shrink: 0;
}

.msg-avatar.user {
  background: var(--brand-500);
  color: #fff;
}

.msg-avatar.assistant {
  background: var(--bg-elevated);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
}

.msg-bubble {
  max-width: 75%;
  min-width: 0;
}

.msg-row.user .msg-bubble {
  background: var(--brand-500);
  color: #fff;
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md) var(--radius-md) var(--radius-2xs) var(--radius-md);
}

.msg-row.assistant .msg-bubble {
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md) var(--radius-md) var(--radius-md) var(--radius-2xs);
}

.msg-text {
  font-size: var(--fs-sm);
  line-height: var(--lh-relaxed);
  word-wrap: break-word;
}

.msg-row.user .msg-text {
  color: #fff;
}

/* Code block in messages */
:deep(.code-block) {
  margin: var(--space-2) 0;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid var(--border-default);
  background: var(--bg-deepest);
}

:deep(.code-hdr) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-1) var(--space-3);
  background: var(--bg-elevated);
  border-bottom: 1px solid var(--border-default);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
}

:deep(.code-lang) {
  font-weight: 500;
}

:deep(.code-copy) {
  padding: 2px 8px;
  font-size: var(--fs-2xs);
  font-weight: 500;
  color: var(--text-tertiary);
  background: transparent;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-2xs);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out);
}

:deep(.code-copy:hover) {
  background: var(--bg-hover);
  color: var(--text-primary);
}

:deep(.code-block pre) {
  padding: var(--space-3);
  overflow-x: auto;
  font-size: var(--fs-xs);
  line-height: 1.6;
  font-family: var(--font-mono);
  color: var(--text-primary);
}

:deep(.code-block code) {
  font-family: var(--font-mono);
}

:deep(.inline-code) {
  padding: 1px 5px;
  background: var(--bg-elevated);
  border-radius: var(--radius-2xs);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  border: 1px solid var(--border-default);
}

:deep(.cursor-blink) {
  animation: blink 1s steps(2) infinite;
  color: var(--brand-500);
  font-weight: 700;
}

@keyframes blink {
  0%   { opacity: 1; }
  50%  { opacity: 0; }
}

.msg-time {
  font-size: 10px;
  color: var(--text-tertiary);
  margin-top: var(--space-1);
  opacity: 0.7;
}

.msg-row.user .msg-time {
  color: rgba(255,255,255,0.6);
}

.streaming-label {
  animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 0.7; }
  50%      { opacity: 1; }
}

/* ── Error toast ── */
.error-toast {
  position: absolute;
  bottom: 100px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  background: var(--error-bg);
  border: 1px solid rgba(248,113,113,0.3);
  border-radius: var(--radius-sm);
  color: var(--error);
  font-size: var(--fs-xs);
  font-weight: 500;
  max-width: 500px;
  z-index: 20;
  backdrop-filter: blur(8px);
}

.error-toast-content {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
}

.error-dismiss {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: var(--radius-2xs);
  color: var(--error);
  transition: all var(--duration-fast) var(--ease-out);
  flex-shrink: 0;
}

.error-dismiss:hover {
  background: rgba(248,113,113,0.2);
}

/* ── Input area ── */
.chat-input-area {
  padding: var(--space-3) var(--space-4) var(--space-3);
  flex-shrink: 0;
  border-top: 1px solid var(--border-default);
  background: var(--bg-root);
}

.input-container {
  display: flex;
  align-items: flex-end;
  gap: var(--space-2);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: var(--space-2);
  transition: border-color var(--duration-fast) var(--ease-out),
              box-shadow var(--duration-fast) var(--ease-out);
}

.input-container:focus-within {
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(59,130,246,0.10);
}

.chat-input {
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text-primary);
  font-size: var(--fs-sm);
  font-family: var(--font-sans);
  resize: none;
  padding: var(--space-1) var(--space-2);
  max-height: 160px;
  line-height: 1.5;
}

.chat-input::placeholder {
  color: var(--text-tertiary);
}

.chat-input:disabled {
  opacity: 0.6;
}

.input-actions {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  flex-shrink: 0;
}

.send-btn, .stop-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-xs);
  transition: all var(--duration-fast) var(--ease-out);
  flex-shrink: 0;
}

.send-btn {
  color: var(--text-on-brand);
  background: var(--brand-500);
}

.send-btn:hover:not(:disabled) {
  background: var(--brand-600);
}

.stop-btn {
  color: #fff;
  background: var(--error);
}

.stop-btn:hover {
  background: #dc2626;
}

.input-legal {
  font-size: 10px;
  color: var(--text-tertiary);
  text-align: center;
  margin-top: var(--space-2);
  opacity: 0.5;
}
</style>
