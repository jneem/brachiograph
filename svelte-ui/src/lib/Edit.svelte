<script>
  export let text = "";
</script>

<!-- it would be nice to use a contenteditable div (so that we can style spans), but innerText
is not bindable, and newline handling with textContent is weird -->
<textarea
  bind:value={text}
  spellcheck=false
/>

<style>
textarea {
  border: 1px solid black;
  margin: 10px;
  padding: 10px;
  overflow-y: auto;
  outline: 1;
  flex-grow: 1;
}
</style>
