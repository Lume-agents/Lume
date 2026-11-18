// Runs every desktop test in tests/ (each file in its own Node process, a few at a time) and prints only
// the output of the ones that fail. Pass file names to run just those: node scripts/run-tests.mjs notes git-events
import { spawn } from "node:child_process";
import { readdirSync } from "node:fs";
import { availableParallelism } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
// The phone app's tests run in `npm run check:mobile`.
const mobileOnly = new Set(["mobile-usage-reset.test.mjs", "response-dedup.test.mjs"]);
const wanted = process.argv.slice(2).map((name) => (name.endsWith(".test.mjs") ? name : `${name}.test.mjs`));
const files = readdirSync(join(root, "tests"))
  .filter((file) => file.endsWith(".test.mjs") && !mobileOnly.has(file))
  .filter((file) => !wanted.length || wanted.includes(file))
  .sort();
// The browser tests start a Chromium each, so a few at a time is enough.
const concurrency = Math.max(1, Math.min(4, availableParallelism() - 1));

function run(file) {
  return new Promise((resolve) => {
    const started = Date.now();
    const child = spawn(process.execPath, ["--experimental-strip-types", join("tests", file)], { cwd: root, env: process.env });
    let output = "";
    child.stdout.on("data", (chunk) => (output += chunk));
    child.stderr.on("data", (chunk) => (output += chunk));
    child.on("close", (code, signal) => resolve({ file, ok: code === 0, code: code ?? signal, output, seconds: (Date.now() - started) / 1000 }));
  });
}

const queue = [...files];
const failures = [];
let done = 0;
await Promise.all(Array.from({ length: concurrency }, async () => {
  for (let file = queue.shift(); file; file = queue.shift()) {
    const result = await run(file);
    done += 1;
    console.log(`${result.ok ? "ok  " : "FAIL"} ${String(done).padStart(3)}/${files.length} ${file} (${result.seconds.toFixed(1)}s)`);
    if (!result.ok) failures.push(result);
  }
}));

for (const failure of failures) {
  console.error(`\n── ${failure.file} (exit ${failure.code}) ──\n${failure.output.trim()}`);
}
if (!files.length) console.error("No test matched.");
console.log(`\n${files.length - failures.length} passed, ${failures.length} failed`);
process.exit(failures.length || !files.length ? 1 : 0);
