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
use serde::Deserialize;

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

fn huggingface_resolve_parts(download_url: &str) -> Option<(String, Option<String>, String)> {
    let tail = download_url
        .strip_prefix("https://huggingface.co/")
        .or_else(|| download_url.strip_prefix("http://huggingface.co/"))?;
    let parts: Vec<&str> = tail.split('/').collect();
    if parts.len() < 5 || parts.get(2) != Some(&"resolve") {
        return None;
    }
    Some((
        format!("{}/{}", parts[0], parts[1]),
        parts.get(3).map(|value| value.to_string()),
        parts[4..].join("/"),
    ))
}

fn huggingface_download_file_path(download_url: &str) -> Option<String> {
    huggingface_resolve_parts(download_url).map(|(_, _, file)| file)
}

fn resolved_under_bit_folder(download_url: &str) -> bool {
    huggingface_download_file_path(download_url)
        .is_some_and(|path| path.split('/').any(is_bit_folder_segment))
}

fn base_huggingface_repo(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    if let Some(tail) = trimmed
        .strip_prefix("https://huggingface.co/")
        .or_else(|| trimmed.strip_prefix("http://huggingface.co/"))
    {
        let parts: Vec<&str> = tail.split('/').collect();
        if parts.len() < 2 {
            return None;
        }
        if parts.len() >= 3 && matches!(parts[2], "tree" | "resolve" | "blob") {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
        if parts.len() == 2 {
            return Some(format!("{}/{}", parts[0], parts[1]));
        }
        return None;
    }

    let without_selector = trimmed.split_once(':').map(|(left, _)| left).unwrap_or(trimmed);
    let parts: Vec<&str> = without_selector.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let repo_tail = parts[1].split('@').next()?;
    if repo_tail.is_empty() {
        return None;
    }
    Some(format!("{}/{}", parts[0], repo_tail))
}

fn input_specifies_hf_file_path(input: &str) -> bool {
    let trimmed = input.trim().trim_end_matches('/');
    if huggingface_resolve_parts(trimmed).is_some() {
        return true;
    }
    if trimmed.contains("://") {
        return false;
    }
    let without_selector = trimmed.split_once(':').map(|(left, _)| left).unwrap_or(trimmed);
    without_selector
        .split('/')
        .filter(|segment| !segment.is_empty())
        .count()
        >= 3
}

fn has_explicit_gguf_quant_selector(input: &str) -> bool {
    let trimmed = input.trim();
    let Some((left, selector)) = trimmed.rsplit_once(':') else {
        return false;
    };
    if left.is_empty() || selector.is_empty() {
        return false;
    }
    let selector = selector.split('@').next().unwrap_or(selector);
    if is_bit_folder_segment(selector) {
        return false;
    }
    selector.contains('_') || selector.starts_with('Q')
}

fn resolved_huggingface_repo(details: &ModelDetails) -> Option<String> {
    huggingface_resolve_parts(&details.download_url).map(|(repo, _, _)| repo)
}

/// True when the user did not pick a file, quant, or MLX folder — including HF
/// aliases (`org/repo/`, tree URLs, `@rev` mismatch) and catalog ids.
fn implicit_repo_level_input(input: &str, details: &ModelDetails) -> bool {
    if input_specifies_hf_file_path(input) || has_explicit_gguf_quant_selector(input) {
        return false;
    }
    if details.source == "catalog" {
        return true;
    }
    match (base_huggingface_repo(input), resolved_huggingface_repo(details)) {
        (Some(input_repo), Some(resolved_repo)) => input_repo == resolved_repo,
        _ => false,
    }
}

pub(crate) fn refuse_implicit_mlx_folder_pick(
    input: &str,
    details: &ModelDetails,
) -> Result<(), String> {
    if implicit_repo_level_input(input, details)
        && resolved_under_bit_folder(&details.download_url)
    {
        return Err(MLX_FOLDER_REFUSAL_MESSAGE.to_string());
    }
    Ok(())
}

fn is_mlx_kind(kind: &str) -> bool {
    kind.contains("MLX")
}

fn is_split_mlx_first_shard(basename: &str) -> bool {
    let Some(rest) = basename.strip_prefix("model-") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(".safetensors") else {
        return false;
    };
    let Some((left, right)) = rest.split_once("-of-") else {
        return false;
    };
    left == "00001" && right.len() == 5 && right.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_mlx_weight_path(path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    basename == "model.safetensors" || is_split_mlx_first_shard(basename)
}

fn mlx_weight_variant_folders_from_paths(paths: &[String]) -> BTreeSet<String> {
    let mut folders = BTreeSet::new();
    for path in paths {
        if !is_mlx_weight_path(path) {
            continue;
        }
        let Some((folder, _)) = path.split_once('/') else {
            continue;
        };
        if !folder.is_empty() {
            folders.insert(folder.to_ascii_lowercase());
        }
    }
    folders
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    rfilename: String,
}

#[derive(Debug, Deserialize)]
struct HfModelRevision {
    siblings: Option<Vec<HfSibling>>,
}

async fn fetch_hf_sibling_paths(repo: &str, revision: &str) -> Result<Vec<String>, String> {
    let url = format!("https://huggingface.co/api/models/{repo}/revision/{revision}");
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(format!("buzz-desktop/mesh-ref"))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<HfModelRevision>()
        .await
        .map_err(|error| error.to_string())?;

    Ok(response
        .siblings
        .unwrap_or_default()
        .into_iter()
        .map(|sibling| sibling.rfilename)
        .collect())
}

async fn refuse_repo_only_multi_folder_mlx(
    input: &str,
    details: &ModelDetails,
    variants: Option<&[ModelDetails]>,
) -> Result<(), String> {
    if !implicit_repo_level_input(input, details) || !is_mlx_kind(&details.kind) {
        return Ok(());
    }
    if variants.is_some_and(|entries| !entries.is_empty()) {
        return Ok(());
    }
    let Some((repo, revision, _file)) = huggingface_resolve_parts(&details.download_url) else {
        return Ok(());
    };
    let revision = revision.as_deref().unwrap_or("main");
    let sibling_paths = fetch_hf_sibling_paths(&repo, revision).await?;
    if mlx_weight_variant_folders_from_paths(&sibling_paths).len() > 1 {
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

    let variants = show_model_variants_with_progress(trimmed, |_| {})
        .await
        .map_err(|error| format!("{error:#}"))?;

    refuse_repo_only_multi_folder_mlx(trimmed, &details, variants.as_deref()).await?;

    if let Some(variants) = variants {
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

fn variant_folder_key(relative: &str) -> Option<Option<String>> {
    let path = Path::new(relative);
    let file_name = path.file_name()?.to_str()?;
    if !is_mlx_weight_path(file_name) {
        return None;
    }
    let parent = path
        .parent()
        .and_then(|value| value.as_os_str().to_str())
        .filter(|value| !value.is_empty());
    Some(parent.map(str::to_ascii_lowercase))
}

fn multi_variant_folder_repos(installed: &[InstalledModel]) -> BTreeSet<String> {
    let mut folders_by_repo: BTreeMap<String, BTreeSet<Option<String>>> = BTreeMap::new();
    for model in installed {
        let Some(repo_id) = huggingface_repo_id(&model.path) else {
            continue;
        };
        let Some(relative) = snapshot_relative_path(&model.path) else {
            continue;
        };
        let Some(folder_key) = variant_folder_key(&relative) else {
            continue;
        };
        folders_by_repo
            .entry(repo_id)
            .or_default()
            .insert(folder_key);
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
    let ambiguous_repos = multi_variant_folder_repos(installed);
    if installed.iter().any(|model| {
        model.model_ref == trimmed && installed_model_is_folder_unknown(model, &ambiguous_repos)
    }) {
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
    let ambiguous_repos = multi_variant_folder_repos(&installed);

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
        test_details_with_source(exact_ref, download_url, "huggingface")
    }

    fn test_details_with_source(
        exact_ref: &str,
        download_url: &str,
        source: &'static str,
    ) -> ModelDetails {
        ModelDetails {
            display_name: exact_ref.to_string(),
            exact_ref: exact_ref.to_string(),
            source,
            kind: "🍎 MLX",
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
    fn refuse_implicit_mlx_folder_pick_for_hf_aliases() {
        let details = test_details(
            "org/repo@main",
            "https://huggingface.co/org/repo/resolve/main/2bit/model-00001-of-00003.safetensors",
        );
        for alias in [
            "org/repo/",
            "https://huggingface.co/org/repo/",
            "https://huggingface.co/org/repo/tree/main",
            "org/repo@dev",
        ] {
            assert!(
                refuse_implicit_mlx_folder_pick(alias, &details).is_err(),
                "alias must refuse: {alias}"
            );
        }
    }

    #[test]
    fn refuse_implicit_mlx_folder_pick_for_catalog_nested_bit_folder() {
        let details = test_details_with_source(
            "catalog-mlx",
            "https://huggingface.co/org/repo/resolve/main/2bit/model-00001-of-00003.safetensors",
            "catalog",
        );
        assert!(refuse_implicit_mlx_folder_pick("catalog-mlx", &details).is_err());
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
    fn mlx_weight_variant_folders_detects_non_bit_variant_folders() {
        let folders = mlx_weight_variant_folders_from_paths(&[
            "fp16/model.safetensors".to_string(),
            "int4/model-00001-of-00002.safetensors".to_string(),
            "README.md".to_string(),
        ]);
        assert_eq!(folders.len(), 2);
        assert!(folders.contains("fp16"));
        assert!(folders.contains("int4"));
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
    fn flags_multi_variant_folder_repos_from_absolute_hub_paths() {
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
                    "fp16/model-00001-of-00002.safetensors",
                ),
                size_bytes: None,
                capabilities: Default::default(),
            },
        ];
        let ambiguous = multi_variant_folder_repos(&installed);
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

    #[test]
    fn allow_installed_model_delete_for_single_exact_gguf_artifact() {
        let installed = vec![InstalledModel {
            model_ref: "org/demo-gguf:Q4_K_M".to_string(),
            path: absolute_hub_shard_path(
                "org/demo-gguf",
                "abc123def456",
                "model-Q4_K_M.gguf",
            ),
            size_bytes: None,
            capabilities: Default::default(),
        }];
        refuse_installed_model_delete_for_scan("org/demo-gguf:Q4_K_M", &installed)
            .expect("single gguf artifact delete must be allowed");
    }
}
