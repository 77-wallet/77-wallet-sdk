use std::{fs, path::Path};

fn visit_rs_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_rs_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

#[test]
fn wallet_api_should_not_directly_use_dao_v1() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let src_dir = Path::new(&manifest_dir).join("src");
    let mut rs_files = Vec::new();
    visit_rs_files(&src_dir, &mut rs_files).expect("failed to scan wallet-api/src");

    let mut violations = Vec::new();
    for file in rs_files {
        let content = fs::read_to_string(&file).expect("failed to read rust file");
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("DaoV1::") {
                violations.push(format!("{}:{}", file.display(), idx + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found direct DaoV1:: usage in wallet-api/src:\n{}",
        violations.join("\n")
    );
}

#[test]
fn wallet_domain_should_not_directly_use_infrastructure() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let file = Path::new(&manifest_dir).join("src/domain/wallet/mod.rs");
    let content = fs::read_to_string(&file).expect("failed to read wallet domain file");

    let forbidden = [
        "Context::",
        "core_pool()",
        "api_wallet_pool()",
        "get_global_backend_api",
        "get_global_dirs",
        "WalletRepo::",
        "AccountRepo::",
        "DeviceRepo::",
        "Tasks::",
        "BackendApiTask",
        "BackendApiTaskData",
    ];

    let mut violations = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        for needle in forbidden {
            if trimmed.contains(needle) {
                violations.push(format!("{}:{} contains {}", file.display(), idx + 1, needle));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found direct infrastructure usage in wallet domain:\n{}",
        violations.join("\n")
    );
}
