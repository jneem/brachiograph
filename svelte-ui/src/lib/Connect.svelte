<script lang="ts">
  import { emit, listen } from '@tauri-apps/api/event'
  import { invoke } from '@tauri-apps/api/tauri'

  // TODO: Figure out a better way of making the flash work
  let flash1 = true
  export function update() {
    flash1 = !flash1;
  }

  function clss(flash1: boolean) {
      if (flash1) {
        return "bold-flash1";
      } else {
        return "bold-flash2";
      }
  }

  listen('brachio-msg', (event) => {
    if (event.payload == 'Missing') {
      update();
    }
  })
</script>

<div>
  <button on:click={() => invoke('check_status')}>
    Connect
  </button>
  <span class={ clss(flash1) }>No brachiograph connected...</span>
</div>


<style>
@keyframes flash1 {
  0%    {color:black;}
  20%   {color:red;}
  100%  {color:black;}
}

@keyframes flash2 {
  0%    {color:black;}
  20%   {color:red;}
  100%  {color:black;}
}


.bold-flash1 {
  font-weight: bold;
  animation-name: flash1;
  animation-duration: 500ms;
}
.bold-flash2 {
  font-weight: bold;
  animation-name: flash2;
  animation-duration: 500ms;
}
</style>
