<script lang="ts">
  /**
   * One pane of the stage: a strip of tabs, and under it the editor showing
   * whichever file's tab is in front.
   *
   * A terminal's tab is in the strip like any other, but the terminal itself
   * is not in here: App lays it over this pane's body, so that it is the same
   * terminal whichever pane its tab is dragged to.
   */
  import Editor from "./Editor.svelte";
  import type { Leaf, SplitDir } from "./panes";
  import type { EditorConfig } from "./state/config.svelte";
  import type { Documents } from "./state/documents.svelte";
  import TabBar, { type Tab } from "./TabBar.svelte";

  type Props = {
    leaf: Leaf;
    /** Where in the stage it goes, as CSS. */
    style: string;
    /** Whether this is the pane the keyboard is in. */
    focused: boolean;
    tabs: Tab[];
    /** Whether there is another pane, and so a point in closing this one. */
    closable: boolean;
    docs: Documents;
    /** For the font vim's command line is set in: the editor's. */
    editor: EditorConfig;
    /** The pointer or the keyboard has come to this pane. */
    onfocus: () => void;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    onmove: (key: number, to: number, index: number) => void;
    oncontext: (event: MouseEvent, key: number) => void;
    onsplit: (dir: SplitDir) => void;
    onclosepane: () => void;
  };

  let {
    leaf,
    style,
    focused,
    tabs,
    closable,
    docs,
    editor,
    onfocus,
    onselect,
    onclose,
    onmove,
    oncontext,
    onsplit,
    onclosepane,
  }: Props = $props();

  /** The file in front, or null while a terminal is, or nothing. */
  const file = $derived(leaf.active !== null && leaf.active > 0 ? leaf.active : null);
  const onTerminal = $derived(leaf.active !== null && leaf.active < 0);

  // Coming to a file's tab, or to this pane with a file in front, is asking
  // to type in it.
  $effect(() => {
    docs.editor.show(leaf.id, file);
    if (focused && file !== null) docs.editor.focus(leaf.id);
  });

  let vimLine = $state<HTMLElement | undefined>();

  $effect(() => {
    const id = leaf.id;
    docs.vimLines[id] = vimLine ?? null;
    return () => {
      delete docs.vimLines[id];
      delete docs.cursors[id];
      delete docs.vimModes[id];
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pane" data-pane={leaf.id} {style} onfocusin={onfocus} onpointerdowncapture={onfocus}>
  <!-- No strip with nothing open: an empty bar across the top of an empty
       editor is a line with no reason to be there. -->
  {#if tabs.length > 0}
    <TabBar
      pane={leaf.id}
      {focused}
      {tabs}
      activeKey={leaf.active}
      {closable}
      {onselect}
      {onclose}
      {onmove}
      {oncontext}
      {onsplit}
      {onclosepane}
    />
  {/if}

  <!-- Hidden, not removed, behind a terminal: the editor keeps its scroll
       position and its measurements. -->
  <div class="body" class:hidden={onTerminal}>
    <div class="text">
      <Editor host={docs.editor} pane={leaf.id} />
    </div>
    <!-- Vim's `:` line, `/` search and messages are put here by `vim.ts`:
         one line of the editor, in the editor's own font and at its line
         height, that takes the place of the last line on screen for as long
         as it has something in it. Always present, so there is somewhere to
         put them the moment vim asks. -->
    <div
      class="vim-line"
      data-vim-line
      bind:this={vimLine}
      style:font-family={editor.font_family}
      style:font-size="{editor.font_size}px"
      style:height="{editor.font_size * editor.line_height}px"
    ></div>
  </div>
</div>

<style>
  .pane {
    position: absolute;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  /* The text, and under it vim's line when it has something to say: the
     text gives up that much height and gets it back. */
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .body.hidden {
    visibility: hidden;
  }
  .text {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  /* Vim's command line: a line across the foot of the editor, as vim has it.
     It is set as a line of the file is (the font and the height come from the
     config, inline), and the text above gives up exactly that much, so it
     reads as the last line on screen having been swapped for it. Nothing is
     drawn for it, no surface and no rule. Not there at all while it is empty.
     What goes in it is built by the vim extension, not by this component, so
     it is reached with `:global`. */
  .vim-line {
    flex: none;
    box-sizing: content-box;
    display: flex;
    align-items: center;
    /* In line with the line numbers above it, which also clears the card's
       rounded corner. */
    padding: 0 14px;
    color: var(--fg);
  }
  .vim-line:empty {
    display: none;
  }
  /* The extension's panel fills the bar, so the field in it has the width. */
  .vim-line :global(> *) {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
  }
  /* Everything the extension puts in here takes the window's own font and
     colour, over the monospace and the hard red it asks for inline. */
  .vim-line :global(*) {
    color: inherit !important;
    font-family: inherit !important;
    font-size: inherit !important;
  }
  .vim-line :global(input) {
    flex: 1;
    min-width: 0;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
  }
  .vim-line :global(.cm-vim-message) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
