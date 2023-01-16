<script lang="ts">
  import { MsgKind } from './data'
  export let msgKind: MsgKind = MsgKind.Info
  export let msg = 'Status message'
</script>

<span class="info">{ msg }</span>

<style>
.info {
  color:gray;
}
</style>
