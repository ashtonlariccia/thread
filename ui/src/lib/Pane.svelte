<script lang="ts">
  /**
   * One pane of the stage: a strip of tabs, under it the editor showing
   * whichever file's tab is in front, and at its foot a line about that
   * file, as each of vim's windows has.
   *
   * A terminal's tab is in the strip like any other, but the terminal itself
   * is not in here: App lays it over this pane's body, so that it is the same
   * terminal whichever pane its tab is dragged to.
   */
  import Editor from "./Editor.svelte";
  import { indentLabel } from "./indent";
  import { languageOf } from "./languages";
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
    /** Counts the configs taken on: what a file is called can change with one. */
    configured: number;
    /** The pointer or the keyboard has come to this pane. */
    onfocus: () => void;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    onmove: (key: number, to: number, index: number) => void;
    oncontext: (event: MouseEvent, key: number) => void;
    onsplit: (dir: SplitDir) => void;
    onlock: () => void;
    onclosepane: () => void;
    /** Right-click in the editor. App owns the menu, so only one is ever open. */
    oneditorcontext: (event: MouseEvent) => void;
  };

  let {
    leaf,
    style,
    focused,
    tabs,
    closable,
    docs,
    editor,
    configured,
    onfocus,
    onselect,
    onclose,
    onmove,
    oncontext,
    onsplit,
    onlock,
    onclosepane,
    oneditorcontext,
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

  // What the line at the foot says: about the file in front, and where
  // this pane's cursor is in it.
  const doc = $derived(docs.find(file));
  const cursor = $derived(docs.cursors[leaf.id] ?? { line: 1, col: 1 });
  const mode = $derived(docs.vimOn ? (docs.vimModes[leaf.id] ?? "normal") : null);

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
      locked={leaf.locked ?? false}
      {onselect}
      {onclose}
      {onmove}
      {oncontext}
      {onsplit}
      {onlock}
      {onclosepane}
    />
  {/if}

  <!-- Hidden, not removed, behind a terminal: the editor keeps its scroll
       position and its measurements. -->
  <div class="body" class:hidden={onTerminal}>
    <!-- With no file in front there is no editor to have a menu, and the
         click is the window's. -->
    <div
      class="text"
      oncontextmenu={(e) => {
        if (file === null) return;
        e.preventDefault();
        e.stopPropagation();
        oneditorcontext(e);
      }}
    >
      <Editor host={docs.editor} pane={leaf.id} />
    </div>
    <!-- The line about the file: the mode on the left, as vim has it, and
         on the right where the cursor is and what the file is. It is the
         last line of the pane until a command is typed, when it moves up
         one to make room underneath. What rarely changes is first to go
         as the pane narrows. -->
    {#if doc}
      <div
        class="status"
        class:focused
        style:font-family={editor.font_family}
        style:font-size="{editor.font_size}px"
        style:height="{editor.font_size * editor.line_height}px"
      >
        {#if mode}
          <span class="mode" data-mode={mode}>{mode}</span>
        {/if}
        <span class="fill"></span>
        <!-- What a file is can change with the config, which names the
             user's own languages; `languageOf` itself is not watched. -->
        <span class="extra">{(void configured, languageOf(doc.name))}</span>
        <span class="extra">{doc.bom ? "UTF-8 with BOM" : "UTF-8"}</span>
        <span class="extra">{doc.eol === "crlf" ? "CRLF" : "LF"}</span>
        <span class="extra">{indentLabel(doc.indent)}</span>
        <span>Ln {cursor.line}, Col {cursor.col}</span>
      </div>
    {/if}
    <!-- Vim's `:` line, `/` search and messages are put here by `vim.ts`:
         one line of the editor, in the editor's own font and at its line
         height, that opens under the line above for as long as it has
         something in it. Always present, so there is somewhere to put them
         the moment vim asks. -->
    <div
      class="vim-line"
      data-vim-line
      bind:this={vimLine}
      style:font-family={editor.font_family}
      style:font-size="{editor.font_size}px"
      style:--line="{editor.font_size * editor.line_height}px"
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

  /* The text, under it the line about the file, and under that vim's line
     when it has something to say: the text gives up that much height and
     gets it back. */
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

  /* The line about the file, set as a line of the file is: the font and the
     height come from the config, inline. All of it in one colour, quieter
     than the text, and quieter again in a pane the keyboard is not in. */
  .status {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2ch;
    /* In line with the line numbers above it, which also clears the card's
       rounded corner. */
    padding: 0 14px;
    overflow: hidden;
    color: var(--fg-dim);
    white-space: nowrap;
    user-select: none;
    container-type: inline-size;
  }
  .status:not(.focused) {
    opacity: 0.6;
  }
  .status .fill {
    flex: 1;
  }
  /* Named the way the rest is: "Normal", not vim's shouted "NORMAL". */
  .mode {
    text-transform: capitalize;
  }
  /* A narrow pane keeps the mode and where the cursor is. */
  @container (max-width: 34rem) {
    .extra {
      display: none;
    }
  }

  /* Vim's command line: the last line of the pane, as vim has it, opening
     under the line about the file, which slides up to make room. It is set
     as a line of the file is (the font and the height come from the config,
     inline), and the text above gives up exactly that much. Nothing is drawn
     for it, no surface and no rule. No height at all while it is empty. What
     goes in it is built by the vim extension, not by this component, so it
     is reached with `:global`. */
  .vim-line {
    flex: none;
    box-sizing: content-box;
    display: flex;
    align-items: center;
    height: var(--line);
    overflow: hidden;
    /* In line with the line numbers above it, which also clears the card's
       rounded corner. */
    padding: 0 14px;
    /* The one colour of the line above it, what is typed here included. */
    color: var(--fg-dim);
    transition: height 110ms ease-out;
  }
  .vim-line:empty {
    height: 0;
  }
  @media (prefers-reduced-motion: reduce) {
    .vim-line {
      transition: none;
    }
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
