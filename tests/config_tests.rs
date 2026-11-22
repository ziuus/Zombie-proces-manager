#[path = "../src/config.rs"]
mod config;

use config::*;
use std::ffi::OsString;
use std::path::PathBuf;

#[test]
fn test_default_config_contains_13_defaults() {
    let config = Config::default();
    let resolved = config.resolved_suspicious_names();
    assert_eq!(resolved.len(), 13);
    assert_eq!(resolved.len(), DEFAULT_SUSPICIOUS_NAMES.len());

    for expected in DEFAULT_SUSPICIOUS_NAMES {
        assert!(
            resolved.contains(&expected.to_string()),
            "missing expected default: {expected}"
        );
        assert!(
            config.matches(expected),
            "config.matches failed for default: {expected}"
        );
    }

    assert_eq!(config.filters.mode, FilterMode::Merge);
    assert!(!config.matches("systemd"));
    assert!(!config.matches("bash"));
    assert!(!config.matches("init"));
}

#[test]
fn test_merge_mode_appends_custom_patterns_without_duplicates() {
    let toml_str = r#"
        [filters]
        mode = "merge"
        suspicious_names = ["my-worker", "NODE", "python", "custom-api"]
    "#;
    let config = Config::from_toml_str(toml_str).expect("failed to parse TOML");
    let resolved = config.resolved_suspicious_names();

    // 13 defaults + 2 new ("my-worker", "custom-api") = 15 total
    // "NODE" and "python" are already in defaults so should be deduplicated
    assert_eq!(resolved.len(), 15);

    assert!(resolved.contains(&"my-worker".to_string()));
    assert!(resolved.contains(&"custom-api".to_string()));
    assert!(resolved.contains(&"node".to_string()));
    assert!(resolved.contains(&"python".to_string()));

    // Make sure "node" only appears once
    let node_count = resolved.iter().filter(|&s| s == "node").count();
    assert_eq!(node_count, 1, "node should only appear once");

    assert!(config.matches("my-worker"));
    assert!(config.matches("custom-api"));
    assert!(config.matches("node"));
    assert!(config.matches("ruby"));
    assert!(!config.matches("unrelated-daemon"));
}

#[test]
fn test_merge_mode_default_when_mode_omitted() {
    let toml_str = r#"
        [filters]
        suspicious_names = ["sidekiq"]
    "#;
    let config = Config::from_toml_str(toml_str).expect("failed to parse TOML");
    assert_eq!(config.filters.mode, FilterMode::Merge);
    let resolved = config.resolved_suspicious_names();
    assert_eq!(resolved.len(), 14);
    assert!(resolved.contains(&"sidekiq".to_string()));
    assert!(config.matches("sidekiq"));
    assert!(config.matches("node"));
}

#[test]
fn test_replace_mode_replaces_all_defaults() {
    let toml_str = r#"
        [filters]
        mode = "replace"
        suspicious_names = ["my-custom-service", "worker-proc"]
    "#;
    let config = Config::from_toml_str(toml_str).expect("failed to parse TOML");
    assert_eq!(config.filters.mode, FilterMode::Replace);

    let resolved = config.resolved_suspicious_names();
    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved, vec!["my-custom-service", "worker-proc"]);

    assert!(config.matches("my-custom-service"));
    assert!(config.matches("worker-proc"));
    // Defaults should NOT match
    assert!(!config.matches("node"));
    assert!(!config.matches("python"));
    assert!(!config.matches("chrome"));
}

#[test]
fn test_replace_mode_empty_list() {
    let toml_str = r#"
        [filters]
        mode = "replace"
        suspicious_names = []
    "#;
    let config = Config::from_toml_str(toml_str).expect("failed to parse TOML");
    let resolved = config.resolved_suspicious_names();
    assert!(resolved.is_empty());
    assert!(!config.matches("node"));
    assert!(!config.matches("anything"));
}

#[test]
fn test_empty_and_whitespace_filter_strings_are_stripped() {
    let toml_str = r#"
        [filters]
        mode = "replace"
        suspicious_names = ["", "   ", "\t", "\n", "valid-app", "  "]
    "#;
    let config = Config::from_toml_str(toml_str).expect("failed to parse TOML");
    let resolved = config.resolved_suspicious_names();

    // Only "valid-app" should survive
    assert_eq!(resolved, vec!["valid-app"]);
    assert!(config.matches("valid-app"));

    // Critical check: empty patterns must NEVER cause random processes to match!
    assert!(!config.matches("systemd"));
    assert!(!config.matches("init"));
    assert!(!config.matches("bash"));
    assert!(!config.matches("sshd"));
}

#[test]
fn test_case_insensitivity_and_whitespace_trimming() {
    let toml_str = r#"
        [filters]
        mode = "replace"
        suspicious_names = ["  NODE  ", "  Python_Worker  ", "\tRubyServer\n"]
    "#;
    let config = Config::from_toml_str(toml_str).expect("failed to parse TOML");
    let resolved = config.resolved_suspicious_names();

    assert_eq!(resolved, vec!["node", "python_worker", "rubyserver"]);

    // Case-insensitive matching
    assert!(config.matches("NODE"));
    assert!(config.matches("node"));
    assert!(config.matches("/usr/bin/Node"));
    assert!(config.matches("python_worker"));
    assert!(config.matches("PYTHON_WORKER"));
    assert!(config.matches("my_rubyserver_daemon"));
    assert!(!config.matches("rustc"));
}

#[test]
fn test_filter_mode_case_insensitivity() {
    let merge_variations = ["merge", "Merge", "MERGE", "  merge  "];
    for mode_str in merge_variations {
        let toml_str = format!("[filters]\nmode = \"{mode_str}\"");
        let config: Config = toml::from_str(&toml_str).expect("should parse merge variant");
        assert_eq!(config.filters.mode, FilterMode::Merge);
    }

    let replace_variations = ["replace", "Replace", "REPLACE", "  replace  "];
    for mode_str in replace_variations {
        let toml_str = format!("[filters]\nmode = \"{mode_str}\"");
        let config: Config = toml::from_str(&toml_str).expect("should parse replace variant");
        assert_eq!(config.filters.mode, FilterMode::Replace);
    }
}

fn get_test_scratch_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_scratch");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[test]
fn test_malformed_toml_returns_err_and_load_fallback() {
    let malformed_toml = r#"
        [filters
        suspicious_names = ["broken
    "#;
    let res = Config::from_toml_str(malformed_toml);
    assert!(res.is_err(), "malformed toml must return an error");
    let err_str = res.unwrap_err();
    assert!(
        err_str.contains("Failed to parse TOML configuration"),
        "unexpected error message: {err_str}"
    );

    // Test load_from_candidates with a malformed file
    let scratch_dir = get_test_scratch_dir();
    let malformed_file = scratch_dir.join(format!("test_opm_malformed_{}.toml", std::process::id()));
    std::fs::write(&malformed_file, malformed_toml).expect("write scratch file");

    let load_res = Config::load_from_candidates(std::slice::from_ref(&malformed_file));
    // Clean up scratch file immediately
    let _ = std::fs::remove_file(&malformed_file);

    assert_eq!(load_res.source, Some(malformed_file));
    assert!(load_res.warning.is_some());
    // Fallback config contains all 13 defaults
    assert_eq!(load_res.config.resolved_suspicious_names().len(), 13);
    assert!(load_res.config.matches("node"));
}

#[test]
fn test_unknown_fields_tolerated_for_forward_compatibility() {
    let toml_str = r#"
        future_global_setting = true

        [filters]
        suspicious_names = ["custom-proc"]
        future_filter_opt = 42

        [unrelated_section]
        foo = "bar"
    "#;
    let res = Config::from_toml_str(toml_str);
    assert!(res.is_ok(), "unknown fields must be ignored: {res:?}");
    let config = res.unwrap();
    assert!(config.matches("custom-proc"));
    assert!(config.matches("node"));
}

#[test]
fn test_invalid_filter_mode_fails_deserialization() {
    let toml_str = r#"
        [filters]
        mode = "invalid_mode_value"
    "#;
    let res = Config::from_toml_str(toml_str);
    assert!(res.is_err());
    let err_msg = res.unwrap_err();
    assert!(err_msg.contains("invalid filter mode 'invalid_mode_value'"));
}

#[test]
fn test_candidate_paths_with_unset_and_empty_env_variables() {
    // Both unset
    let candidates = Config::candidate_config_paths_from_env(&None, &None);
    assert_eq!(candidates, vec![PathBuf::from(".opm.toml")]);

    // Both empty string
    let candidates = Config::candidate_config_paths_from_env(
        &Some(OsString::from("")),
        &Some(OsString::from("")),
    );
    assert_eq!(candidates, vec![PathBuf::from(".opm.toml")]);

    // Only XDG set
    let candidates = Config::candidate_config_paths_from_env(
        &Some(OsString::from("/custom/xdg")),
        &None,
    );
    assert_eq!(
        candidates,
        vec![
            PathBuf::from(".opm.toml"),
            PathBuf::from("/custom/xdg/opm/config.toml"),
        ]
    );

    // Only HOME set
    let candidates = Config::candidate_config_paths_from_env(
        &None,
        &Some(OsString::from("/custom/home")),
    );
    assert_eq!(
        candidates,
        vec![
            PathBuf::from(".opm.toml"),
            PathBuf::from("/custom/home/.config/opm/config.toml"),
        ]
    );

    // Both XDG and HOME set
    let candidates = Config::candidate_config_paths_from_env(
        &Some(OsString::from("/custom/xdg")),
        &Some(OsString::from("/custom/home")),
    );
    assert_eq!(
        candidates,
        vec![
            PathBuf::from(".opm.toml"),
            PathBuf::from("/custom/xdg/opm/config.toml"),
            PathBuf::from("/custom/home/.config/opm/config.toml"),
        ]
    );
}

#[test]
fn test_nonexistent_candidate_paths_return_defaults_without_warning() {
    let non_existent = vec![
        PathBuf::from("/non/existent/path/.opm.toml"),
        PathBuf::from("/another/non/existent/config.toml"),
    ];
    let load_res = Config::load_from_candidates(&non_existent);
    assert_eq!(load_res.source, None);
    assert_eq!(load_res.warning, None);
    assert_eq!(load_res.config.resolved_suspicious_names().len(), 13);
}

#[test]
fn test_real_file_loading_roundtrip() {
    let scratch_dir = get_test_scratch_dir();
    let file_path = scratch_dir.join(format!("test_opm_valid_{}.toml", std::process::id()));
    let content = r#"
        [filters]
        mode = "replace"
        suspicious_names = ["my-service"]
    "#;
    std::fs::write(&file_path, content).expect("write scratch file");

    let load_res = Config::load_from_candidates(std::slice::from_ref(&file_path));
    let _ = std::fs::remove_file(&file_path);

    assert_eq!(load_res.source, Some(file_path));
    assert!(load_res.warning.is_none());
    assert_eq!(load_res.config.resolved_suspicious_names(), vec!["my-service"]);
    assert!(load_res.config.matches("my-service"));
    assert!(!load_res.config.matches("node"));
}
