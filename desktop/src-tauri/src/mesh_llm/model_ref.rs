//! Share-compute model ref contract — Mesh-owned resolution, not Buzz-invented
//! grammar.
//!
//! Share accepts only refs that Mesh v0.75.1 resolves through
//! `show_exact_model` / `parse_exact_model_ref` (catalog id, Hugging Face exact
//! ref, MLX repo shorthand). `hf://` refs, local file paths, shard pointers,
//! and MLX bit-folder selectors are rejected here.

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

pub const HF_SHARE_REFUSAL_MESSAGE: &str =
    "hf:// is not a Share model ref. Use a catalog id or Hugging Face exact ref like org/repo:Q4_K_M.";

pub const AMBIGUOUS_MLX_DELETE_REFUSAL_MESSAGE: &str =
    "Cannot delete this MLX cache entry: folder is unknown on this Mesh pin.";

/// Synchronous Share input guards before any Mesh network I/O.
pub fn reject_share_model_ref_input(input: &str) -> Result<(), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("modelId is required for serve mode".to_string());
    }
    if trimmed.starts_with("hf://") {
        return Err(HF_SHARE_REFUSAL_MESSAGE.to_string());
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

pub(crate) fn refuse_implicit_mlx_folder_pick(
    input: &str,
    details: &ModelDetails,
) -> Result<(), String> {
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
    for component in path.components() {
        let folder = component.as_os_str().to_str()?;
        if let Some(stripped) = folder.strip_prefix("models--") {
            return Some(stripped.replace("--", "/"));
        }
    }
    None
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

fn installed_model_is_folder_unknown(
    model: &InstalledModel,
    ambiguous_repos: &BTreeSet<String>,
) -> bool {
    huggingface_repo_id(&model.path).is_some_and(|repo_id| ambiguous_repos.contains(&repo_id))
}

fn refuse_installed_model_delete_for_scan(
    model_ref: &str,
    installed: &[InstalledModel],
) -> Result<(), String> {
    let trimmed = model_ref.trim();
    if trimmed.is_empty() {
        return Err("modelRef is required".to_string());
    }
    let ambiguous_repos = multi_bit_folder_repos(installed);
    if installed
        .iter()
        .any(|model| model.model_ref == trimmed && installed_model_is_folder_unknown(model, &ambiguous_repos))
    {
        return Err(AMBIGUOUS_MLX_DELETE_REFUSAL_MESSAGE.to_string());
    }
    Ok(())
}

/// Backend safety gate for cache eviction — not only a UI affordance.
pub fn refuse_installed_model_delete(model_ref: &str) -> Result<(), String> {
    let cache = mesh_llm_node::models::default_huggingface_cache_dir();
    let installed = mesh_llm_node::models::scan_installed_models(cache);
    refuse_installed_model_delete_for_scan(model_ref, &installed)
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
            let folder_unknown = installed_model_is_folder_unknown(&model, &ambiguous_repos);
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
    use mesh_llm_host_runtime::models::ModelCapabilities;

    fn test_details(exact_ref: &str, download_url: &str) -> ModelDetails {
        ModelDetails {
            display_name: exact_ref.to_string(),
            exact_ref: exact_ref.to_string(),
            source: "huggingface",
            kind: "mlx",
            download_url: download_url.to_string(),
            size_label: None,
            description: None,
            draft: None,
            capabilities: ModelCapabilities::default(),
        }
    }

    const HUB_ROOT: &str = "/Users/me/Library/Caches/huggingface/hub";

    fn absolute_hub_shard_path(repo: &str, revision: &str, relative: &str) -> PathBuf {
        let folder = format!("models--{}", repo.replace('/', "--"));
        PathBuf::from(HUB_ROOT)
            .join(folder)
            .join("snapshots")
            .join(revision)
            .join(relative)
    }

    #[test]
    fn rejects_hf_refs() {
        let err = reject_share_model_ref_input("hf://meshllm/Qwen3-8B-Q4_K_M-layers@abc123")
            .expect_err("hf:// must be rejected");
        assert_eq!(err, HF_SHARE_REFUSAL_MESSAGE);
        assert!(!err.to_ascii_lowercase().contains("layer package"));
    }

    #[test]
    fn rejects_local_paths() {
        assert!(reject_share_model_ref_input("/Users/me/model.gguf").is_err());
        assert!(reject_share_model_ref_input("./model.gguf").is_err());
        assert!(reject_share_model_ref_input("~/model.gguf").is_err());
        assert!(reject_share_model_ref_input("my-model.gguf").is_err());
    }

    #[test]
    fn rejects_bit_folder_pointers_without_shard() {
        assert!(reject_share_model_ref_input("org/repo:4bit").is_err());
        assert!(reject_share_model_ref_input("org/repo:6bit").is_err());
        assert!(reject_share_model_ref_input("org/repo/4bit").is_err());
        assert!(reject_share_model_ref_input(
            "PocketAiHub/Qwen3.8-27B-Abliterated-MLX/4bit/model-00001-of-00003.safetensors"
        )
        .is_err());
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
    fn refuse_implicit_mlx_folder_pick_for_repo_only_nested_bit_folder() {
        let details = test_details(
            "org/repo",
            "https://huggingface.co/org/repo/resolve/main/2bit/model-00001-of-00003.safetensors",
        );
        assert!(refuse_implicit_mlx_folder_pick("org/repo", &details).is_err());
        let details_at_rev = test_details(
            "org/repo@main",
            "https://huggingface.co/org/repo/resolve/main/2bit/model-00001-of-00003.safetensors",
        );
        assert!(refuse_implicit_mlx_folder_pick("org/repo@main", &details_at_rev).is_err());
    }

    #[test]
    fn allow_repo_root_mlx_resolve() {
        let details = test_details(
            "mlx-community/foo-8bit",
            "https://huggingface.co/mlx-community/foo-8bit/resolve/main/model.safetensors",
        );
        refuse_implicit_mlx_folder_pick("mlx-community/foo-8bit", &details).expect("repo-root mlx");
    }

    #[test]
    fn huggingface_repo_id_finds_models_prefix_on_absolute_hub_path() {
        let path = absolute_hub_shard_path(
            "org/demo-mlx",
            "abc123def456",
            "4bit/model-00001-of-00002.safetensors",
        );
        assert_eq!(huggingface_repo_id(&path).as_deref(), Some("org/demo-mlx"));
    }

    #[test]
    fn flags_multi_bit_folder_repos_from_absolute_hub_paths() {
        let installed = vec![
            InstalledModel {
                model_ref: "org/demo-mlx".to_string(),
                path: absolute_hub_shard_path(
                    "org/demo-mlx",
                    "abc123def456",
                    "4bit/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
            InstalledModel {
                model_ref: "org/demo-mlx".to_string(),
                path: absolute_hub_shard_path(
                    "org/demo-mlx",
                    "abc123def456",
                    "6bit/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
        ];
        let ambiguous = multi_bit_folder_repos(&installed);
        assert!(ambiguous.contains("org/demo-mlx"));
        assert!(installed_model_is_folder_unknown(&installed[0], &ambiguous));
    }

    #[test]
    fn refuse_installed_model_delete_for_ambiguous_mlx_repo() {
        let installed = vec![
            InstalledModel {
                model_ref: "org/demo-mlx".to_string(),
                path: absolute_hub_shard_path(
                    "org/demo-mlx",
                    "abc123def456",
                    "4bit/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
            InstalledModel {
                model_ref: "org/demo-mlx".to_string(),
                path: absolute_hub_shard_path(
                    "org/demo-mlx",
                    "abc123def456",
                    "6bit/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
        ];
        let err = refuse_installed_model_delete_for_scan("org/demo-mlx", &installed)
            .expect_err("ambiguous mlx delete must be refused");
        assert_eq!(err, AMBIGUOUS_MLX_DELETE_REFUSAL_MESSAGE);
    }
}
