//! Observation content identity: what a policy's ID embeddings mean. It is computed
//! only from fields of `content_manifest.json`, never from generated Rust source, so
//! a refactor that respells engine tables keeps checkpoints valid and any build's
//! manifest proves whether two builds agree. Shared with build.rs through `#[path]`.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub fn content_hash(manifest: &Value) -> [u8; 32] {
    Sha256::digest(serde_json::to_vec(&json!({
        "registries": manifest["registries"],
        "original_dll_capture": manifest["original_dll_capture"],
        "original_sources": manifest["original_sources"],
    })).unwrap()).into()
}

pub fn team_schema_hash(base:[u8;32])->[u8;32] {
    let team:Value=serde_json::from_str(include_str!("../team_schema.json")).unwrap();
    Sha256::digest(serde_json::to_vec(&json!({"base":base,"team":team})).unwrap()).into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn compiled_identity_is_the_hash_of_the_published_manifest() {
        let manifest: serde_json::Value = serde_json::from_str(crate::CONTENT_MANIFEST_JSON).unwrap();
        assert_eq!(super::content_hash(&manifest), crate::CONTENT_HASH);
        let mut respelled = manifest.clone();
        respelled["registries"][0]["ids"][0]["name"] = "Renamed".into();
        assert_ne!(super::content_hash(&respelled), crate::CONTENT_HASH);
    }
}
