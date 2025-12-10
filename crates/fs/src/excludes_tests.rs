use blaze_runtime::DEFAULT_SYSTEM_SKIP_PREFIXES;

use super::*;
use std::path::Path;

#[test]
fn trash_config_add_root_and_is_in_trash_basic() {
    let mut cfg = TrashConfig::default();
    cfg.add_root(PathBuf::from("trash_root"));

    assert!(
        cfg.is_in_trash(Path::new("trash_root/file.txt")),
        "path inside added trash root should be considered trash",
    );
    assert!(
        !cfg.is_in_trash(Path::new("other/file.txt")),
        "unrelated path should not be considered trash",
    );
}

#[cfg(target_os = "windows")]
#[test]
fn trash_config_detects_recycle_bin_component_on_windows() {
    let cfg = TrashConfig::default();

    // Path containing "$Recycle.Bin" anywhere in its components
    let p = Path::new(r"C:\$Recycle.Bin\some\file.txt");
    assert!(
        cfg.is_in_trash(p),
        "paths containing $Recycle.Bin should be treated as trash on Windows",
    );
}

#[test]
fn ignore_options_default_values() {
    let opts = IgnoreOptions::default();
    assert!(opts.use_default_patterns);
    assert!(opts.extra_ignore_files.is_empty());
}

#[test]
fn ignore_engine_builds_without_defaults_and_does_not_ignore_arbitrary_path() {
    use tempfile::tempdir;

    let tmp = tempdir().expect("create temp dir");
    let root = tmp.path();

    // No default patterns, no extra ignore files
    let opts = IgnoreOptions {
        use_default_patterns: false,
        extra_ignore_files: Box::new([]),
    };

    let engine = IgnoreEngine::new(root, Some(opts)).expect("build ignore engine");

    let p = root.join("some_file.txt");
    assert!(
        !engine.is_ignored(&p, false),
        "engine with no patterns should not ignore arbitrary paths",
    );
}

#[test]
fn ignore_engine_respects_extra_ignore_files() {
    use std::io::Write;
    use tempfile::tempdir;

    let tmp = tempdir().expect("create temp dir");
    let root = tmp.path();

    let ignore_path = root.join(".blazeignore");
    {
        let mut f: _ = std::fs::File::create(&ignore_path).expect("create ignore file");
        writeln!(f, "foo").unwrap();
        writeln!(f, "bar/").unwrap();
    }

    let opts = IgnoreOptions {
        use_default_patterns: false,
        extra_ignore_files: vec![ignore_path].into_boxed_slice(),
    };

    let engine = IgnoreEngine::new(root, Some(opts)).expect("build ignore engine");

    let foo_file = root.join("foo");
    let bar_dir = root.join("bar");
    let other = root.join("baz");

    assert!(
        engine.is_ignored(&foo_file, false),
        "path matching 'foo' pattern should be ignored",
    );
    assert!(
        engine.is_ignored(&bar_dir, true),
        "directory matching 'bar/' pattern should be ignored",
    );
    assert!(
        !engine.is_ignored(&other, false),
        "unmatched path should not be ignored",
    );
}

#[test]
fn ignore_engine_with_defaults_constructs_successfully() {
    use tempfile::tempdir;

    let tmp = tempdir().expect("create temp dir");
    let root = tmp.path();

    // We don't assume anything about DEFAULT_PROJECT_IGNORE_PATTERNS here;
    // just ensure construction succeeds and is_ignored() is callable.
    let engine = IgnoreEngine::with_defaults(root).expect("build ignore engine with defaults");
    let p = root.join("some_file.txt");
    let _ = engine.is_ignored(&p, false);
}

#[test]
fn user_excludes_basic_inclusion() {
    let ux = UserExcludes::new(vec![PathBuf::from("root")]);

    assert!(
        ux.is_excluded(Path::new("root/file.txt")),
        "paths under an exclude root should be excluded",
    );
    assert!(
        !ux.is_excluded(Path::new("other/file.txt")),
        "paths outside exclude roots should not be excluded",
    );
}

#[test]
fn user_excludes_add_root_collapses_children_when_parent_added() {
    let mut ux = UserExcludes::new(Vec::new());

    ux.add_root(PathBuf::from("root/sub"));
    assert!(
        ux.is_excluded(Path::new("root/sub/file.txt")),
        "child root should exclude its subtree",
    );

    // Now add the parent; implementation should remove the child root
    ux.add_root(PathBuf::from("root"));

    // Still excluded, but now via the parent
    assert!(
        ux.is_excluded(Path::new("root/sub/file.txt")),
        "after adding parent, child paths should still be excluded",
    );

    // Nested under root is excluded
    assert!(ux.is_excluded(Path::new("root/other/file.txt")));

    // Outside root is not excluded
    assert!(!ux.is_excluded(Path::new("other/file.txt")));
}

#[test]
fn user_excludes_add_root_ignores_child_when_parent_already_present() {
    let mut ux = UserExcludes::new(vec![PathBuf::from("root")]);

    // Adding a child of an existing root should be a no-op
    ux.add_root(PathBuf::from("root/sub"));

    assert!(
        ux.is_excluded(Path::new("root/sub/file.txt")),
        "child paths remain excluded via existing parent root",
    );
    assert!(
        ux.is_excluded(Path::new("root/other/file.txt")),
        "other child paths remain excluded",
    );
}

#[test]
fn user_excludes_with_system_defaults_covers_configured_prefixes_with_canonicalization() {
    let ux = UserExcludes::with_system_defaults();

    for prefix in DEFAULT_SYSTEM_SKIP_PREFIXES {
        let base = PathBuf::from(prefix);

        let base_for_test = base.canonicalize().unwrap_or(base);
        let path = base_for_test.join("some_child");

        assert!(
            ux.is_excluded(&path),
            "system default prefix {:?} (canonicalized to {:?}) should exclude its subtree",
            prefix,
            base_for_test,
        );
    }
}
