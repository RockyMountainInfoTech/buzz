//! Share-compute model ref contract — Mesh-owned resolution, not Buzz-invented
//! grammar.
//!
//! Share accepts only refs that Mesh v0.75.1 resolves through
//! `show_exact_model` / `parse_exact_model_ref` (catalog id, Hugging Face exact
//! ref, MLX repo shorthand). Layer-package `hf://` refs and local file paths are
//! rejected here because they use different Mesh entry points.

use mesh_llm_host_runtime::models::{
    installed_model_display_name, show_exact_model, show_model_variants_with_progress,
};

use super::MeshModelOption;

/// Synchronous Share input guards before any Mesh network I/O.
pub fn reject_share_model_ref_input(input: &str) -> Result<(), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("modelId is required for serve mode".to_string());
    }
    if trimmed.starts_with("hf://") {
        return Err(
            "Layer package refs (hf://…) are not supported in Share compute. \
             Use a catalog id or Hugging Face exact ref like org/repo:QUANT."
                .to_string(),
        );
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('~')
    {
        return Err(
            "Local file paths are not supported in Share compute. \
             Use a catalog id or Hugging Face exact ref."
                .to_string(),
        );
    }
    if trimmed.to_ascii_lowercase().ends_with(".gguf") {
        return Err(
            "Local GGUF file refs are not supported in Share compute. \
             Use a catalog id or Hugging Face exact ref."
                .to_string(),
        );
    }
    if looks_like_shard_path(trimmed) {
        return Err(
            "Shard file refs are not supported in Share compute. \
             Use a catalog id, org/repo:QUANT, org/repo@rev, or an MLX repo ref."
                .to_string(),
        );
    }
    Ok(())
}

fn looks_like_shard_path(input: &str) -> bool {
    input.split('/').any(|segment| {
        let seg = segment.to_ascii_lowercase();
        (seg.contains("-of-") && (seg.ends_with(".safetensors") || seg.ends_with(".gguf")))
            || seg.contains("mmproj")
    })
}

/// Resolve and canonicalize a Share model ref through Mesh. Returns the exact
/// ref Mesh will serve and advertise on `:9337`.
pub async fn resolve_share_model_ref(input: &str) -> Result<String, String> {
    reject_share_model_ref_input(input)?;
    let trimmed = input.trim();

    let details = show_exact_model(trimmed)
        .await
        .map_err(|error| format!("{error:#}"))?;

    if let Some(variants) = show_model_variants_with_progress(trimmed, |_| {})
        .await
        .map_err(|error| format!("{error:#}"))?
    {
        if variants.len() > 1 && trimmed != details.exact_ref {
            let user_picked_quant = trimmed.contains(':')
                && variants
                    .iter()
                    .any(|variant| variant.exact_ref == details.exact_ref);
            if !user_picked_quant {
                let list = variants
                    .iter()
                    .map(|variant| variant.exact_ref.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "Multiple quant variants are available — pick one exact ref: {list}"
                ));
            }
        }
    }

    Ok(details.exact_ref)
}

/// Installed models from disk — no running Mesh runtime required.
pub fn installed_models_from_disk() -> Vec<MeshModelOption> {
    let cache = mesh_llm_node::models::default_huggingface_cache_dir();
    mesh_llm_node::models::scan_installed_models(cache)
        .into_iter()
        .map(|model| {
            let id = model.model_ref;
            MeshModelOption {
                name: Some(installed_model_display_name(&id)),
                id,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_hf_package_refs() {
        let err = reject_share_model_ref_input("hf://meshllm/Qwen3-8B-Q4_K_M-layers@abc123")
            .expect_err("hf:// must be rejected");
        assert!(err.contains("hf://"));
    }

    #[test]
    fn rejects_local_paths() {
        assert!(reject_share_model_ref_input("/Users/me/model.gguf").is_err());
        assert!(reject_share_model_ref_input("./model.gguf").is_err());
        assert!(reject_share_model_ref_input("~/model.gguf").is_err());
        assert!(reject_share_model_ref_input("my-model.gguf").is_err());
    }

    #[test]
    fn rejects_shard_paths() {
        assert!(reject_share_model_ref_input(
            "PocketAiHub/Qwen3.8-27B-Abliterated-MLX/4bit/model-00001-of-00003.safetensors"
        )
        .is_err());
    }

    #[test]
    fn accepts_catalog_style_ids() {
        reject_share_model_ref_input("Qwen3-8B-Q4_K_M").expect("catalog id");
        reject_share_model_ref_input("unsloth/gemma-4-E4B-it-GGUF:Q4_K_M")
            .expect("org/repo:QUANT");
    }
}
