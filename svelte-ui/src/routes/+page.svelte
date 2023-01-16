<script lang="ts">
  import Edit from '../lib/Edit.svelte'
  import Run from '../lib/Run.svelte'
  import Status from '../lib/Status.svelte'
  import Connect from '../lib/Connect.svelte'
  import { MsgKind } from '../lib/data'
  import { invoke } from '@tauri-apps/api/tauri'
  import { emit, listen } from '@tauri-apps/api/event'

  async function getStatus(): Promise<string> {
    return await invoke('brachio_status')
  }

  let text = "forward 10";
  let statusMsg = "";
  let statusKind = MsgKind.Info;
  let ready = false;

  listen('save', (event) => {
    const path: string = event.payload;
    invoke('write_file', { path, text })
  })

  listen('load', (event) => {
    console.log(text);
    text = event.payload;
    console.log(text);
  })

  listen('brachio-msg', (event) => {
    if (event.payload == 'Missing') {
      ready = false
    } else if (event.payload = 'Ready') {
      statusMsg = "Ready"
      ready = true
    }
  })
  invoke('check_status')
</script>

<div id="page">
  <Edit bind:text={text}/>
  {#if ready}
    <div>
      <Run
        code={text}
        on:runError={(e) => {
          console.log(e)
          if (e.detail == "Connection") {
            ready = false;
          }
        }}
      />
      <Status
        msg={statusMsg}
        msgKind={statusKind}
      />
    </div>
  {:else}
  <Connect />
  {/if}
</div>

<style>
#page {
  display: flex;
  flex-direction: column;
  justify-content: flex-start;
  align-items: stretch;
  height: 100%;
}

:global(body) {
  margin: 0;
  height: 100%;
}

:global(html) {
  margin: 0;
  height: 100%;
}
</style>