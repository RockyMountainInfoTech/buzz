import assert from "node:assert/strict";
import test from "node:test";

import { classifyModelRef } from "./classifyModelRef.ts";

test("empty string → unknown", () => {
  assert.deepEqual(classifyModelRef(""), { kind: "unknown" });
  assert.deepEqual(classifyModelRef("   "), { kind: "unknown" });
});

test("hf:// prefix → invalid", () => {
  const result = classifyModelRef("hf://meshllm/qwen3-8b@main");
  assert.equal(result.kind, "invalid");
  if (result.kind === "invalid") {
    assert.match(result.reason, /hf:\/\//);
  }
});

test("absolute path → invalid", () => {
  const result = classifyModelRef("/Users/me/models/qwen.gguf");
  assert.equal(result.kind, "invalid");
});

test("relative path with ./ → invalid", () => {
  const result = classifyModelRef("./models/qwen.gguf");
  assert.equal(result.kind, "invalid");
});

test("home shortcut → invalid", () => {
  const result = classifyModelRef("~/models/qwen.gguf");
  assert.equal(result.kind, "invalid");
});

test(".gguf extension without path prefix → invalid", () => {
  const result = classifyModelRef("my-model.gguf");
  assert.equal(result.kind, "invalid");
});

test("shard safetensors path → invalid", () => {
  const result = classifyModelRef(
    "PocketAiHub/Qwen3.8-27B-Abliterated-MLX/4bit/model-00001-of-00003.safetensors",
  );
  assert.equal(result.kind, "invalid");
});

test(":4bit selector → invalid", () => {
  const result = classifyModelRef("org/repo:4bit");
  assert.equal(result.kind, "invalid");
  if (result.kind === "invalid") {
    assert.match(result.reason, /cannot pick an MLX folder/i);
  }
});

test("plain catalog name → exact", () => {
  assert.deepEqual(classifyModelRef("Qwen3-8B-Q4_K_M"), {
    kind: "exact",
    ref: "Qwen3-8B-Q4_K_M",
  });
});

test("org/repo:QUANT → exact", () => {
  assert.deepEqual(
    classifyModelRef("unsloth/gemma-4-E4B-it-GGUF:Q4_K_M"),
    {
      kind: "exact",
      ref: "unsloth/gemma-4-E4B-it-GGUF:Q4_K_M",
    },
  );
});

test("trims whitespace before classifying", () => {
  assert.deepEqual(classifyModelRef("  Qwen3-8B-Q4_K_M  "), {
    kind: "exact",
    ref: "Qwen3-8B-Q4_K_M",
  });
});
