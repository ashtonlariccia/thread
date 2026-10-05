<script lang="ts">
  import type { EditorHost } from "./editor";

  type Props = {
    host: EditorHost;
  };

  let { host }: Props = $props();

  let el = $state<HTMLElement | undefined>();

  $effect(() => {
    if (!el) return;
    host.mount(el);
    return () => host.unmount();
  });
</script>

<div class="editor" bind:this={el}></div>

<style>
  .editor {
    position: absolute;
    inset: 0;
  }
</style>
