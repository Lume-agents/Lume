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

// The notice for a changed model or effort.
const { modelChangeNoticeText, effortName } = await import("../src/lib/interruptNotice.ts");
const pt = (english, portuguese) => portuguese;
const en = (english) => english;
assert.equal(modelChangeNoticeText('{"model":"Sonnet 5.5"}', pt), "Modelo: Sonnet 5.5");
assert.equal(modelChangeNoticeText('{"effort":"xhigh"}', pt), "Esforço: Muito alto");
assert.equal(modelChangeNoticeText('{"model":"gpt-b","effort":"high"}', en), "Model: gpt-b · Effort: High");
assert.equal(modelChangeNoticeText("not json", pt), "Modelo alterado");
assert.equal(effortName("custom", en), "Custom");
console.log("display text test passed");
