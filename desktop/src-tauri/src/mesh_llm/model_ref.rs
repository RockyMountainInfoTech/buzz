//! Share-compute model ref contract — Mesh-owned resolution, not Buzz-invented
//! grammar.
//!
//! Share accepts only refs that Mesh v0.75.1 resolves through
//! `show_exact_model` / `parse_exact_model_ref` (catalog id, Hugging Face exact
//! ref, MLX repo shorthand). Layer-package `hf://` refs, local file paths,
//! shard pointers, and MLX bit-folder selectors are rejected here because they
//! use different Mesh entry points or are not supported on this pin.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mesh_llm_host_runtime::models::{
    installed_model_display_name, show_exact_model, show_model_variants_with_progress,
    ModelDetails,
};
use mesh_llm_node::models::InstalledModel;

use super::MeshModelOption;

pub const MLX_FOLDER_REFUSAL_MESSAGE: &str =
    "This Mesh pin cannot pick an MLX folder; use a GGUF :QUANT or a single-folder MLX repo.";

/// Synchronous Share input guards before any Mesh network I/O.
pub fn reject_share_model_ref_input(input: &str) -> Result<(), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("modelId is required for serve mode".to_string());
    }
    if trimmed.starts_with("hf://") {
        return Err(
            "Layer package refs (hf://…) are not supported in Share compute. \
             Use a catalog id or Hugging Face exact ref like org/repo:Q4_K_M."
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
        return Err(MLX_FOLDER_REFUSAL_MESSAGE.to_string());
    }
    if contains_bit_folder_pointer(trimmed) {
        return Err(MLX_FOLDER_REFUSAL_MESSAGE.to_string());
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

fn is_bit_folder_segment(segment: &str) -> bool {
    let seg = segment.trim().to_ascii_lowercase();
    let Some(digits) = seg.strip_suffix("bit") else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn contains_bit_folder_pointer(input: &str) -> bool {
    if input.split('/').any(is_bit_folder_segment) {
        return true;
    }
    // Reject org/repo:4bit-style MLX folder selectors. GGUF quants use
    // underscores (Q4_K_M) and are resolved by Mesh `show_model_variants`.
    if let Some((_, selector)) = input.rsplit_once(':') {
        return is_bit_folder_segment(selector.split('@').next().unwrap_or(selector));
    }
    false
}

fn huggingface_download_file_path(download_url: &str) -> Option<String> {
    let tail = download_url
        .strip_prefix("https://huggingface.co/")
        .or_else(|| download_url.strip_prefix("http://huggingface.co/"))?;
    let parts: Vec<&str> = tail.split('/').collect();
    if parts.len() < 5 || parts.get(2) != Some(&"resolve") {
        return None;
    }
    Some(parts[4..].join("/"))
}

fn resolved_under_bit_folder(download_url: &str) -> bool {
    huggingface_download_file_path(download_url)
        .is_some_and(|path| path.split('/').any(is_bit_folder_segment))
}

fn repo_only_huggingface_input(input: &str, details: &ModelDetails) -> bool {
    if details.source != "huggingface" {
        return false;
    }
    let trimmed = input.trim();
    if trimmed.contains('/') {
        // org/repo/subpath or org/repo@rev/subpath — not repo-only.
        let without_scheme = trimmed
            .strip_prefix("https://huggingface.co/")
            .or_else(|| trimmed.strip_prefix("http://huggingface.co/"))
            .unwrap_or(trimmed);
        let segments: Vec<&str> = without_scheme.split('/').collect();
        if segments.len() > 2 {
            return false;
        }
    }
    trimmed == details.exact_ref
        || trimmed
            .strip_prefix("https://huggingface.co/")
            .is_some_and(|value| value == details.exact_ref)
        || trimmed
            .strip_prefix("http://huggingface.co/")
            .is_some_and(|value| value == details.exact_ref)
}

fn refuse_implicit_mlx_folder_pick(input: &str, details: &ModelDetails) -> Result<(), String> {
    if repo_only_huggingface_input(input, details) && resolved_under_bit_folder(&details.download_url)
    {
        return Err(MLX_FOLDER_REFUSAL_MESSAGE.to_string());
    }
    Ok(())
}

/// Resolve and canonicalize a Share model ref through Mesh. Returns the exact
/// ref Mesh will serve and advertise on `:9337`.
pub async fn resolve_share_model_ref(input: &str) -> Result<String, String> {
    reject_share_model_ref_input(input)?;
    let trimmed = input.trim();

    let details = show_exact_model(trimmed)
        .await
        .map_err(|error| format!("{error:#}"))?;

    refuse_implicit_mlx_folder_pick(trimmed, &details)?;

    if let Some(variants) = show_model_variants_with_progress(trimmed, |_| {})
        .await
        .map_err(|error| format!("{error:#}"))?
    {
        if variants.len() > 1 && trimmed != details.exact_ref {
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

    Ok(details.exact_ref)
}

fn snapshot_relative_path(path: &Path) -> Option<String> {
    let mut components = path.components();
    while let Some(component) = components.next() {
        if component.as_os_str() == "snapshots" {
            let _revision = components.next()?;
            let relative = components
                .map(|value| value.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()?
                .join("/");
            return Some(relative);
        }
    }
    None
}

fn huggingface_repo_id(path: &Path) -> Option<String> {
    let mut components = path.components();
    let repo_folder = components.next()?.as_os_str().to_str()?;
    repo_folder
        .strip_prefix("models--")
        .map(|value| value.replace("--", "/"))
}

fn bit_folder_in_relative_path(relative: &str) -> Option<String> {
    relative
        .split('/')
        .find(|segment| is_bit_folder_segment(segment))
        .map(ToString::to_string)
}

fn multi_bit_folder_repos(installed: &[InstalledModel]) -> BTreeSet<String> {
    let mut folders_by_repo: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for model in installed {
        let Some(repo_id) = huggingface_repo_id(&model.path) else {
            continue;
        };
        let Some(relative) = snapshot_relative_path(&model.path) else {
            continue;
        };
        let Some(folder) = bit_folder_in_relative_path(&relative) else {
            continue;
        };
        folders_by_repo
            .entry(repo_id)
            .or_default()
            .insert(folder);
    }
    folders_by_repo
        .into_iter()
        .filter(|(_, folders)| folders.len() > 1)
        .map(|(repo, _)| repo)
        .collect()
}

fn installed_display_name(model: &InstalledModel, folder_unknown: bool) -> String {
    if folder_unknown {
        snapshot_relative_path(&model.path)
            .map(|relative| format!("{relative} (folder unknown)"))
            .unwrap_or_else(|| format!("{} (folder unknown)", model.model_ref))
    } else {
        installed_model_display_name(&model.model_ref)
    }
}

/// Installed models from disk — no running Mesh runtime required.
pub fn installed_models_from_disk() -> Vec<MeshModelOption> {
    let cache = mesh_llm_node::models::default_huggingface_cache_dir();
    let installed = mesh_llm_node::models::scan_installed_models(cache);
    let ambiguous_repos = multi_bit_folder_repos(&installed);

    installed
        .into_iter()
        .map(|model| {
            let folder_unknown = huggingface_repo_id(&model.path)
                .is_some_and(|repo_id| ambiguous_repos.contains(&repo_id));
            MeshModelOption {
                id: model.model_ref.clone(),
                name: Some(installed_display_name(&model, folder_unknown)),
                path: Some(model.path.display().to_string()),
                deletable: !folder_unknown,
                folder_unknown,
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
    fn rejects_shard_and_bit_folder_pointers() {
        assert!(reject_share_model_ref_input(
            "PocketAiHub/Qwen3.8-27B-Abliterated-MLX/4bit/model-00001-of-00003.safetensors"
        )
        .is_err());
        assert!(reject_share_model_ref_input("org/repo:4bit").is_err());
        assert!(reject_share_model_ref_input("org/repo:6bit").is_err());
    }

    #[test]
    fn accepts_catalog_and_gguf_quant_refs() {
        reject_share_model_ref_input("Qwen3-8B-Q4_K_M").expect("catalog id");
        reject_share_model_ref_input("unsloth/gemma-4-E4B-it-GGUF:Q4_K_M")
            .expect("org/repo:QUANT");
    }

    #[test]
    fn detects_bit_folder_in_download_url() {
        assert!(resolved_under_bit_folder(
            "https://huggingface.co/org/repo/resolve/main/2bit/model-00001-of-00003.safetensors"
        ));
        assert!(!resolved_under_bit_folder(
            "https://huggingface.co/mlx-community/foo-8bit/resolve/main/model.safetensors"
        ));
    }

    #[test]
    fn flags_multi_bit_folder_repos_from_scan_paths() {
        let installed = vec![
            InstalledModel {
                model_ref: "org/demo-mlx".to_string(),
                path: PathBuf::from(
                    "/cache/models--org--demo-mlx/snapshots/abc/4bit/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
            InstalledModel {
                model_ref: "org/demo-mlx".to_string(),
                path: PathBuf::from(
                    "/cache/models--org--demo-mlx/snapshots/abc/6bit/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
        ];
        let ambiguous = multi_bit_folder_repos(&installed);
        assert!(ambiguous.contains("org/demo-mlx"));
    }
}
