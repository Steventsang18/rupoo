<script>
  let { page, onNavigate } = $props();

  const navItems = [
    { id: 'chat', icon: '💬', label: 'Chat' },
    { id: 'plans', icon: '📋', label: 'Plans' },
    { id: 'skills', icon: '🧠', label: 'Skills' },
    { id: 'memory', icon: '💾', label: 'Memory' },
    { id: 'config', icon: '⚙️', label: 'Config' },
    { id: 'status', icon: '📊', label: 'Status' },
  ];
</script>

<nav class="w-16 bg-gray-900 border-r border-gray-800 flex flex-col items-center py-4 gap-1 shrink-0">
  {#each navItems as item}
    <button
      onclick={() => onNavigate(item.id)}
      class="w-12 h-12 flex items-center justify-center rounded-lg text-lg transition-colors
        {page === item.id
          ? 'bg-cyan-700/30 text-cyan-400'
          : 'text-gray-500 hover:text-gray-300 hover:bg-gray-800'}"
      title={item.label}
    >
      <span class="text-xl">{item.icon}</span>
    </button>
  {/each}
</nav>
