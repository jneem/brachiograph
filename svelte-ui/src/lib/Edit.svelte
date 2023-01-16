<script>
  export let text = "";
</script>

<div
  contenteditable=true
  spellcheck=false
>
{text}
</div>

<style>
div {
  border: 1px solid black;
  margin: 10px;
  padding: 10px;
  overflow-y: auto;
  outline: 1;
  flex-grow: 1;
}
</style>
