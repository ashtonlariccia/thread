// Import a VS Code file-icon theme into Thread.
//
//   node scripts/import-icons.mjs [path-to-theme-extension]
//
// Defaults to the Material Icon Theme (Charcoal) fork under ~/.vscode. Copies
// every SVG the dark theme can resolve to into ui/public/icons, and boils the
// theme's JSON down to the lookups Thread actually does. The result is
// committed, so this only needs re-running when the pack changes.
import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function defaultTheme() {
  const extensions = join(homedir(), ".vscode", "extensions");
  const found = readdirSync(extensions)
    .filter((name) => name.startsWith("local.material-icon-theme-charcoal-"))
    .sort()
    .at(-1);
  if (!found) throw new Error(`no material-icon-theme-charcoal under ${extensions}`);
  return join(extensions, found);
}

const themeDir = resolve(process.argv[2] ?? defaultTheme());
const manifest = JSON.parse(readFileSync(join(themeDir, "package.json"), "utf8"));
const themePath = join(themeDir, manifest.contributes.iconThemes[0].path);
const theme = JSON.parse(readFileSync(themePath, "utf8"));

const iconsOut = join(root, "ui", "public", "icons");
const mapsOut = join(root, "ui", "src", "lib", "icons");
rmSync(iconsOut, { recursive: true, force: true });
mkdirSync(iconsOut, { recursive: true });
mkdirSync(mapsOut, { recursive: true });

// Keys are matched case-insensitively at lookup time.
const lower = (map) => Object.fromEntries(Object.entries(map).map(([k, v]) => [k.toLowerCase(), v]));

const files = {
  file: theme.file,
  extensions: lower(theme.fileExtensions),
  names: lower(theme.fileNames),
};
const folders = {
  folder: theme.folder,
  folderOpen: theme.folderExpanded,
  root: theme.rootFolder,
  rootOpen: theme.rootFolderExpanded,
  names: lower(theme.folderNames),
  namesOpen: lower(theme.folderNamesExpanded),
};

// Only the dark theme's icons: the `light` and `highContrast` variants are
// never shown, and they are a tenth of the pack.
const used = new Set([
  files.file,
  ...Object.values(files.extensions),
  ...Object.values(files.names),
  folders.folder,
  folders.folderOpen,
  folders.root,
  folders.rootOpen,
  ...Object.values(folders.names),
  ...Object.values(folders.namesOpen),
]);

for (const id of used) {
  const definition = theme.iconDefinitions[id];
  if (!definition) throw new Error(`theme maps to "${id}" but does not define it`);
  cpSync(join(dirname(themePath), definition.iconPath), join(iconsOut, `${id}.svg`));
}

writeFileSync(join(mapsOut, "files.json"), JSON.stringify(files));
writeFileSync(join(mapsOut, "folders.json"), JSON.stringify(folders));

console.log(`imported ${used.size} icons from ${themeDir}`);
