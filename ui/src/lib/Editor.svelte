<script lang="ts">
  import type { EditorHost } from "./editor";

  type Props = {
    host: EditorHost;
    /** The pane this is the editor of. */
    pane: number;
  };

  let { host, pane }: Props = $props();

  let el = $state<HTMLElement | undefined>();

  $effect(() => {
    if (!el) return;
    const id = pane;
    host.mount(id, el);
    return () => host.unmount(id);
  });
</script>

<div class="editor" bind:this={el}></div>

<style>
  .editor {
    position: absolute;
    inset: 0;
  }
</style>
