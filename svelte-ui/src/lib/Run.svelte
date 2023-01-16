<script lang="ts">
  import { invoke } from '@tauri-apps/api/tauri'
  import { createEventDispatcher } from 'svelte';

  const dispatch = createEventDispatcher();

  async function run(code: string) {
    await invoke('run', { code })
      .then(() => dispatch('runStarted'))
      .catch(e => dispatch('runError', e))
  }

  export let code: string
</script>

<button on:click={() => run(code)}>Run</button>
