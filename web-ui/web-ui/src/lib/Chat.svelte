<script>
  import { api, wsUrl, setToken, hasToken } from './api.js';

  let messages = $state([]);
  let input = $state('');
  let thinking = $state(false);
  let tokenIn = $state(0);
  let tokenOut = $state(0);
  let pendingApproval = $state(null);
  let ws = $state(null);
  let initialized = $state(false);
  let msgContainer = $state(null);
  let tokenInput = $state('');
  let wsError = $state('');

  // Auto-connect if token already saved
  if (hasToken()) {
    connect();
  }

  function connect() {
    wsError = '';
    const saved = localStorage.getItem('rupoo_token');
    if (saved) tokenInput = saved;

    if (!tokenInput.trim()) {
      wsError = 'Please enter your Web Panel token.';
      return;
    }
    setToken(tokenInput.trim());

    ws = new WebSocket(wsUrl('/ws/chat'));
    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data);
      switch (msg.type) {
        case 'message':
          messages = [...messages, { role: msg.role, content: msg.content }];
          break;
        case 'thinking':
          thinking = msg.status === 'thinking';
          break;
        case 'token_update':
          tokenIn = msg.in;
          tokenOut = msg.out;
          break;
        case 'approval':
          pendingApproval = { toolName: msg.tool_name, args: msg.args };
          break;
        case 'error':
          messages = [...messages, { role: 'system', content: `Error: ${msg.message}` }];
          thinking = false;
          break;
        case 'idle':
          thinking = false;
          pendingApproval = null;
          break;
        case 'plan_complete':
          thinking = false;
          break;
      }
    };
    ws.onopen = () => { initialized = true; wsError = ''; };
    ws.onclose = () => { initialized = false; };
    ws.onerror = () => { wsError = 'Connection failed. Is the server running?'; };
  }

  function disconnect() {
    if (ws) ws.close();
    initialized = false;
  }

  function send() {
    if (!input.trim() || !ws || !initialized) return;
    messages = [...messages, { role: 'user', content: input }];
    ws.send(JSON.stringify({ type: 'chat', content: input }));
    input = '';
  }

  function approve(all) {
    ws.send(JSON.stringify({ type: 'approve', choice: all ? 'all' : 'once' }));
    pendingApproval = null;
  }

  function deny() {
    ws.send(JSON.stringify({ type: 'deny' }));
    pendingApproval = null;
  }

  $effect(() => {
    if (msgContainer) {
      msgContainer.scrollTop = msgContainer.scrollHeight;
    }
  });
</script>

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="border-b border-gray-800 px-4 py-2 flex items-center justify-between">
    <h1 class="text-cyan-400 font-bold">Chat</h1>
    <div class="flex items-center gap-3">
      {#if initialized}
        <span class="text-xs text-green-400 font-mono">Connected</span>
        <button onclick={disconnect} class="text-xs bg-gray-700 px-2 py-1 rounded hover:bg-gray-600 font-mono">Disconnect</button>
      {:else}
        <span class="text-xs text-red-400 font-mono">Disconnected</span>
      {/if}
    </div>
  </div>

  <!-- Token input (shown only when not connected and no saved token) -->
  {#if !initialized && !localStorage.getItem('rupoo_token')}
    <div class="p-4 border-b border-gray-800 bg-gray-900/50">
      <p class="text-gray-400 text-sm mb-2 font-mono">Connect to your Rupoo assistant</p>
      <div class="flex gap-2 max-w-xl">
        <input
          bind:value={tokenInput}
          onkeydown={(e) => e.key === 'Enter' && connect()}
          class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
          placeholder="Paste your Web Panel token here..."
        />
        <button onclick={connect}
          class="px-4 py-2 bg-cyan-700 rounded hover:bg-cyan-600 text-sm font-mono"
        >
          Connect
        </button>
      </div>
      {#if wsError}
        <p class="text-red-400 text-xs mt-1 font-mono">{wsError}</p>
      {/if}
    </div>
  {:else if !initialized}
    <div class="p-4 border-b border-gray-800 bg-gray-900/50">
      <button onclick={connect} class="px-4 py-2 bg-cyan-700 rounded hover:bg-cyan-600 text-sm font-mono">
        Reconnect
      </button>
      {#if wsError}
        <span class="text-red-400 text-xs ml-2 font-mono">{wsError}</span>
      {/if}
    </div>
  {/if}

  function send() {
    if (!input.trim() || !ws || !initialized) return;
    messages = [...messages, { role: 'user', content: input }];
    ws.send(JSON.stringify({ type: 'chat', content: input }));
    input = '';
  }

  function approve(all) {
    ws.send(JSON.stringify({ type: 'approve', choice: all ? 'all' : 'once' }));
    pendingApproval = null;
  }

  function deny() {
    ws.send(JSON.stringify({ type: 'deny' }));
    pendingApproval = null;
  }

  $effect(() => {
    if (msgContainer) {
      msgContainer.scrollTop = msgContainer.scrollHeight;
    }
  });
</script>

<svelte:window on:load={() => { if (tokenInput) connect(); }} />

<div class="flex flex-col h-full">
  <!-- Header -->
  <div class="border-b border-gray-800 px-4 py-2 flex items-center justify-between">
    <h1 class="text-cyan-400 font-bold">Chat</h1>
    <div class="flex items-center gap-3">
      {#if !initialized}
        <span class="text-xs text-red-400 font-mono">Disconnected</span>
      {:else}
        <span class="text-xs text-green-400 font-mono">Connected</span>
      {/if}
    </div>
  </div>

  <!-- Token input (shown when not connected) -->
  {#if !initialized}
    <div class="p-4 border-b border-gray-800 bg-gray-900/50">
      <div class="flex gap-2 max-w-xl">
        <input
          bind:value={tokenInput}
          onkeydown={(e) => e.key === 'Enter' && connect()}
          class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
          placeholder="Paste your Web Panel token here..."
        />
        <button onclick={connect}
          class="px-4 py-2 bg-cyan-700 rounded hover:bg-cyan-600 text-sm font-mono"
        >
          Connect
        </button>
      </div>
      {#if wsError}
        <p class="text-red-400 text-xs mt-1 font-mono">{wsError}</p>
      {/if}
      <p class="text-gray-500 text-xs mt-1 font-mono">
        Token is printed when you run: rupoo serve --port 8080
      </p>
    </div>
  {/if}

  <!-- Messages -->
  <div class="flex-1 overflow-y-auto p-4 space-y-3" bind:this={msgContainer}>
    {#if messages.length === 0 && !thinking && initialized}
      <div class="flex items-center justify-center h-full">
        <div class="text-center text-gray-500">
          <p class="text-4xl mb-2">💬</p>
          <p class="text-sm">Start a conversation with your AI assistant.</p>
        </div>
      </div>
    {/if}

    {#each messages as msg}
      <div class="flex {msg.role === 'user' ? 'justify-end' : 'justify-start'}">
        <div class="max-w-2xl p-3 rounded-lg whitespace-pre-wrap text-sm
          {msg.role === 'user' ? 'bg-cyan-700 text-white' : msg.role === 'system' ? 'bg-yellow-900 text-yellow-100' : 'bg-gray-800 text-gray-100'}">
          {msg.content}
        </div>
      </div>
    {/each}

    {#if thinking}
      <div class="flex items-center gap-2 text-cyan-400">
        <span class="animate-spin inline-block text-lg">&#x27F3;</span>
        <span class="text-sm font-mono">Thinking...</span>
      </div>
    {/if}
  </div>

  <!-- Approval modal -->
  {#if pendingApproval}
    <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div class="bg-gray-900 border border-gray-700 p-6 rounded-lg max-w-lg w-full mx-4">
        <h3 class="text-cyan-400 font-bold mb-2">Tool Approval Required</h3>
        <p class="text-gray-300 text-sm mb-2">
          Tool: <span class="text-cyan-300 font-mono">{pendingApproval.toolName}</span>
        </p>
        <pre class="bg-gray-950 p-2 rounded text-xs mb-4 overflow-auto max-h-40 font-mono">{JSON.stringify(pendingApproval.args, null, 2)}</pre>
        <div class="flex gap-2 justify-end">
          <button onclick={deny} class="px-3 py-1.5 bg-red-800 rounded hover:bg-red-700 text-sm font-mono">Deny</button>
          <button onclick={() => approve(false)} class="px-3 py-1.5 bg-cyan-700 rounded hover:bg-cyan-600 text-sm font-mono">Approve Once</button>
          <button onclick={() => approve(true)} class="px-3 py-1.5 bg-green-800 rounded hover:bg-green-700 text-sm font-mono">Approve All</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Input -->
  <div class="border-t border-gray-800 p-3">
    <div class="flex gap-2">
      <input
        bind:value={input}
        onkeydown={(e) => e.key === 'Enter' && send()}
        class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-gray-100 text-sm font-mono outline-none focus:border-cyan-600"
        placeholder="Type a message..."
        disabled={thinking || !initialized}
      />
      <button
        onclick={send}
        disabled={thinking || !initialized}
        class="px-4 py-2 bg-cyan-700 rounded hover:bg-cyan-600 disabled:opacity-40 text-sm font-mono font-medium"
      >
        Send
      </button>
    </div>
    <div class="flex justify-end gap-3 mt-1 text-xs text-gray-500 font-mono">
      <span>Tokens: &uarr;{tokenIn} &darr;{tokenOut}</span>
    </div>
  </div>
</div>
