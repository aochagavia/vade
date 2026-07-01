//! Integration test that runs the `deploy` command against every
//! example configuration used in `examples/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// Path to the compiled `vade` binary, provided by Cargo for integration tests.
const VADE_BIN: &str = env!("CARGO_BIN_EXE_vade");

#[test]
fn run_server_setup() {
    let out = fresh_out_dir("server-setup");
    run_vade(&["server-setup", "--out-dir", path_arg(&out)]);
    assert_execute_py_created(&out, None, "server-setup");
}

#[test]
fn run_create() {
    let out = fresh_out_dir("create");
    run_vade(&["create", "test-app", "--out-dir", path_arg(&out)]);
    assert_execute_py_created(&out, None, "create");
}

#[test]
fn examples_run_deploy() {
    let configs = example_configs();
    assert!(
        !configs.is_empty(),
        "no example `vade.toml` files were found"
    );

    for config in configs {
        let example_dir = config.parent().unwrap();
        let name = example_dir.file_name().unwrap().to_string_lossy();

        let deploy_out = fresh_out_dir(&format!("{name}-deploy"));
        run_vade(&[
            "deploy",
            "test-app",
            "--config",
            path_arg(&config),
            "--out-dir",
            path_arg(&deploy_out),
        ]);
        assert_execute_py_created(&deploy_out, Some(&config), "deploy");
    }
}

#[test]
fn deploy_applies_var_overrides() {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/python-no-deps/vade.toml");
    let out = fresh_out_dir("override-deploy");
    run_vade(&[
        "deploy",
        "test-app",
        "--config",
        path_arg(&config),
        "--out-dir",
        path_arg(&out),
        "--set",
        r#"caddyfile.vars.domains=["override.example.com"]"#,
        "--set",
        "systemd-unit[0].vars.exec_start=\"python3 /custom/main.py\"",
    ]);

    let caddyfile = fs::read_to_string(out.join("Caddyfile")).expect("Caddyfile was not generated");
    assert!(
        caddyfile.contains("override.example.com"),
        "Caddyfile did not use the overridden domain:\n{caddyfile}"
    );
    assert!(
        !caddyfile.contains("python-site.example.com"),
        "Caddyfile still contains the original domain:\n{caddyfile}"
    );

    let unit = fs::read_to_string(out.join("test-app.service")).expect("unit was not generated");
    assert!(
        unit.contains("ExecStart=python3 /custom/main.py"),
        "systemd unit did not use the overridden exec_start:\n{unit}"
    );
}

#[test]
fn deploy_rejects_var_for_missing_config() {
    // Caddyfile
    let stderr = run_vade_expect_deploy_error(
        "tests/resources/vade-empty.toml",
        &["--set", "caddyfile.vars.foo=42"],
    );

    insta::assert_snapshot!(stderr, @"
    Error:   × --set targets `caddyfile`, but the configuration does not have a
      │ `[caddyfile]` section
    ");

    // Systemd unit
    let stderr = run_vade_expect_deploy_error(
        "tests/resources/vade-empty.toml",
        &["--set", "systemd-unit[0].vars.foo=42"],
    );

    insta::assert_snapshot!(stderr, @"
    Error:   × --set targets `systemd-unit[0]`, but the configuration does not have a
      │ systemd unit at that index (the total number of systemd units is 0)
    ")
}

#[test]
fn deploy_rejects_malformed_overrides() {
    // Invalid format (not path=value)
    let stderr = run_vade_expect_deploy_error(
        "examples/python-no-deps/vade.toml",
        &["--set", "no-equals-sign-to-be-seen"],
    );

    insta::assert_snapshot!(stderr, @"
    error: invalid value 'no-equals-sign-to-be-seen' for '--set <PATH=JSON>': expected the format `<path>=<value>`

    For more information, try '--help'.
    ");

    // Invalid path
    let stderr =
        run_vade_expect_deploy_error("examples/python-no-deps/vade.toml", &["--set", "foo=42"]);

    insta::assert_snapshot!(stderr, @"
    error: invalid value 'foo=42' for '--set <PATH=JSON>': failed to parse path `foo`: it must start with `caddyfile.vars.` or `systemd-unit[<index>].vars.`

    For more information, try '--help'.
    ");

    // Valid path, invalid value
    let stderr = run_vade_expect_deploy_error(
        "examples/python-no-deps/vade.toml",
        &[
            "--set",
            "systemd-unit[0].vars.exec_start=this_string_is_missing_quotes",
        ],
    );

    insta::assert_snapshot!(stderr, @"
    error: invalid value 'systemd-unit[0].vars.exec_start=this_string_is_missing_quotes' for '--set <PATH=JSON>': failed to parse JSON in `this_string_is_missing_quotes`, expected ident at line 1 column 2

    For more information, try '--help'.
    ")
}

#[test]
fn deploy_with_bad_toml_raises_error() {
    let stderr = run_vade_expect_deploy_error("tests/resources/vade-bad-toml.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × failed to parse vade config file
       ╭─[<REPO_ROOT>/tests/resources/vade-bad-toml.toml:1:16]
     1 │ ╭─▶ [[systemd-unit]
     2 │ ├─▶ [systemd-unit.template]
       · ╰──── expected a right bracket, found a newline
     3 │     builtin = "webapp.service"
       ╰────
    "#);
}

#[test]
fn deploy_with_multiple_issues_raises_error() {
    let stderr = run_vade_expect_deploy_error("tests/resources/vade-multiple-errors.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × failed to parse vade config file
       ╭─[<REPO_ROOT>/tests/resources/vade-multiple-errors.toml:3:12]
     2 │ [systemd-unit.template]
     3 │ builtin = "webapp.service"
       ·            ───────┬──────
       ·                   ╰── conflicting template source: set only one of `builtin`, `file`, or `inline`
     4 │ inline = "oops, two sources"
       ·           ────────┬────────
       ·                   ╰── conflicting template source: set only one of `builtin`, `file`, or `inline`
     5 │ typo-key = 1
       · ────┬───
       ·     ╰── unexpected key `typo-key`
       ╰────
    "#);
}

#[test]
fn deploy_with_duplicate_unit_filenames_raises_error() {
    let stderr =
        run_vade_expect_deploy_error("tests/resources/vade-duplicate-unit-filenames.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × invalid vade config file
       ╭─[<REPO_ROOT>/tests/resources/vade-duplicate-unit-filenames.toml:3:1]
     2 │ # to the same file on the server
     3 │ [[systemd-unit]]
       · ────────┬────────
       ·         ╰── systemd unit filename `test-app.service` is declared multiple times, you can use the `filename-suffix` and `file-extension` properties to differentiate between them
     4 │ [systemd-unit.template]
     5 │ inline = "first"
     6 │ 
     7 │ [[systemd-unit]]
       · ────────┬────────
       ·         ╰── systemd unit filename `test-app.service` is declared multiple times, you can use the `filename-suffix` and `file-extension` properties to differentiate between them
     8 │ [systemd-unit.template]
     9 │ inline = "second"
       ╰────
    "#);
}

#[test]
fn deploy_with_invalid_artifacts_not_found_raises_error() {
    let stderr = run_vade_expect_deploy_error("tests/resources/vade-artifacts-not-found.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × failed to locate artifacts
       ╭─[tests/resources/vade-artifacts-not-found.toml:2:9]
     1 │ [artifacts]
     2 │ path = "nothing-here"
       ·         ──────┬─────
       ·               ╰── the provided path does not exist or is not a directory
       ╰────
      help: the artifacts path resolved to `<REPO_ROOT>/tests/
            resources/nothing-here`
    "#);
}

#[test]
fn deploy_with_invalid_inline_template_raises_error() {
    let stderr =
        run_vade_expect_deploy_error("tests/resources/vade-inline-template-error.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × failed to render jinja2 template for Caddyfile
       ╭─[tests/resources/vade-inline-template-error.toml:3:31]
     2 │ inline = """
     3 │ Oops... undefined variable {{ vars.kaboom }}
       ·                               ─────┬─────
       ·                                    ╰── undefined value
     4 │ """
       ╰────
      help: `kaboom` is a user-defined variable. Declare it in your `vade.toml`
            file under the relevant template's `vars`, e.g. `vars = { kaboom
            = ... }`, or inject it through the CLI using the `--set` option.
    "#);
}

#[test]
fn deploy_builtin_with_missing_var_raises_error() {
    let stderr = run_vade_expect_deploy_error(
        "tests/resources/vade-builtin-template-missing-var.toml",
        &[],
    );

    insta::assert_snapshot!(stderr, @"
    Error:   × failed to render jinja2 template for systemd unit
        ╭─[webapp.service (builtin systemd unit template):12:14]
     11 │ Type=simple
     12 │ ExecStart={{ vars.exec_start }}
        ·              ───────┬───────
        ·                     ╰── undefined value
     13 │ WorkingDirectory={{ vade.app.paths.storage }}
        ╰────
      help: `exec_start` is a user-defined variable. Declare it in your
            `vade.toml` file under the relevant template's `vars`, e.g. `vars =
            { exec_start = ... }`, or inject it through the CLI using the `--set`
            option.
    ");
}

#[test]
fn deploy_builtin_with_non_existing_name_raises_error() {
    let stderr =
        run_vade_expect_deploy_error("tests/resources/vade-builtin-template-not-found.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × unknown builtin template
       ╭─[tests/resources/vade-builtin-template-not-found.toml:4:12]
     3 │ # Note the missing `e` at the end
     4 │ builtin = "webapp.servic"
       ·            ──────┬──────
       ·                  ╰── there is no systemd unit template with this name
       ╰────
    "#);
}

#[test]
fn deploy_file_with_non_existing_template_raises_error() {
    let stderr =
        run_vade_expect_deploy_error("tests/resources/vade-file-template-not-found.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × failed to load template
       ╭─[tests/resources/vade-file-template-not-found.toml:3:9]
     2 │ [systemd-unit.template]
     3 │ file = "not-found.service"
       ·         ────────┬────────
       ·                 ╰── reading the file resulted in an error: No such file or directory (os error 2)
       ╰────
      help: the path resolved to `<REPO_ROOT>/tests/resources/not-
            found.service`
    "#);
}

#[test]
fn deploy_file_template_with_missing_var_raises_error() {
    let stderr =
        run_vade_expect_deploy_error("tests/resources/vade-file-template-missing-var.toml", &[]);

    insta::assert_snapshot!(stderr, @"
    Error:   × failed to render jinja2 template for systemd unit
       ╭─[<REPO_ROOT>/tests/resources/almost-empty.service:1:4]
     1 │ {{ vars.hey }}
       ·    ────┬───
       ·        ╰── undefined value
       ╰────
      help: `hey` is a user-defined variable. Declare it in your `vade.toml` file
            under the relevant template's `vars`, e.g. `vars = { hey = ... }`, or
            inject it through the CLI using the `--set` option.
    ");
}

#[test]
fn deploy_with_invalid_user_string_in_vade_toml_raises_error() {
    let stderr = run_vade_expect_deploy_error("tests/resources/vade-user-var-error.toml", &[]);

    insta::assert_snapshot!(stderr, @r#"
    Error:   × failed to render user-provided string
        ╭─[tests/resources/vade-user-var-error.toml:11:47]
     10 │ vars = {
     11 │   exec_start = "{{{ vade.app.artifacts.active }}/goatcounter serve -listen :{{ port('main') }}"
        ·                                               ┬
        ·                                               ╰── syntax error: unexpected `}`, expected `:`
     12 │ }
        ╰────
    "#);
}

#[test]
fn deploy_with_invalid_user_string_in_cli_flag_raises_error() {
    let stderr = run_vade_expect_deploy_error(
        "examples/goatcounter/vade.toml",
        &[
            "--set",
            "systemd-unit[0].vars.exec_start=\"hello {{ vars.world }}\"",
        ],
    );

    insta::assert_snapshot!(stderr, @"
    Error:   × failed to render user-provided string
       ╭────
     1 │ hello {{ vars.world }}
       ·          ─────┬────
       ·               ╰── undefined value
       ╰────
      help: 1. this template string was assigned to `systemd-
            unit[0].vars.exec_start` through the `--set` flag
            2. `world` is a user-defined variable. Declare it in your `vade.toml`
            file under the relevant template's `vars`, e.g. `vars = { world
            = ... }`, or inject it through the CLI using the `--set` option.
    ");
}

// Collect every `vade.toml` file matching the glob pattern `examples/*/vade.toml`.
fn example_configs() -> Vec<PathBuf> {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");

    let mut configs = Vec::new();
    for entry in fs::read_dir(&examples).expect("failed to read examples directory") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let candidate_path = path.join("vade.toml");
            if candidate_path.is_file() {
                configs.push(candidate_path);
            }
        }
    }

    configs.sort();
    configs
}

// Return a unique path for the specific example under test, and clean up whatever files might
// already be there
fn fresh_out_dir(dir_name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(dir_name);
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn run_vade(args: &[&str]) {
    let output = Command::new(VADE_BIN)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run the vade binary");

    assert!(
        output.status.success(),
        "`vade {}` failed with status {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn run_vade_expect_deploy_error(vade_toml: &str, extra_args: &[&str]) -> String {
    // Unrelated to this fn, but helps us avoid the noise of writing this line for each test
    bind_repo_root_filter();

    let out = fresh_out_dir("expect-deploy-error");
    let mut args = vec![
        "deploy",
        "test-app",
        "--config",
        vade_toml,
        "--out-dir",
        path_arg(&out),
    ];
    args.extend(extra_args);

    let output = Command::new(VADE_BIN)
        .args(&args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run the vade binary");

    assert!(!output.status.success(), "expected deploy to fail");

    String::from_utf8(output.stderr).unwrap()
}

fn assert_execute_py_created(out_dir: &Path, config: Option<&Path>, command: &str) {
    let execute_py = out_dir.join("execute.py");
    assert!(
        execute_py.is_file(),
        "`{command}` did not create `execute.py` for example `{}`",
        config
            .map(|c| c.display().to_string())
            .unwrap_or("<unknown>".into()),
    );
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("path is not valid UTF-8")
}

// Rewrite the repo root path in snapshots, because it is machine-specific and we want snapshots to
// be independent of the local setup
//
// Note: this is unfortunately brittle, because the path could potentially be cut in two if the line
// where it appears exceeds a certain length. If a newline ends up in the midst of the repo root path,
// the filter won't match it. If this ever becomes a real issue, we should configure vade to NOT wrap
// diagnostics when running the tests.
fn bind_repo_root_filter() {
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(&regex::escape(env!("CARGO_MANIFEST_DIR")), "<REPO_ROOT>");

    // Forget ensures the settings remain bound for the entire duration of the current
    // thread
    std::mem::forget(settings.bind_to_scope());
}
