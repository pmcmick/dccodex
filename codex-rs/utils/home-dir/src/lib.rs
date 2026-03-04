use dirs::home_dir;
use std::path::Path;
use std::path::PathBuf;

const CODEX_HOME_ENV_VAR: &str = "CODEX_HOME";
const DCCODEX_HOME_ENV_VAR: &str = "DCCODEX_HOME";
const CODEX_HOME_DIRNAME: &str = ".codex";
const DCCODEX_HOME_DIRNAME: &str = ".dccodex";

#[derive(Clone, Debug, Eq, PartialEq)]
struct HomeEnvOverride {
    env_var_name: &'static str,
    value: String,
}

/// Returns the path to the Codex configuration directory.
///
/// Resolution order:
/// - `CODEX_HOME`, when set (always wins)
/// - `DCCODEX_HOME`, when the executable is `dccodex*`
/// - default home dir (`~/.codex` for `codex*`, `~/.dccodex` for `dccodex*`)
///
/// - If `CODEX_HOME` is set, the value must exist and be a directory. The
///   value will be canonicalized and this function will Err otherwise.
/// - If neither home env var is set, this function does not verify that the
///   directory exists.
pub fn find_codex_home() -> std::io::Result<PathBuf> {
    let executable_name = current_executable_name();
    let codex_home_env = std::env::var(CODEX_HOME_ENV_VAR).ok();
    let dccodex_home_env = std::env::var(DCCODEX_HOME_ENV_VAR).ok();
    let resolved_home_env = resolve_home_env_override(
        codex_home_env.as_deref(),
        dccodex_home_env.as_deref(),
        executable_name.as_deref(),
    );
    find_codex_home_from_env(
        resolved_home_env
            .as_ref()
            .map(|override_env| override_env.value.as_str()),
        resolved_home_env
            .as_ref()
            .map(|override_env| override_env.env_var_name),
        executable_name.as_deref(),
    )
}

/// Returns the project-local config directory name for the current execution context.
///
/// Resolution order:
/// - `dccodex*` executable name => `.dccodex`
/// - `codex_home` basename is `.dccodex` => `.dccodex`
/// - otherwise => `.codex`
///
/// This keeps project-local config isolated when `dccodex` is used, while also
/// honoring explicitly passed `codex_home` paths that end in `.dccodex`.
pub fn project_config_dir_name(codex_home: &Path) -> &'static str {
    project_config_dir_name_for(current_executable_name().as_deref(), codex_home)
}

fn current_executable_name() -> Option<String> {
    std::env::args_os()
        .next()
        .as_deref()
        .and_then(|arg0| {
            std::path::Path::new(arg0)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
        })
        .map(str::to_owned)
}

fn is_dccodex_executable(executable_name: Option<&str>) -> bool {
    executable_name.is_some_and(|name| {
        let normalized = name.trim().to_ascii_lowercase();
        normalized == "dccodex" || normalized.starts_with("dccodex-")
    })
}

fn resolve_home_env_override(
    codex_home_env: Option<&str>,
    dccodex_home_env: Option<&str>,
    executable_name: Option<&str>,
) -> Option<HomeEnvOverride> {
    let codex_home = codex_home_env
        .map(str::trim)
        .filter(|val| !val.is_empty())
        .map(str::to_owned)
        .map(|value| HomeEnvOverride {
            env_var_name: CODEX_HOME_ENV_VAR,
            value,
        });
    if codex_home.is_some() {
        return codex_home;
    }
    if is_dccodex_executable(executable_name) {
        return dccodex_home_env
            .map(str::trim)
            .filter(|val| !val.is_empty())
            .map(str::to_owned)
            .map(|value| HomeEnvOverride {
                env_var_name: DCCODEX_HOME_ENV_VAR,
                value,
            });
    }
    None
}

fn default_home_dirname(executable_name: Option<&str>) -> &'static str {
    if is_dccodex_executable(executable_name) {
        DCCODEX_HOME_DIRNAME
    } else {
        CODEX_HOME_DIRNAME
    }
}

fn project_config_dir_name_for(executable_name: Option<&str>, codex_home: &Path) -> &'static str {
    if is_dccodex_executable(executable_name) || is_dccodex_home_dir(codex_home) {
        DCCODEX_HOME_DIRNAME
    } else {
        CODEX_HOME_DIRNAME
    }
}

fn is_dccodex_home_dir(codex_home: &Path) -> bool {
    codex_home
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(DCCODEX_HOME_DIRNAME))
}

fn find_codex_home_from_env(
    codex_home_env: Option<&str>,
    codex_home_env_var_name: Option<&str>,
    executable_name: Option<&str>,
) -> std::io::Result<PathBuf> {
    // Honor configured home env vars when set to allow users (and tests) to
    // override the default location.
    match codex_home_env {
        Some(val) => {
            let codex_home_env_var_name = codex_home_env_var_name.unwrap_or(CODEX_HOME_ENV_VAR);
            let path = PathBuf::from(val);
            let metadata = std::fs::metadata(&path).map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "{codex_home_env_var_name} points to {val:?}, but that path does not exist"
                    ),
                ),
                _ => std::io::Error::new(
                    err.kind(),
                    format!("failed to read {codex_home_env_var_name} {val:?}: {err}"),
                ),
            })?;

            if !metadata.is_dir() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{codex_home_env_var_name} points to {val:?}, but that path is not a directory"
                    ),
                ))
            } else {
                path.canonicalize().map_err(|err| {
                    std::io::Error::new(
                        err.kind(),
                        format!("failed to canonicalize {codex_home_env_var_name} {val:?}: {err}"),
                    )
                })
            }
        }
        None => {
            let mut p = home_dir().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find home directory",
                )
            })?;
            p.push(default_home_dirname(executable_name));
            Ok(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::default_home_dirname;
    use super::find_codex_home_from_env;
    use super::project_config_dir_name_for;
    use super::resolve_home_env_override;
    use dirs::home_dir;
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::io::ErrorKind;
    use tempfile::TempDir;

    #[test]
    fn find_codex_home_env_missing_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let missing = temp_home.path().join("missing-codex-home");
        let missing_str = missing
            .to_str()
            .expect("missing codex home path should be valid utf-8");

        let err = find_codex_home_from_env(Some(missing_str), Some("CODEX_HOME"), None)
            .expect_err("missing CODEX_HOME");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(
            err.to_string().contains("CODEX_HOME"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_file_path_is_fatal() {
        let temp_home = TempDir::new().expect("temp home");
        let file_path = temp_home.path().join("codex-home.txt");
        fs::write(&file_path, "not a directory").expect("write temp file");
        let file_str = file_path
            .to_str()
            .expect("file codex home path should be valid utf-8");

        let err = find_codex_home_from_env(Some(file_str), Some("CODEX_HOME"), None)
            .expect_err("file CODEX_HOME");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("not a directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn find_codex_home_env_valid_directory_canonicalizes() {
        let temp_home = TempDir::new().expect("temp home");
        let temp_str = temp_home
            .path()
            .to_str()
            .expect("temp codex home path should be valid utf-8");

        let resolved = find_codex_home_from_env(Some(temp_str), Some("CODEX_HOME"), None)
            .expect("valid CODEX_HOME");
        let expected = temp_home
            .path()
            .canonicalize()
            .expect("canonicalize temp home");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn find_codex_home_without_env_uses_default_home_dir() {
        let resolved = find_codex_home_from_env(None, None, None).expect("default CODEX_HOME");
        let mut expected = home_dir().expect("home dir");
        expected.push(".codex");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn find_codex_home_without_env_uses_dccodex_home_dir_for_dccodex_binary() {
        let resolved =
            find_codex_home_from_env(None, None, Some("dccodex")).expect("default DCCODEX_HOME");
        let mut expected = home_dir().expect("home dir");
        expected.push(".dccodex");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_home_env_override_prefers_codex_home_over_dccodex_home() {
        let resolved = resolve_home_env_override(
            Some("/tmp/codex-home"),
            Some("/tmp/dccodex-home"),
            Some("dccodex"),
        );
        assert_eq!(
            resolved,
            Some(super::HomeEnvOverride {
                env_var_name: "CODEX_HOME",
                value: "/tmp/codex-home".to_string(),
            })
        );
    }

    #[test]
    fn resolve_home_env_override_uses_dccodex_home_for_dccodex_binary() {
        let resolved = resolve_home_env_override(None, Some("/tmp/dccodex-home"), Some("dccodex"));
        assert_eq!(
            resolved,
            Some(super::HomeEnvOverride {
                env_var_name: "DCCODEX_HOME",
                value: "/tmp/dccodex-home".to_string(),
            })
        );
    }

    #[test]
    fn resolve_home_env_override_ignores_dccodex_home_for_codex_binary() {
        let resolved = resolve_home_env_override(None, Some("/tmp/dccodex-home"), Some("codex"));
        assert_eq!(resolved, None);
    }

    #[test]
    fn default_home_dirname_is_dccodex_for_dccodex_executable() {
        assert_eq!(default_home_dirname(Some("dccodex")), ".dccodex");
        assert_eq!(default_home_dirname(Some("dccodex-test")), ".dccodex");
        assert_eq!(default_home_dirname(Some("codex")), ".codex");
    }

    #[test]
    fn project_config_dir_name_uses_dccodex_for_dccodex_executable() {
        let resolved = project_config_dir_name_for(Some("dccodex"), std::path::Path::new("/tmp/x"));
        assert_eq!(resolved, ".dccodex");
    }

    #[test]
    fn project_config_dir_name_uses_dccodex_when_home_basename_is_dccodex() {
        let resolved = project_config_dir_name_for(
            Some("codex"),
            std::path::Path::new("/home/alice/.dccodex"),
        );
        assert_eq!(resolved, ".dccodex");
    }

    #[test]
    fn project_config_dir_name_defaults_to_codex() {
        let resolved =
            project_config_dir_name_for(Some("codex"), std::path::Path::new("/home/alice/custom"));
        assert_eq!(resolved, ".codex");
    }
}
