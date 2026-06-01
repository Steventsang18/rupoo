<script>
  import { api } from './api.js';

  let status = $state(null);
  let loading = $state(true);

  async function fetchStatus() {
    loading = true;
    try {
      const res = await api('/api/status');
      if (res.ok) status = await res.json();
    } catch (e) {
      console.error('Failed to fetch status:', e);
    } finally {
      loading = false;
    }
  }

  // Fetch on mount
  $effect(() => { fetchStatus(); });

  function formatUptime(seconds) {
    if (!seconds && seconds !== 0) return 'N/A';
    const d = Math.floor(seconds / 86400);
    const h = Math.floor((seconds % 86400) / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = Math.floor(seconds % 60);
    const parts = [];
    if (d > 0) parts.push(`${d}d`);
    if (h > 0) parts.push(`${h}h`);
    if (m > 0) parts.push(`${m}m`);
    parts.push(`${s}s`);
    return parts.join(' ');
  }

  function formatBytes(bytes) {
    if (!bytes && bytes !== 0) return 'N/A';
    const units = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    let size = bytes;
    while (size >= 1024 && i < units.length - 1) {
      size /= 1024;
      i++;
    }
    return `${size.toFixed(1)} ${units[i]}`;
  }

  let valueEntries = $derived(
    status ? Object.entries(status).filter(([k]) => k !== 'label') : []
  );
</script>

<!-- auto-fetched via $effect -->

<div class="p-4">
  <div class="flex items-center justify-between mb-4">
    <h1 class="text-cyan-400 font-bold text-lg">Status</h1>
    <button onclick={fetchStatus} class="text-xs bg-gray-800 hover:bg-gray-700 border border-gray-700 px-2 py-1 rounded font-mono">
      Refresh
    </button>
  </div>

  {#if loading}
    <div class="text-center py-12 text-gray-500">Loading status...</div>
  {:else if !status}
    <div class="text-center py-12 text-gray-500 text-sm">Failed to load system status.</div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
      <!-- System Info -->
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <h2 class="text-gray-400 text-xs uppercase tracking-wider mb-3">System</h2>
        <div class="space-y-2">
          {#if status.version}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">Version</span>
              <span class="text-gray-200 text-xs font-mono">{status.version}</span>
            </div>
          {/if}
          {#if status.uptime || status.uptime_seconds}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">Uptime</span>
              <span class="text-gray-200 text-xs font-mono">{formatUptime(status.uptime_seconds || status.uptime)}</span>
            </div>
          {/if}
          {#if status.arch}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">Architecture</span>
              <span class="text-gray-200 text-xs font-mono">{status.arch}</span>
            </div>
          {/if}
          {#if status.os}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">OS</span>
              <span class="text-gray-200 text-xs font-mono">{status.os}</span>
            </div>
          {/if}
        </div>
      </div>

      <!-- Memory / Resource -->
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <h2 class="text-gray-400 text-xs uppercase tracking-wider mb-3">Resources</h2>
        <div class="space-y-2">
          {#if status.memory_used || status.memory}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">Memory Used</span>
              <span class="text-gray-200 text-xs font-mono">
                {formatBytes(status.memory_used || status.memory?.used)}
              </span>
            </div>
          {/if}
          {#if status.memory_total || status.memory}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">Memory Total</span>
              <span class="text-gray-200 text-xs font-mono">
                {formatBytes(status.memory_total || status.memory?.total)}
              </span>
            </div>
          {/if}
          {#if status.cpu_usage || status.cpu}
            <div class="flex justify-between">
              <span class="text-gray-500 text-xs">CPU Usage</span>
              <span class="text-gray-200 text-xs font-mono">
                {status.cpu_usage || status.cpu}%
              </span>
            </div>
          {/if}
        </div>
      </div>

      <!-- Stats -->
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4 md:col-span-2">
        <h2 class="text-gray-400 text-xs uppercase tracking-wider mb-3">Statistics</h2>
        <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
          {#each valueEntries as [key, value]}
            <div class="bg-gray-950 rounded p-3">
              <div class="text-gray-500 text-xs mb-1 truncate">{key}</div>
              <div class="text-gray-200 text-sm font-mono truncate">
                {typeof value === 'object' ? JSON.stringify(value) : String(value)}
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>
