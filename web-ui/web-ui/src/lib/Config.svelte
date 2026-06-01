<script>
  import { api } from './api.js';

  let configEntries = $state([]);
  let loading = $state(true);
  let editingKey = $state(null);
  let editValue = $state('');
  let saveResult = $state(null);

  function isApiKey(key) {
    const lower = key.toLowerCase();
    return lower.includes('api_key') || lower.includes('apikey') || lower.includes('token') || lower.includes('secret');
  }

  async function fetchConfig() {
    loading = true;
    try {
      const res = await api('/api/config');
      if (res.ok) configEntries = await res.json();
    } catch (e) {
      console.error('Failed to fetch config:', e);
    } finally {
      loading = false;
    }
  }

  // Fetch on mount
  $effect(() => { fetchConfig(); });

  function startEdit(key, value) {
    editingKey = key;
    editValue = value;
    saveResult = null;
  }

  function cancelEdit() {
    editingKey = null;
    editValue = '';
    saveResult = null;
  }

  async function saveConfig() {
    if (!editingKey) return;
    saveResult = null;
    try {
      const res = await api(`/api/config/${encodeURIComponent(editingKey)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ value: editValue }),
      });
      if (res.ok) {
        saveResult = { success: true, message: 'Saved.' };
        configEntries = configEntries.map(entry =>
          entry.key === editingKey ? { ...entry, value: editValue } : entry
        );
        editingKey = null;
        editValue = '';
      } else {
        const err = await res.text();
        saveResult = { success: false, message: err };
      }
    } catch (e) {
      saveResult = { success: false, message: e.message };
    }
  }

  async function deleteConfig(key) {
    try {
      const res = await api(`/api/config/${encodeURIComponent(key)}`, { method: 'DELETE' });
      if (res.ok) {
        configEntries = configEntries.filter(entry => entry.key !== key);
      }
    } catch (e) {
      console.error('Failed to delete config:', e);
    }
  }
</script>

<div class="p-4">
  <div class="flex items-center justify-between mb-4">
    <h1 class="text-cyan-400 font-bold text-lg">Config</h1>
    <button onclick={fetchConfig} class="text-xs bg-gray-800 hover:bg-gray-700 border border-gray-700 px-2 py-1 rounded font-mono">
      Refresh
    </button>
  </div>

  {#if loading}
    <div class="text-center py-12 text-gray-500">Loading config...</div>
  {:else if configEntries.length === 0}
    <div class="text-center py-12 text-gray-500 text-sm">No configuration entries.</div>
  {:else}
    <div class="space-y-2">
      {#each configEntries as entry}
        <div class="bg-gray-900 border border-gray-800 rounded-lg p-3">
          <div class="flex items-start justify-between gap-2">
            <div class="flex-1 min-w-0">
              <div class="text-cyan-400 text-xs font-mono font-medium mb-1 break-all">{entry.key}</div>
              {#if editingKey === entry.key}
                <!-- Edit mode -->
                {#if isApiKey(entry.key)}
                  <input
                    bind:value={editValue}
                    type="password"
                    class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
                  />
                {:else}
                  <textarea
                    bind:value={editValue}
                    class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600 resize-none"
                    rows="2"
                  ></textarea>
                {/if}
                <div class="flex gap-2 mt-2">
                  <button onclick={saveConfig} class="px-2 py-1 bg-cyan-700 rounded hover:bg-cyan-600 text-xs font-mono">Save</button>
                  <button onclick={cancelEdit} class="px-2 py-1 bg-gray-700 rounded hover:bg-gray-600 text-xs font-mono">Cancel</button>
                </div>
                {#if saveResult}
                  <p class="mt-1 text-xs {saveResult.success ? 'text-green-400' : 'text-red-400'} font-mono">{saveResult.message}</p>
                {/if}
              {:else}
                <!-- View mode -->
                <p class="text-gray-300 text-sm font-mono break-all">
                  {isApiKey(entry.key) ? maskApiKey(entry.value) : entry.value}
                </p>
              {/if}
            </div>
            <div class="flex gap-1 shrink-0">
              <button
                onclick={() => startEdit(entry.key, entry.value)}
                class="px-2 py-1 bg-gray-800 rounded hover:bg-gray-700 text-xs font-mono text-gray-400"
              >
                Edit
              </button>
              <button
                onclick={() => deleteConfig(entry.key)}
                class="px-2 py-1 bg-red-900 rounded hover:bg-red-800 text-xs font-mono text-red-300"
              >
                Del
              </button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
