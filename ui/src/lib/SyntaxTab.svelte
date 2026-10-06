<script lang="ts">
  /**
   * Edit → LSPs → Languages: languages of the user's own, for file
   * extensions Thread has no grammar for.
   *
   * A language is lists of words and a few delimiters, with regular
   * expressions for whatever those cannot say. It is edited here against a
   * sample that is coloured as it will be, and saved into the config as a
   * `[syntax.<name>]` table: the same thing as writing that table by hand.
   */
  import { untrack } from "svelte";
  import { syntaxHighlighting } from "@codemirror/language";

  import { Compartment, EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";

  import {
    CATEGORIES,
    customLanguage,
    EMPTY_SYNTAX,
    patternError,
    type SyntaxDef,
    type SyntaxPattern,
  } from "./customSyntax";
  import type { ConfigStore } from "./state/config.svelte";
  import { syntaxTheme } from "./themes";

  type Props = { config: ConfigStore };
  let { config }: Props = $props();

  /** A language as it is typed: lists are text until they are saved. */
  type Draft = {
    /** The name it is saved under, or null for one not saved yet. */
    original: string | null;
    name: string;
    extensions: string;
    line: string;
    open: string;
    close: string;
    strings: string;
    keywords: string;
    types: string;
    constants: string;
    functions: string;
    patterns: SyntaxPattern[];
  };

  /** The word lists, each with the colour its words will be. */
  const LISTS = [
    { key: "keywords", label: "Keywords", colour: "#cba6f7", hint: "if else while return" },
    { key: "types", label: "Types", colour: "#f9e2af", hint: "int string bool" },
    { key: "constants", label: "Constants", colour: "#fab387", hint: "true false nil" },
    { key: "functions", label: "Functions", colour: "#89b4fa", hint: "print len" },
  ] as const;

  const saved = $derived(config.current.syntax);
  const names = $derived(Object.keys(saved));

  let draft = $state<Draft | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);

  const words = (text: string) => text.split(/[\s,]+/).filter(Boolean);

  function toDraft(name: string | null, def: SyntaxDef): Draft {
    return {
      original: name,
      name: name ?? "",
      extensions: def.extensions.join(", "),
      line: def.line_comment,
      open: def.block_comment[0] ?? "",
      close: def.block_comment[1] ?? "",
      strings: def.strings.join(" "),
      keywords: def.keywords.join(" "),
      types: def.types.join(" "),
      constants: def.constants.join(" "),
      functions: def.functions.join(" "),
      patterns: def.patterns.map((pattern) => ({ ...pattern })),
    };
  }

  function toDef(from: Draft): SyntaxDef {
    const open = from.open.trim();
    const close = from.close.trim();
    return {
      extensions: [...new Set(words(from.extensions).map((e) => e.replace(/^\.+/, "").toLowerCase()))],
      line_comment: from.line.trim(),
      block_comment: open && close ? [open, close] : [],
      strings: [...new Set(words(from.strings))],
      keywords: [...new Set(words(from.keywords))],
      types: [...new Set(words(from.types))],
      constants: [...new Set(words(from.constants))],
      functions: [...new Set(words(from.functions))],
      patterns: from.patterns.filter((pattern) => pattern.match !== ""),
    };
  }

  const def = $derived(draft ? toDef(draft) : null);

  /** What stands in the way of saving, or null if nothing does. */
  const problem = $derived.by(() => {
    if (!draft || !def) return null;
    const name = draft.name.trim();
    if (name === "") return "Give the language a name.";
    if (name !== draft.original && name in saved) return `There is already a language called ${name}.`;
    if (def.extensions.length === 0) return "Give it at least one file extension.";
    if ((draft.open.trim() === "") !== (draft.close.trim() === "")) {
      return "A block comment needs both what opens it and what closes it.";
    }
    if (def.patterns.some((pattern) => patternError(pattern.match) !== null)) {
      return "One of the patterns is not a regular expression.";
    }
    return null;
  });

  const dirty = $derived.by(() => {
    if (!draft || !def) return false;
    if (draft.original === null) return true;
    return (
      draft.name.trim() !== draft.original ||
      JSON.stringify(def) !== JSON.stringify(saved[draft.original])
    );
  });

  function select(name: string) {
    error = null;
    touched = false;
    draft = toDraft(name, saved[name]);
    sample = sampleFor(saved[name]);
  }

  function create() {
    error = null;
    touched = false;
    draft = toDraft(null, {
      ...EMPTY_SYNTAX,
      line_comment: "#",
      strings: ['"'],
    });
    sample = sampleFor(toDef(draft));
  }

  // Something to look at straight away: the first language there is.
  $effect(() => {
    if (draft === null && names.length > 0) select(names[0]);
  });

  async function save() {
    if (!draft || !def || problem) return;
    const name = draft.name.trim();
    saving = true;
    error = null;
    try {
      await config.setSyntax(name, draft.original !== name ? draft.original : null, def);
      draft = toDraft(name, config.current.syntax[name] ?? def);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!draft) return;
    const name = draft.original;
    try {
      // Gone from the config before it goes from here: the list is the
      // config's, and would otherwise offer it straight back.
      if (name !== null) await config.setSyntax(name, null, null);
      error = null;
      draft = null;
    } catch (e) {
      error = String(e);
    }
  }

  // --- the sample ---------------------------------------------------------------

  /** A few lines that use whatever the language has, to see it coloured. */
  function sampleFor(of: SyntaxDef): string {
    const quote = of.strings[0] ?? '"';
    const [open, close] = of.block_comment;
    const lines = [
      of.line_comment && `${of.line_comment} Type here to try the language out.`,
      `${of.keywords[0] ?? "let"} name = ${quote}some text${quote}`,
      `${of.types[0] ?? "value"} count = 42`,
      `${of.functions[0] ?? "call"}(name, ${of.constants[0] ?? "count"})`,
      open && close && `${open} A comment that can\n   run over lines. ${close}`,
    ];
    return lines.filter(Boolean).join("\n") + "\n";
  }

  let sample = "";
  /** Whether the sample has been typed into, and so is the user's to keep. */
  let touched = false;
  let previewHost = $state<HTMLElement | undefined>();
  let preview = $state.raw<EditorView | null>(null);
  const language = new Compartment();

  // One small editor for as long as there is a language to show, given a new
  // grammar as the language is edited. Its text follows the language too,
  // until it is typed into: after that it is whatever was typed.
  $effect(() => {
    if (!previewHost) return;
    // As they were when it was made. The dialog that changes them is not
    // this one, and making it again would lose what was typed into it.
    const { editor, theme } = untrack(() => config.current);
    const view = new EditorView({
      parent: previewHost,
      state: EditorState.create({
        doc: sample,
        extensions: [
          language.of([]),
          syntaxHighlighting(syntaxTheme(theme.syntax)),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.transactions.some((tr) => tr.isUserEvent("input") || tr.isUserEvent("delete"))) {
              touched = true;
            }
          }),
          EditorView.theme(
            {
              "&": { height: "100%", color: "var(--fg)", backgroundColor: "transparent" },
              "&.cm-focused": { outline: "none" },
              ".cm-scroller": { fontFamily: editor.font_family, lineHeight: "1.55" },
              ".cm-content": { padding: "8px 2px", caretColor: "var(--fg)" },
              ".cm-line": { padding: "0 10px" },
            },
            { dark: true },
          ),
        ],
      }),
    });
    preview = view;
    return () => {
      preview = null;
      view.destroy();
    };
  });

  $effect(() => {
    if (!preview || !def) return;
    if (!touched) {
      const text = sampleFor(def);
      if (text !== preview.state.doc.toString()) {
        preview.dispatch({ changes: { from: 0, to: preview.state.doc.length, insert: text } });
      }
    }
    // A pattern that is not one yet is left out by the tokeniser, so half
    // typed it costs nothing but itself.
    preview.dispatch({ effects: language.reconfigure(customLanguage(def)) });
  });
</script>

<div class="langs">
  <nav class="names" aria-label="Languages">
    {#each names as name (name)}
      <button
        class="name"
        class:on={draft?.original === name}
        onclick={() => select(name)}
      >
        <span class="title">{name}</span>
        <span class="exts">{saved[name].extensions.map((e) => `.${e}`).join(" ")}</span>
      </button>
    {/each}
    {#if draft && draft.original === null}
      <button class="name on">
        <span class="title">{draft.name.trim() || "New language"}</span>
        <span class="exts">not saved yet</span>
      </button>
    {/if}
    <button class="add" onclick={create} disabled={draft?.original === null}>+ New Language</button>
  </nav>

  {#if draft}
    <div class="form">
      <div class="pair">
        <label>
          <span class="cap">Name</span>
          <input type="text" bind:value={draft.name} placeholder="Mylang" spellcheck="false" />
        </label>
        <label>
          <span class="cap">File extensions</span>
          <input
            type="text"
            class="mono"
            bind:value={draft.extensions}
            placeholder="my, myl"
            spellcheck="false"
          />
        </label>
      </div>

      <div class="quad">
        <label>
          <span class="cap">Line comment</span>
          <input type="text" class="mono" bind:value={draft.line} placeholder="//" spellcheck="false" />
        </label>
        <label>
          <span class="cap">Block opens</span>
          <input type="text" class="mono" bind:value={draft.open} placeholder="/*" spellcheck="false" />
        </label>
        <label>
          <span class="cap">Block closes</span>
          <input type="text" class="mono" bind:value={draft.close} placeholder="*/" spellcheck="false" />
        </label>
        <label>
          <span class="cap">Strings</span>
          <input
            type="text"
            class="mono"
            bind:value={draft.strings}
            placeholder={`" '`}
            spellcheck="false"
          />
        </label>
      </div>

      {#each LISTS as list (list.key)}
        <label>
          <span class="cap">
            <span class="swatch" style:background={list.colour}></span>
            {list.label}
            <span class="count">{words(draft[list.key]).length || ""}</span>
          </span>
          <textarea
            class="mono"
            rows="2"
            bind:value={draft[list.key]}
            placeholder={list.hint}
            spellcheck="false"
          ></textarea>
        </label>
      {/each}

      <div class="patterns">
        <span class="cap">Patterns</span>
        <p class="note">
          Regular expressions for what the lists cannot say, tried where each token starts and
          ahead of the lists.
        </p>
        {#each draft.patterns as pattern, index (index)}
          {@const bad = pattern.match === "" ? null : patternError(pattern.match)}
          <div class="pattern">
            <input
              type="text"
              class="mono"
              class:bad
              bind:value={pattern.match}
              placeholder="@[a-z]+"
              title={bad ?? ""}
              spellcheck="false"
            />
            <select bind:value={pattern.as} aria-label="Colour as">
              {#each CATEGORIES as category (category)}
                <option value={category}>{category}</option>
              {/each}
            </select>
            <button
              class="drop"
              aria-label="Remove pattern"
              onclick={() => draft?.patterns.splice(index, 1)}
            >
              <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
                <path
                  d="M1.5 1.5 L8.5 8.5 M8.5 1.5 L1.5 8.5"
                  stroke="currentColor"
                  stroke-width="1.3"
                  stroke-linecap="round"
                />
              </svg>
            </button>
          </div>
        {/each}
        <button class="add inline" onclick={() => draft?.patterns.push({ match: "", as: "keyword" })}>
          + Add Pattern
        </button>
      </div>
    </div>

    <div class="side">
      <span class="cap">Preview</span>
      <div class="preview" bind:this={previewHost}></div>

      <p class="status" class:bad={error !== null || (dirty && problem !== null)}>
        {error ?? (dirty ? (problem ?? "Not saved yet.") : "Saved in the config file.")}
      </p>
      <div class="buttons">
        <button class="btn ghost danger-text" onclick={() => void remove()}>
          {draft.original === null ? "Discard" : "Delete"}
        </button>
        <button
          class="btn primary"
          disabled={!dirty || problem !== null || saving}
          onclick={() => void save()}
        >
          Save
        </button>
      </div>
    </div>
  {:else}
    <div class="empty">
      <p>No languages of your own yet.</p>
      <p class="note">
        Define one to colour files Thread has no grammar for: name it, give it its file
        extensions, and fill in its keywords, types and comments.
      </p>
      <button class="btn primary" onclick={create}>New Language</button>
    </div>
  {/if}
</div>

<style>
  .langs {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 0.9rem;
    height: 27rem;
  }

  /* The languages there are, down the left. */
  .names {
    flex: none;
    width: 9.5rem;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }
  .name {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    padding: 0.35rem 0.5rem;
    background: transparent;
    border: none;
    border-left: 2px solid transparent;
    border-radius: 0 var(--chip-radius) var(--chip-radius) 0;
    color: var(--fg-dim);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
  }
  .name:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .name.on {
    background: var(--accent-soft);
    border-left-color: var(--accent);
    color: var(--fg);
  }
  .title {
    font-size: 0.82rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .exts {
    font-size: 0.68rem;
    color: var(--fg-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .add {
    margin-top: 0.3rem;
    padding: 0.35rem 0.5rem;
    background: transparent;
    border: 1px dashed var(--border-input);
    border-radius: var(--chip-radius);
    color: var(--fg-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.76rem;
  }
  .add:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }
  .add:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .add.inline {
    align-self: flex-start;
    margin-top: 0.1rem;
  }

  /* What the language is: the part that scrolls. */
  .form {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
    overflow-y: auto;
    padding-right: 0.5rem;
  }
  .pair,
  .quad {
    display: grid;
    gap: 0.6rem;
  }
  .pair {
    grid-template-columns: 1fr 1fr;
  }
  .quad {
    grid-template-columns: repeat(4, 1fr);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .cap {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--fg-faint);
    font-size: 0.66rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .swatch {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .count {
    font-weight: 400;
    letter-spacing: 0;
  }

  input[type="text"],
  textarea,
  select {
    width: 100%;
    min-width: 0;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.45rem;
  }
  textarea {
    resize: vertical;
    min-height: 2.6rem;
    line-height: 1.45;
  }
  .mono {
    font-family: Consolas, monospace;
    font-size: 0.78rem;
  }
  input:focus-visible,
  textarea:focus-visible,
  select:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  input.bad {
    border-color: var(--danger);
  }
  ::placeholder {
    color: var(--fg-faint);
  }

  .patterns {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .note {
    margin: 0;
    color: var(--fg-dim);
    font-size: 0.71rem;
    line-height: 1.35;
  }
  .pattern {
    display: grid;
    grid-template-columns: 1fr 7rem auto;
    align-items: center;
    gap: 0.4rem;
  }
  .drop {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .drop:hover {
    background: #f38ba81f;
    color: var(--danger);
  }

  /* The sample, and under it whether what is on screen has been saved. */
  .side {
    flex: none;
    width: 17.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    min-height: 0;
  }
  .preview {
    flex: 1;
    min-height: 0;
    background: #0000003d;
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 0.8rem;
    overflow: hidden;
  }
  .status {
    margin: 0.2rem 0 0;
    min-height: 2.1em;
    color: var(--fg-dim);
    font-size: 0.71rem;
    line-height: 1.35;
  }
  .status.bad {
    color: var(--danger);
  }
  .buttons {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .danger-text:hover {
    border-color: var(--danger);
    color: var(--danger);
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.6rem;
    text-align: center;
  }
  .empty p {
    margin: 0;
    max-width: 24rem;
    font-size: 0.85rem;
  }
</style>
