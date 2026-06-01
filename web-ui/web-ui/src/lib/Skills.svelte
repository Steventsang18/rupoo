<script>
  import { api } from './api.js';

  let skills = $state([]);
  let searchQuery = $state('');
  let loading = $state(true);
  let learnPlanName = $state('');
  let learnResult = $state(null);

  async function fetchSkills() {
    loading = true;
    try {
      const res = await api('/api/skills');
      if (res.ok) skills = await res.json();
    } catch (e) {
      console.error('Failed to fetch skills:', e);
    } finally {
      loading = false;
    }
  }

  // Fetch on mount
  $effect(() => { fetchSkills(); });

  let filteredSkills = $derived(
    searchQuery
      ? skills.filter(s =>
          (s.name || '').toLowerCase().includes(searchQuery.toLowerCase()) ||
          (s.description || '').toLowerCase().includes(searchQuery.toLowerCase())
        )
      : skills
  );

  async function runSkill(skillName) {
    try {
      const res = await fetch('/api/skills/run', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: skillName }),
      });
      if (res.ok) {
        alert(`Skill "${skillName}" started.`);
      } else {
        const err = await res.text();
        alert(`Failed to run skill: ${err}`);
      }
    } catch (e) {
      alert(`Error: ${e.message}`);
    }
  }

  async function learnFromPlan() {
    if (!learnPlanName.trim()) return;
    learnResult = null;
    try {
      const res = await fetch('/api/skills/learn', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ plan_name: learnPlanName.trim() }),
      });
      if (res.ok) {
        const data = await res.json();
        learnResult = { success: true, message: data.message || 'Skill learned successfully.' };
      } else {
        const err = await res.text();
        learnResult = { success: false, message: err };
      }
    } catch (e) {
      learnResult = { success: false, message: e.message };
    }
  }
</script>

<svelte:window on:load={fetchSkills} />

<div class="p-4">
  <div class="flex items-center justify-between mb-4">
    <h1 class="text-cyan-400 font-bold text-lg">Skills</h1>
    <button onclick={fetchSkills} class="text-xs bg-gray-800 hover:bg-gray-700 border border-gray-700 px-2 py-1 rounded font-mono">
      Refresh
    </button>
  </div>

  <!-- Search -->
  <div class="mb-4">
    <input
      bind:value={searchQuery}
      class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
      placeholder="Search skills..."
    />
  </div>

  <!-- Learn from Plan -->
  <div class="bg-gray-900 border border-gray-800 rounded-lg p-3 mb-4">
    <h2 class="text-gray-300 text-sm font-medium mb-2">Learn from Plan</h2>
    <div class="flex gap-2">
      <input
        bind:value={learnPlanName}
        class="flex-1 bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
        placeholder="Plan name..."
      />
      <button
        onclick={learnFromPlan}
        disabled={!learnPlanName.trim()}
        class="px-3 py-1.5 bg-yellow-700 rounded hover:bg-yellow-600 disabled:opacity-40 text-sm font-mono"
      >
        Learn
      </button>
    </div>
    {#if learnResult}
      <p class="mt-2 text-xs {learnResult.success ? 'text-green-400' : 'text-red-400'} font-mono">
        {learnResult.message}
      </p>
    {/if}
  </div>

  <!-- Skill list -->
  {#if loading}
    <div class="text-center py-12 text-gray-500">Loading skills...</div>
  {:else if filteredSkills.length === 0}
    <div class="text-center py-12 text-gray-500 text-sm">
      {searchQuery ? 'No skills match your search.' : 'No skills available.'}
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
      {#each filteredSkills as skill}
        <div class="bg-gray-900 border border-gray-800 rounded-lg p-4 hover:bg-gray-800 transition-colors">
          <div class="flex items-start justify-between mb-2">
            <h3 class="text-gray-100 font-medium text-sm">{skill.name || 'Unnamed'}</h3>
            <button
              onclick={() => runSkill(skill.name)}
              class="shrink-0 px-2 py-1 bg-cyan-700 rounded hover:bg-cyan-600 text-xs font-mono"
            >
              Run
            </button>
          </div>
          {#if skill.description}
            <p class="text-gray-500 text-xs">{skill.description}</p>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
