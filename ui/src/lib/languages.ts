/**
 * The language a file is written in, as the status bar names it.
 *
 * By file name only. Nothing here drives behaviour yet — there is no
 * highlighting or completion to pick — so this is a label, and a file it does
 * not recognise is plain text.
 */

/** Whole file names, lower-case. Checked before the extension. */
const BY_NAME: Record<string, string> = {
  dockerfile: "Dockerfile",
  makefile: "Makefile",
  "cmakelists.txt": "CMake",
  "cargo.lock": "TOML",
  ".gitignore": "Ignore",
  ".gitattributes": "Git Attributes",
  ".editorconfig": "EditorConfig",
  ".bashrc": "Shell Script",
  ".zshrc": "Shell Script",
  ".profile": "Shell Script",
};

/** Final extensions, lower-case, without the dot. */
const BY_EXTENSION: Record<string, string> = {
  asm: "Assembly",
  s: "Assembly",
  bat: "Batch",
  cmd: "Batch",
  c: "C",
  h: "C",
  cs: "C#",
  cc: "C++",
  cpp: "C++",
  cxx: "C++",
  hh: "C++",
  hpp: "C++",
  hxx: "C++",
  cmake: "CMake",
  css: "CSS",
  csv: "CSV",
  dart: "Dart",
  diff: "Diff",
  patch: "Diff",
  ex: "Elixir",
  exs: "Elixir",
  glsl: "GLSL",
  go: "Go",
  graphql: "GraphQL",
  gql: "GraphQL",
  hs: "Haskell",
  htm: "HTML",
  html: "HTML",
  cfg: "INI",
  conf: "INI",
  ini: "INI",
  java: "Java",
  cjs: "JavaScript",
  js: "JavaScript",
  mjs: "JavaScript",
  jsx: "JavaScript JSX",
  json: "JSON",
  jsonc: "JSON with Comments",
  ipynb: "Jupyter Notebook",
  kt: "Kotlin",
  kts: "Kotlin",
  tex: "LaTeX",
  less: "Less",
  ld: "Linker Script",
  log: "Log",
  lua: "Lua",
  markdown: "Markdown",
  md: "Markdown",
  nix: "Nix",
  ml: "OCaml",
  pl: "Perl",
  php: "PHP",
  txt: "Plain Text",
  ps1: "PowerShell",
  psm1: "PowerShell",
  proto: "Protocol Buffers",
  py: "Python",
  pyi: "Python",
  r: "R",
  rb: "Ruby",
  rs: "Rust",
  scala: "Scala",
  scss: "SCSS",
  bash: "Shell Script",
  sh: "Shell Script",
  zsh: "Shell Script",
  sql: "SQL",
  svelte: "Svelte",
  svg: "SVG",
  swift: "Swift",
  tf: "Terraform",
  toml: "TOML",
  cts: "TypeScript",
  mts: "TypeScript",
  ts: "TypeScript",
  tsx: "TypeScript JSX",
  vim: "Vim Script",
  vue: "Vue",
  wgsl: "WGSL",
  xml: "XML",
  yaml: "YAML",
  yml: "YAML",
  zig: "Zig",
};

export const PLAIN_TEXT = "Plain Text";

export function languageOf(fileName: string): string {
  const lower = fileName.toLowerCase();
  const named = BY_NAME[lower];
  if (named) return named;

  const dot = lower.lastIndexOf(".");
  return (dot === -1 ? undefined : BY_EXTENSION[lower.slice(dot + 1)]) ?? PLAIN_TEXT;
}
