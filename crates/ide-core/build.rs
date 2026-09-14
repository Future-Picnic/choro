use std::{env, fs, path::Path};

fn collect(root: &Path, dir: &Path, entries: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("read bundled Expert skills") {
        let entry = entry.expect("read skill entry");
        let path = entry.path();
        let kind = entry.file_type().expect("skill file type");
        assert!(
            !kind.is_symlink(),
            "bundled skills must not contain symlinks"
        );
        if kind.is_dir() {
            collect(root, &path, entries);
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            entries.push((relative, path.to_str().unwrap().to_owned()));
        }
    }
}

fn main() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/experts");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut entries = Vec::new();
    collect(&root, &root, &mut entries);
    entries.sort();
    let mut source = String::from("pub static ASSETS: &[(&str, &str)] = &[\n");
    for (relative, absolute) in entries {
        source.push_str(&format!("({relative:?}, include_str!({absolute:?})),\n"));
    }
    source.push_str("];\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("expert_assets.rs"),
        source,
    )
    .expect("embed Expert catalog");
}
