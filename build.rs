#[path = "build_support/harness_identity.rs"]
mod harness_identity;

#[path = "crates/ca-automation/src/self_improvement/harness_protected.rs"]
mod harness_protected;

fn main() {
    let root = std::path::Path::new(".");
    let inputs = harness_identity::input_roots(root).expect("build identity inputs");
    // Existing input-directory watches cover additions/deletions. Adding a
    // workspace member changes Cargo.toml; do not watch absent optional paths
    // or target directories, which would force every subsequent build dirty.
    for path in &inputs {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let settings = std::env::vars()
        .filter(|(key, _)| harness_identity::relevant_setting(key))
        .collect();
    // Cargo itself tracks target/feature/profile changes. Track this override
    // explicitly without printing its value (or arbitrary process environment).
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    let compiler = std::process::Command::new(std::env::var_os("RUSTC").expect("Cargo rustc"))
        .arg("-vV")
        .output()
        .expect("read compiler identity");
    assert!(compiler.status.success(), "compiler identity unavailable");
    let digest = harness_identity::digest(root, &inputs, &settings, &compiler.stdout)
        .expect("hash build identity inputs");
    println!("cargo:rustc-env=CA_BUILD_DIGEST={digest}");
    let source_digest = harness_identity::digest(root, &inputs, &Default::default(), &[])
        .expect("hash source identity inputs");
    println!("cargo:rustc-env=CA_SOURCE_DIGEST={source_digest}");
    let authority =
        harness_identity::authority_roots(root, harness_protected::HARNESS_PROTECTED_SOURCE)
            .expect("harness authority inputs");
    for path in &authority {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let adapter = harness_identity::digest(root, &authority, &Default::default(), &[])
        .expect("hash harness authority inputs");
    println!("cargo:rustc-env=CA_HARNESS_ADAPTER_DIGEST={adapter}");
}
