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

// Collect every `vade.toml` file matching the glob pattern `examples/*/vade.toml`.
fn example_configs() -> Vec<PathBuf> {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");

    let mut configs = Vec::new();
    for entry in fs::read_dir(&examples).expect("failed to read examples directory") {
        let path = entry.unwrap().path();
        if path.is_dir() && path.file_name().is_some_and(|name| name != "timer") {
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
