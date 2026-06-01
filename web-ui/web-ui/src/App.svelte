<script>
  import Sidebar from './lib/Sidebar.svelte';
  import Chat from './lib/Chat.svelte';
  import Plans from './lib/Plans.svelte';
  import Skills from './lib/Skills.svelte';
  import Memory from './lib/Memory.svelte';
  import Config from './lib/Config.svelte';
  import Status from './lib/Status.svelte';
  import './app.css';

  let page = $state('chat');
</script>

<div class="flex h-screen font-mono">
  <Sidebar {page} onNavigate={(p) => page = p} />
  <main class="flex-1 overflow-auto bg-gray-950">
    {#if page === 'chat'}
      <Chat />
    {:else if page === 'plans'}
      <Plans />
    {:else if page === 'skills'}
      <Skills />
    {:else if page === 'memory'}
      <Memory />
    {:else if page === 'config'}
      <Config />
    {:else if page === 'status'}
      <Status />
    {/if}
  </main>
</div>
