<script>
  import { api } from './api.js';

  let memories = $state([]);
  let searchQuery = $state('');
  let searchResults = $state(null);
  let loading = $state(true);
  let searching = $state(false);

  // New memory form
  let newContent = $state('');
  let newTags = $state('');
  let storeResult = $state(null);

  async function fetchRecent() {
    loading = true;
    try {
      const res = await api('/api/memory/recent');
      if (res.ok) memories = await res.json();
    } catch (e) {
      console.error('Failed to fetch recent memories:', e);
    } finally {
      loading = false;
    }
  }

  async function doSearch() {
    if (!searchQuery.trim()) return;
    searching = true;
    searchResults = null;
    try {
      const res = await api(`/api/memory/search?q=${encodeURIComponent(searchQuery.trim())}`);
      if (res.ok) searchResults = await res.json();
      else searchResults = [];
    } catch (e) {
      searchResults = [];
      console.error('Search failed:', e);
    } finally {
      searching = false;
    }
  }

  async function storeMemory() {
    if (!newContent.trim()) return;
    storeResult = null;
    try {
      const tags = newTags.trim() ? newTags.split(',').map(t => t.trim()).filter(Boolean) : [];
      const res = await api('/api/memory', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          content: newContent.trim(),
          tags,
          source: 'web',
        }),
      });
      if (res.ok) {
        storeResult = { success: true, message: 'Memory stored.' };
        newKey = '';
        newValue = '';
        newDomain = '';
        fetchRecent();
      } else {
        const err = await res.text();
        storeResult = { success: false, message: err };
      }
    } catch (e) {
      storeResult = { success: false, message: e.message };
    }
  }

  function clearSearch() {
    searchQuery = '';
    searchResults = null;
  }
</script>

<svelte:window on:load={fetchRecent} />

<div class="p-4">
  <div class="flex items-center justify-between mb-4">
    <h1 class="text-cyan-400 font-bold text-lg">Memory</h1>
    <button onclick={fetchRecent} class="text-xs bg-gray-800 hover:bg-gray-700 border border-gray-700 px-2 py-1 rounded font-mono">
      Refresh
    </button>
  </div>

  <!-- Search -->
  <div class="mb-4">
    <div class="flex gap-2">
      <input
        bind:value={searchQuery}
        onkeydown={(e) => e.key === 'Enter' && doSearch()}
        class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
        placeholder="Search memories..."
      />
      <button
        onclick={doSearch}
        disabled={!searchQuery.trim() || searching}
        class="px-3 py-2 bg-cyan-700 rounded hover:bg-cyan-600 disabled:opacity-40 text-sm font-mono"
      >
        {searching ? '...' : 'Search'}
      </button>
      {#if searchResults !== null}
        <button onclick={clearSearch} class="px-3 py-2 bg-gray-800 rounded hover:bg-gray-700 text-sm font-mono">
          Clear
        </button>
      {/if}
    </div>
  </div>

  <!-- Search results -->
  {#if searchResults !== null}
    <div class="mb-4">
      <h2 class="text-gray-400 text-xs uppercase tracking-wider mb-2">Search Results</h2>
      {#if searchResults.length === 0}
        <p class="text-gray-500 text-sm">No results found.</p>
      {:else}
        <div class="space-y-2">
          {#each searchResults as mem}
            <div class="bg-gray-900 border border-gray-800 rounded-lg p-3">
              <div class="flex items-center gap-2 mb-1">
                <span class="text-cyan-400 text-xs font-mono font-medium">{mem.key}</span>
                {#if mem.domain}
                  <span class="text-gray-500 text-xs">{mem.domain}</span>
                {/if}
              </div>
              <p class="text-gray-300 text-sm whitespace-pre-wrap">{mem.value}</p>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Store new memory -->
  <div class="bg-gray-900 border border-gray-800 rounded-lg p-4 mb-4">
    <h2 class="text-gray-300 text-sm font-medium mb-3">Store Memory</h2>
    <div class="space-y-2">
      <input
        bind:value={newKey}
        class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
        placeholder="Key"
      />
      <textarea
        bind:value={newValue}
        class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600 resize-none"
        rows="2"
        placeholder="Value"
      ></textarea>
      <input
        bind:value={newDomain}
        class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
        placeholder="Domain (optional)"
      />
      <button
        onclick={storeMemory}
        disabled={!newKey.trim() || !newValue.trim()}
        class="px-3 py-1.5 bg-cyan-700 rounded hover:bg-cyan-600 disabled:opacity-40 text-sm font-mono"
      >
        Store
      </button>
      {#if storeResult}
        <p class="text-xs {storeResult.success ? 'text-green-400' : 'text-red-400'} font-mono">
          {storeResult.message}
        </p>
      {/if}
    </div>
  </div>

  <!-- Recent memories -->
  <div>
    <h2 class="text-gray-400 text-xs uppercase tracking-wider mb-2">Recent</h2>
    {#if loading}
      <p class="text-gray-500 text-sm">Loading...</p>
    {:else if recentMemories.length === 0}
      <p class="text-gray-500 text-sm">No memories stored yet.</p>
    {:else}
      <div class="space-y-2">
        {#each recentMemories as mem}
          <div class="bg-gray-900 border border-gray-800 rounded-lg p-3">
            <div class="flex items-center gap-2 mb-1">
              <span class="text-cyan-400 text-xs font-mono font-medium">{mem.key}</span>
              {#if mem.domain}
                <span class="text-gray-500 text-xs">{mem.domain}</span>
              {/if}
            </div>
            <p class="text-gray-300 text-sm whitespace-pre-wrap line-clamp-3">{mem.value}</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
