use crate::contextual_user_message::PROJECT_MEMORY_FRAGMENT;
use crate::git_info::collect_git_info;
use crate::git_info::get_git_repo_root;
use crate::truncate::TruncationPolicy;
use crate::truncate::truncate_text;
use codex_protocol::models::ResponseItem;
use codex_utils_cache::sha1_digest;
use dunce::canonicalize as normalize_path;
use std::path::Path;
use std::path::PathBuf;

const PROJECT_MEMORY_FILENAME: &str = "memory.md";
const PROJECTS_DIR_NAME: &str = "projects";
const PROJECT_MEMORY_MAX_TOKENS: usize = 256;
const PROJECT_ID_HASH_BYTES: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectMemory {
    pub project_id: String,
    pub git_root: PathBuf,
    pub text: String,
}

impl ProjectMemory {
    pub(crate) fn serialize_to_text(&self) -> String {
        PROJECT_MEMORY_FRAGMENT.wrap(format!(
            "<project_id>{}</project_id>\n<git_root>{}</git_root>\n{}",
            self.project_id,
            self.git_root.display(),
            self.text
        ))
    }
}

impl From<ProjectMemory> for ResponseItem {
    fn from(project_memory: ProjectMemory) -> Self {
        PROJECT_MEMORY_FRAGMENT.into_message(project_memory.serialize_to_text())
    }
}

pub(crate) fn projects_memory_root(codex_home: &Path) -> PathBuf {
    codex_home.join("memories").join(PROJECTS_DIR_NAME)
}

pub(crate) async fn load_project_memory(codex_home: &Path, cwd: &Path) -> Option<ProjectMemory> {
    let git_root = get_git_repo_root(cwd)?;
    let git_root = normalize_path(&git_root).unwrap_or(git_root);
    let origin_url = collect_git_info(&git_root)
        .await
        .and_then(|git_info| git_info.repository_url);
    let project_id = build_project_id(&git_root, origin_url.as_deref());
    let memory_path = projects_memory_root(codex_home)
        .join(project_id.as_str())
        .join(PROJECT_MEMORY_FILENAME);
    let memory = tokio::fs::read_to_string(&memory_path).await.ok()?;
    let memory = truncate_text(
        memory.trim(),
        TruncationPolicy::Tokens(PROJECT_MEMORY_MAX_TOKENS),
    );
    if memory.is_empty() {
        return None;
    }

    Some(ProjectMemory {
        project_id,
        git_root,
        text: memory,
    })
}

fn build_project_id(git_root: &Path, origin_url: Option<&str>) -> String {
    let repo_slug = git_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_slug)
        .filter(|slug| !slug.is_empty())
        .unwrap_or_else(|| "project".to_string());
    let mut identity = git_root.to_string_lossy().replace('\\', "/");
    if let Some(origin_url) = origin_url
        .map(str::trim)
        .filter(|origin_url| !origin_url.is_empty())
    {
        identity.push('\n');
        identity.push_str(origin_url);
    }
    let digest = sha1_digest(identity.as_bytes());
    let suffix = digest[..PROJECT_ID_HASH_BYTES]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{repo_slug}-{suffix}")
}

fn sanitize_slug(input: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut last_was_separator = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            slug.push('-');
            last_was_separator = true;
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    fn init_git_repo(root: &Path) {
        let output = std::process::Command::new("git")
            .arg("init")
            .arg(root)
            .output()
            .expect("git init should succeed");
        assert!(output.status.success(), "git init failed: {output:?}");
    }

    fn add_origin(root: &Path, origin_url: &str) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["remote", "add", "origin", origin_url])
            .output()
            .expect("git remote add should succeed");
        assert!(output.status.success(), "git remote add failed: {output:?}");
    }

    #[test]
    fn project_id_uses_repo_slug_and_identity_hash() {
        let git_root = Path::new("/tmp/My Repo");
        let without_origin = build_project_id(git_root, None);
        let with_origin = build_project_id(git_root, Some("git@example.com:openai/repo.git"));

        assert!(without_origin.starts_with("my-repo-"));
        assert!(with_origin.starts_with("my-repo-"));
        assert_ne!(without_origin, with_origin);
    }

    #[tokio::test]
    async fn load_project_memory_reads_repo_scoped_memory_file() {
        let codex_home = TempDir::new().expect("create codex home");
        let workspace = TempDir::new().expect("create workspace");
        init_git_repo(workspace.path());
        add_origin(workspace.path(), "git@example.com:openai/dccodex.git");

        let project_id =
            build_project_id(workspace.path(), Some("git@example.com:openai/dccodex.git"));
        let memory_dir = projects_memory_root(codex_home.path()).join(project_id.as_str());
        std::fs::create_dir_all(&memory_dir).expect("create project memory dir");
        std::fs::write(
            memory_dir.join(PROJECT_MEMORY_FILENAME),
            "Remember to update protocol docs when changing lifecycle hooks.",
        )
        .expect("write project memory");

        let project_memory = load_project_memory(codex_home.path(), workspace.path())
            .await
            .expect("project memory should load");

        assert_eq!(project_memory.project_id, project_id);
        assert_eq!(project_memory.git_root, workspace.path());
        assert_eq!(
            project_memory.text,
            "Remember to update protocol docs when changing lifecycle hooks."
        );
    }

    #[tokio::test]
    async fn load_project_memory_returns_none_without_memory_file() {
        let codex_home = TempDir::new().expect("create codex home");
        let workspace = TempDir::new().expect("create workspace");
        init_git_repo(workspace.path());

        let project_memory = load_project_memory(codex_home.path(), workspace.path()).await;

        assert_eq!(project_memory, None);
    }
}
