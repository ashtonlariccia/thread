/**
 * Grammars, by language name.
 *
 * Each one is fetched the first time a file in that language is opened, and
 * never before: a window that only ever edits Rust never downloads the PHP
 * parser. The names are the ones `languages.ts` gives files, so the bottom
 * bar and the highlighter cannot disagree about what a file is.
 *
 * Two kinds of grammar. Where CodeMirror has a proper parser for a language
 * (`@codemirror/lang-*`) that is used. The rest come from its "legacy modes" —
 * simpler tokenisers ported from CodeMirror 5 — which colour well enough and
 * cover a great many more languages.
 */
import type { Extension } from "@codemirror/state";
import type { StreamParser } from "@codemirror/language";

import { customLanguage, type SyntaxDef } from "./customSyntax";
import { setCustomLanguages } from "./languages";


/** Wrap a legacy tokeniser as a language. */
async function legacy(mode: Promise<StreamParser<unknown>>): Promise<Extension> {
  const { StreamLanguage } = await import("@codemirror/language");
  return StreamLanguage.define(await mode);
}

const clike = () => import("@codemirror/legacy-modes/mode/clike");
const javascript = () => import("@codemirror/lang-javascript");

const LOADERS: Record<string, () => Promise<Extension>> = {
  Assembly: () => legacy(import("@codemirror/legacy-modes/mode/gas").then((m) => m.gas)),
  C: async () => (await import("@codemirror/lang-cpp")).cpp(),
  "C#": () => legacy(clike().then((m) => m.csharp)),
  "C++": async () => (await import("@codemirror/lang-cpp")).cpp(),
  CMake: () => legacy(import("@codemirror/legacy-modes/mode/cmake").then((m) => m.cmake)),
  CSS: async () => (await import("@codemirror/lang-css")).css(),
  Dart: () => legacy(clike().then((m) => m.dart)),
  Diff: () => legacy(import("@codemirror/legacy-modes/mode/diff").then((m) => m.diff)),
  Dockerfile: () =>
    legacy(import("@codemirror/legacy-modes/mode/dockerfile").then((m) => m.dockerFile)),
  GLSL: () => legacy(clike().then((m) => m.shader)),
  Go: async () => (await import("@codemirror/lang-go")).go(),
  Haskell: () => legacy(import("@codemirror/legacy-modes/mode/haskell").then((m) => m.haskell)),
  HTML: async () => (await import("@codemirror/lang-html")).html(),
  INI: () => legacy(import("@codemirror/legacy-modes/mode/properties").then((m) => m.properties)),
  Java: async () => (await import("@codemirror/lang-java")).java(),
  JavaScript: async () => (await javascript()).javascript(),
  "JavaScript JSX": async () => (await javascript()).javascript({ jsx: true }),
  JSON: async () => (await import("@codemirror/lang-json")).json(),
  // No grammar of its own; the comments just come out as errors-in-waiting,
  // which the JSON grammar tolerates and colours as plain text.
  "JSON with Comments": async () => (await import("@codemirror/lang-json")).json(),
  "Jupyter Notebook": async () => (await import("@codemirror/lang-json")).json(),
  Kotlin: () => legacy(clike().then((m) => m.kotlin)),
  LaTeX: () => legacy(import("@codemirror/legacy-modes/mode/stex").then((m) => m.stex)),
  Less: async () => (await import("@codemirror/lang-less")).less(),
  Lua: () => legacy(import("@codemirror/legacy-modes/mode/lua").then((m) => m.lua)),
  Markdown: async () => (await import("@codemirror/lang-markdown")).markdown(),
  OCaml: () => legacy(import("@codemirror/legacy-modes/mode/mllike").then((m) => m.oCaml)),
  Perl: () => legacy(import("@codemirror/legacy-modes/mode/perl").then((m) => m.perl)),
  PHP: async () => (await import("@codemirror/lang-php")).php(),
  PowerShell: () =>
    legacy(import("@codemirror/legacy-modes/mode/powershell").then((m) => m.powerShell)),
  "Protocol Buffers": () =>
    legacy(import("@codemirror/legacy-modes/mode/protobuf").then((m) => m.protobuf)),
  Python: async () => (await import("@codemirror/lang-python")).python(),
  R: () => legacy(import("@codemirror/legacy-modes/mode/r").then((m) => m.r)),
  Ruby: () => legacy(import("@codemirror/legacy-modes/mode/ruby").then((m) => m.ruby)),
  Rust: async () => (await import("@codemirror/lang-rust")).rust(),
  Scala: () => legacy(clike().then((m) => m.scala)),
  SCSS: async () => (await import("@codemirror/lang-sass")).sass(),
  "Shell Script": () => legacy(import("@codemirror/legacy-modes/mode/shell").then((m) => m.shell)),
  SQL: async () => (await import("@codemirror/lang-sql")).sql(),
  Svelte: async () => (await import("@replit/codemirror-lang-svelte")).svelte(),
  SVG: async () => (await import("@codemirror/lang-xml")).xml(),
  Swift: () => legacy(import("@codemirror/legacy-modes/mode/swift").then((m) => m.swift)),
  TOML: () => legacy(import("@codemirror/legacy-modes/mode/toml").then((m) => m.toml)),
  TypeScript: async () => (await javascript()).javascript({ typescript: true }),
  "TypeScript JSX": async () => (await javascript()).javascript({ typescript: true, jsx: true }),
  Vue: async () => (await import("@codemirror/lang-vue")).vue(),
  XML: async () => (await import("@codemirror/lang-xml")).xml(),
  YAML: async () => (await import("@codemirror/lang-yaml")).yaml(),
};

const loaded = new Map<string, Promise<Extension | null>>();

/** The user's own languages, by name, as the config describes them. */
let custom: Record<string, SyntaxDef> = {};

/**
 * Take on the user's languages (`[syntax.*]` in the config). They come ahead
 * of the grammars above, for the extensions they claim and by name.
 */
export function setCustomSyntax(defs: Record<string, SyntaxDef>) {
  custom = defs;
  setCustomLanguages(
    Object.fromEntries(Object.entries(defs).map(([name, def]) => [name, def.extensions])),
  );
}

/**
 * What a language's grammar is built from, as text, where it is one of the
 * user's: when this changes the grammar has, and wants loading again.
 */
export function syntaxRevision(language: string): string {
  return language in custom ? JSON.stringify(custom[language]) : "";
}

/** Whether a language has a grammar at all. */
export function hasSyntax(language: string): boolean {
  return language in custom || language in LOADERS;
}

/**
 * The grammar for a language, or null if there is none or it would not load.
 * Never rejects: a file with no grammar is plain text, not a failure.
 */
export function loadSyntax(language: string): Promise<Extension | null> {
  // Built here and now, and not kept: it is small, and it changes whenever
  // its description does.
  if (language in custom) return Promise.resolve(customLanguage(custom[language]));

  let pending = loaded.get(language);
  if (!pending) {
    const loader = LOADERS[language];
    pending = loader
      ? loader().catch((e) => {
          console.error(`loading the ${language} grammar failed`, e);
          // Let a later file in this language try again.
          loaded.delete(language);
          return null;
        })
      : Promise.resolve(null);
    loaded.set(language, pending);
  }
  return pending;
}
