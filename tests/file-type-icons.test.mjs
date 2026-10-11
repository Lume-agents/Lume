import assert from "node:assert/strict";
import { registerHooks } from "node:module";

// The icon packages are published as ESM files without the extension in their import paths.
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier.startsWith("@iconify-icons/") && !specifier.endsWith(".js")) return nextResolve(`${specifier}.js`, context);
    return nextResolve(specifier, context);
  },
});

const { fileTypeIconForPath, renderFileTypeIconHtml } = await import("../src/lib/fileTypeIcons.ts");

const glyph = (path) => fileTypeIconForPath(path).glyph;
const brand = (path) => Boolean(fileTypeIconForPath(path).icon || fileTypeIconForPath(path).maintainedPath);

// Files without a logo of their own get a Lume glyph; the same glyph for the same kind of file.
assert.ok(glyph("tests/handoff.test.mjs"), ".mjs is an experiment flask");
assert.equal(glyph("a.mjs"), glyph("notebook.ipynb"));
assert.ok(glyph("settings.toml") && glyph("app.ini") && glyph(".editorconfig"));
assert.equal(glyph("settings.toml"), glyph("app.cfg"), "configuration files share a glyph");
assert.ok(glyph("something.lock"), "an unnamed lock file gets a padlock");
assert.ok(glyph(".env") && glyph(".env.production") && glyph("cert.pem"));
assert.ok(glyph("data.csv") && glyph("db.sqlite") && glyph("notes.txt") && glyph("run.ps1") && glyph("a.xml"));
assert.ok(glyph(".gitignore") && glyph("bundle.zip") && glyph("fix.patch") && glyph("photo.png") && glyph("schema.proto"));
const distinct = new Set(["a.mjs", "a.toml", "a.lock", "a.env", "a.csv", "a.sql", "a.txt", "a.bat", "a.xml", ".gitignore", "a.zip", "a.patch", "a.png", "a.proto"].map(glyph));
assert.equal(distinct.size, 14, "each kind has its own drawing");

// A glyph replaces the language logo for these, but a file with a logo of its own keeps it.
assert.equal(fileTypeIconForPath("a.mjs").icon, null);
assert.equal(fileTypeIconForPath("a.toml").icon, null);
assert.ok(brand("Cargo.toml") && glyph("Cargo.toml") === null, "Cargo.toml keeps the Rust logo");
assert.ok(brand("Cargo.lock") && glyph("Cargo.lock") === null);
assert.ok(brand("yarn.lock") && brand("package-lock.json"));
assert.ok(brand("app.ts") && brand("main.rs") && brand("index.js"));
assert.equal(glyph("main.rs"), null);

// Unknown extensions still fall back to the plain page, and the Markdown badge draws the glyph.
assert.equal(glyph("file.unknownthing"), null);
assert.match(renderFileTypeIconHtml("a.mjs"), /viewBox="0 0 20 20"[^>]*><path d="M8 3h4/);
assert.match(renderFileTypeIconHtml("file.unknownthing"), /M4 3h8l4 4v10H4Z/);

console.log("file type icon test suite passed");
