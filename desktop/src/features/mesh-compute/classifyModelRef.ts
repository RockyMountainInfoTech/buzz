/**
 * Classification of a free-text model ref entered into the Share compute card.
 * Validation mirrors Mesh v0.75.1 `parse_exact_model_ref` / `show_exact_model`:
 * catalog ids, Hugging Face exact refs (org/repo:QUANT, org/repo@rev, MLX repo
 * shorthand). Layer-package `hf://` refs, local paths, and shard paths are
 * rejected — they use different Mesh entry points or are not valid Share input.
 */
export type ModelRefKind =
  | { kind: "exact"; ref: string }
  | { kind: "invalid"; reason: string }
  | { kind: "unknown" };

const SHARD_SEGMENT =
  /(?:^|\/)[^/]*-\d+-of-\d+\.(?:safetensors|gguf)(?:$|\/)/i;

function looksLikeShardPath(value: string): boolean {
  if (value.toLowerCase().includes("mmproj")) {
    return true;
  }
  return SHARD_SEGMENT.test(value);
}

function looksLikeLocalPath(value: string): boolean {
  return (
    value.startsWith("/") ||
    value.startsWith("./") ||
    value.startsWith("../") ||
    value.startsWith("~") ||
    value.toLowerCase().endsWith(".gguf")
  );
}

/**
 * Classify Share-compute custom input before Mesh canonicalizes it server-side.
 */
export function classifyModelRef(raw: string): ModelRefKind {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    return { kind: "unknown" };
  }
  if (trimmed.startsWith("hf://")) {
    return {
      kind: "invalid",
      reason:
        "Layer package refs (hf://…) are not supported here. Use a catalog id or Hugging Face exact ref like org/repo:QUANT.",
    };
  }
  if (looksLikeLocalPath(trimmed)) {
    return {
      kind: "invalid",
      reason:
        "Local file paths are not supported here. Use a catalog id or Hugging Face exact ref.",
    };
  }
  if (looksLikeShardPath(trimmed)) {
    return {
      kind: "invalid",
      reason:
        "Shard file refs are not supported here. Use a catalog id, org/repo:QUANT, org/repo@rev, or an MLX repo ref.",
    };
  }
  return { kind: "exact", ref: trimmed };
}
