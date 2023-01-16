<script>
  import { invoke } from '@tauri-apps/api/tauri'

  let name = ''
  let greetMsg = ''
  let status = ''

  async function greet() {
    greetMsg = await invoke('greet', { name })
  }

  async function getStatus() {
    return await invoke('brachio_status')
  }

  getStatus().then(s => status = s)
</script>

<div>
  <input id="greet-input" placeholder="Enter a name..." bind:value="{name}" />
  <button on:click="{greet}">Greet</button>
  <p>{greetMsg}</p>
  <p>{status}</p>
</div>
