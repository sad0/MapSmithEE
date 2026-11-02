<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    --bg: #f4f2ee; --panel: #ffffff; --fg: #1d1b18; --dim: #6b665e; --line: #dcd7cf; --accent: #b5651d; --danger: #b3261e;
    color-scheme: light;
  }
  :global(:root[data-theme='dark']) {
    --bg: #17181b; --panel: #202226; --fg: #e8e6e3; --dim: #9a978f; --line: #34363b; --accent: #e09a4f; --danger: #ff8a80;
    color-scheme: dark;
  }
  :global(html, body) { margin: 0; height: 100%; background: var(--bg); color: var(--fg); font: 13px/1.4 system-ui, 'Segoe UI', sans-serif; overflow: hidden; }
  :global(button) { font: inherit; color: var(--fg); background: var(--panel); border: 1px solid var(--line); border-radius: 6px; padding: 4px 10px; cursor: pointer; }
  :global(button:hover:not(:disabled)) { border-color: var(--accent); }
  :global(button:disabled) { opacity: 0.45; cursor: default; }
  :global(button.on) { background: var(--accent); color: #fff; border-color: var(--accent); }
  :global(input, select) { font: inherit; color: var(--fg); background: var(--bg); border: 1px solid var(--line); border-radius: 5px; padding: 3px 6px; min-width: 0; }
  :global(input[type='checkbox'], input[type='range']) { padding: 0; }
</style>
