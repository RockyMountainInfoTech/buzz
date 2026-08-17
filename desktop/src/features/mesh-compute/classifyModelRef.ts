/**
 * Classification of a free-text model ref entered into the Share compute card.
 * Validation mirrors Mesh v0.75.1 `parse_exact_model_ref` / `show_exact_model`:
 * catalog ids, Hugging Face exact refs (org/repo:QUANT, org/repo@rev, MLX repo
 * shorthand). Layer-package `hf://` refs, local paths, shard pointers, and MLX
 * bit-folder selectors are rejected.
 */
export type ModelRefKind =
  | { kind: "exact"; ref: string }
  | { kind: "invalid"; reason: string }
  | { kind: "unknown" };

const MLX_FOLDER_REFUSAL_MESSAGE =
  "This Mesh pin cannot pick an MLX folder; use a GGUF :QUANT or a single-folder MLX repo.";

const SHARD_SEGMENT =
  /(?:^|\/)[^/]*-\d+-of-\d+\.(?:safetensors|gguf)(?:$|\/)/i;

function isBitFolderSegment(segment: string): boolean {
  const value = segment.trim().toLowerCase();
  const match = /^(\d+)bit$/.exec(value);
  return match != null && match[1] != null && match[1].length > 0;
}

function containsBitFolderPointer(value: string): boolean {
  if (value.split("/").some(isBitFolderSegment)) {
    return true;
  }
  const selector = value.includes(":") ? value.slice(value.lastIndexOf(":") + 1) : "";
  const selectorRoot = selector.split("@")[0] ?? selector;
  return isBitFolderSegment(selectorRoot);
}

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
        "Layer package refs (hf://…) are not supported here. Use a catalog id or Hugging Face exact ref like org/repo:Q4_K_M.",
    };
  }
  if (looksLikeLocalPath(trimmed)) {
    return {
      kind: "invalid",
      reason:
        "Local file paths are not supported here. Use a catalog id or Hugging Face exact ref.",
    };
  }
  if (looksLikeShardPath(trimmed) || containsBitFolderPointer(trimmed)) {
    return {
      kind: "invalid",
      reason: MLX_FOLDER_REFUSAL_MESSAGE,
    };
  }
  return { kind: "exact", ref: trimmed };
}
