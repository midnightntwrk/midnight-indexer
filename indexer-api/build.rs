use anyhow::{Context, Result};
use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    vec,
};
use walkdir::WalkDir;

const PROTOS: &str = "proto";

fn main() -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is defined"));
    let protos = list_protos(Path::new(PROTOS))?;

    tonic_build::configure()
        .build_client(false)
        .file_descriptor_set_path(out_dir.join("midnight_indexer.bin"))
        .compile_protos(&protos, &[PROTOS])
        .context("compile protos")
}

fn list_protos(dir: &Path) -> Result<Vec<PathBuf>> {
    WalkDir::new(dir)
        .into_iter()
        .try_fold(vec![], |mut protos, entry| {
            let entry = entry.context("read proto file")?;
            let path = entry.path();
            if path.extension().and_then(OsStr::to_str) == Some("proto") {
                protos.push(path.to_path_buf());
            }
            Ok(protos)
        })
}
