import assert from "node:assert/strict";
import { fileChangeKind, isPreviewableImage } from "../src/lib/fileChanges.ts";

// Whether a recorded diff made a file, changed it or removed it.
assert.equal(fileChangeKind("diff --git a/x b/x\nnew file mode 100644\n--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+a\n"), "created");
assert.equal(fileChangeKind("diff --git a/x b/x\n--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+a\n"), "created");
assert.equal(fileChangeKind("diff --git a/i.png b/i.png\nnew file mode 100644\nBinary files /dev/null and b/i.png differ\n"), "created");
assert.equal(fileChangeKind("diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n"), "edited");
assert.equal(fileChangeKind("diff --git a/i.png b/i.png\ndeleted file mode 100644\nBinary files a/i.png and /dev/null differ\n"), "deleted");
assert.equal(fileChangeKind("diff --git a/x b/x\n--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-a\n"), "deleted");
assert.equal(fileChangeKind("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"), "edited");
assert.equal(fileChangeKind(""), "edited");
assert.equal(fileChangeKind(undefined), "edited");

// The formats the preview can read back.
for (const path of ["/a/logo.PNG", "b.jpg", "c.jpeg", "d.gif", "e.webp"]) assert.equal(isPreviewableImage(path), true, path);
for (const path of ["a.svg", "b.png.txt", "c.pdf", "png"]) assert.equal(isPreviewableImage(path), false, path);
console.log("changed images test passed");
