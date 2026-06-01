<script>
  import { api } from './api.js';

  let plans = $state([]);
  let selectedPlan = $state(null);
  let loading = $state(true);

  async function fetchPlans() {
    loading = true;
    try {
      const res = await api('/api/plans');
      if (res.ok) plans = await res.json();
    } catch (e) {
      console.error('Failed to fetch plans:', e);
      if (e.message === 'unauthorized') localStorage.removeItem('rupoo_token');
    } finally {
      loading = false;
    }
  }

  async function fetchPlanDetail(id) {
    try {
      const res = await api(`/api/plans/${id}`);
      if (res.ok) selectedPlan = await res.json();
    } catch (e) {
      console.error('Failed to fetch plan:', e);
    }
  }

  function statusColor(status) {
    const s = (status || '').toLowerCase();
    switch (s) {
      case 'completed': return 'bg-green-700 text-green-100';
      case 'failed': return 'bg-red-700 text-red-100';
      case 'running': return 'bg-blue-700 text-blue-100';
      case 'pending': return 'bg-yellow-700 text-yellow-100';
      default: return 'bg-gray-700 text-gray-100';
    }
  }

  function progressPercent(plan) {
    if (!plan.total_steps || plan.total_steps === 0) return 0;
    return Math.round((plan.current_step_index / plan.total_steps) * 100);
  }

  function back() {
    selectedPlan = null;
  }

  // Fetch on mount
  $effect(() => { fetchPlans(); });
</script>

<div class="p-4">
  <div class="flex items-center justify-between mb-4">
    <h1 class="text-cyan-400 font-bold text-lg">
      {#if selectedPlan}
        <button onclick={back} class="text-gray-400 hover:text-white mr-2">&larr;</button>
        Plan Detail
      {:else}
        Plans
      {/if}
    </h1>
    <button onclick={fetchPlans} class="text-xs bg-gray-800 hover:bg-gray-700 border border-gray-700 px-2 py-1 rounded font-mono">
      Refresh
    </button>
  </div>

  {#if loading}
    <div class="text-center py-12 text-gray-500">Loading plans...</div>
  {:else if selectedPlan}
    <!-- Detail view -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-4 mb-4">
      <h2 class="text-white font-bold text-base mb-1">{selectedPlan.name || selectedPlan.title}</h2>
      <p class="text-gray-400 text-xs mb-3">{selectedPlan.description || ''}</p>
      <span class="inline-block px-2 py-0.5 rounded text-xs {statusColor(selectedPlan.status)}">
        {selectedPlan.status || 'unknown'}
      </span>
    </div>

    {#if selectedPlan.steps && selectedPlan.steps.length > 0}
      <div class="space-y-2">
        {#each selectedPlan.steps as step, i}
          <div class="bg-gray-900 border border-gray-800 rounded-lg p-3 flex items-start gap-3">
            <div class="flex flex-col items-center">
              <div class="w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold
                {step.status === 'completed' ? 'bg-green-700 text-green-100' :
                 step.status === 'failed' ? 'bg-red-700 text-red-100' :
                 step.status === 'running' ? 'bg-blue-700 text-blue-100' :
                 'bg-gray-700 text-gray-400'}">
                {i + 1}
              </div>
              {#if i < selectedPlan.steps.length - 1}
                <div class="w-px h-full min-h-4 bg-gray-700"></div>
              {/if}
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <span class="text-gray-200 text-sm font-medium">{step.name || step.title}</span>
                <span class="px-1.5 py-0.5 rounded text-xs {statusColor(step.status)}">
                  {step.status || 'pending'}
                </span>
              </div>
              {#if step.description}
                <p class="text-gray-500 text-xs mt-1">{step.description}</p>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="text-center py-8 text-gray-500 text-sm">No steps for this plan.</div>
    {/if}
  {:else if plans.length === 0}
    <div class="text-center py-12 text-gray-500 text-sm">No plans yet.</div>
  {:else}
    <!-- Card grid -->
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
      {#each plans as plan}
        <button
          onclick={() => selectedPlan = plan}
          class="bg-gray-900 border border-gray-800 rounded-lg p-4 text-left hover:bg-gray-800 transition-colors"
        >
          <div class="flex items-center justify-between mb-2">
            <h3 class="text-gray-100 font-medium text-sm truncate">{plan.name || plan.title || 'Unnamed'}</h3>
            <span class="shrink-0 px-2 py-0.5 rounded text-xs {statusColor(plan.status)}">
              {plan.status || 'unknown'}
            </span>
          </div>
          {#if plan.description}
            <p class="text-gray-500 text-xs mb-2 line-clamp-2">{plan.description}</p>
          {/if}
          <div class="w-full bg-gray-800 rounded-full h-1.5">
            <div
              class="bg-cyan-600 h-1.5 rounded-full transition-all"
              style="width: {progressPercent(plan)}%"
            ></div>
          </div>
          <div class="text-right text-xs text-gray-500 mt-1">{progressPercent(plan)}%</div>
        </button>
      {/each}
    </div>
  {/if}
</div>
