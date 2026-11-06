import assert from "node:assert/strict";
import { displayText } from "../src/lib/i18n.ts";

// Labels written in English read in Portuguese, and the other way round; anything unknown is left as it is.
assert.equal(displayText("pt-BR", "Ready in Lume"), "Pronto no Lume");
assert.equal(displayText("pt-BR", "Prompt interrupted"), "Prompt interrompido");
assert.equal(displayText("pt-BR", "Permission denied"), "Permissão recusada");
assert.equal(displayText("pt-BR", "Executando"), "Executando");
assert.equal(displayText("en", "Executando"), "Running");
assert.equal(displayText("en", "Ready in Lume"), "Ready in Lume");
assert.equal(displayText("pt-BR", "Corrigindo o login"), "Corrigindo o login");
console.log("display text test passed");
