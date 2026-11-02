//! 00001 D6's id checks that span API, Auth, and the database.

mod support;

use std::collections::HashMap;
use std::path::PathBuf;

use pipewhere_api::ids::{PREFIXES, new_id};
use support::Instance;

#[test]
fn no_prefix_is_shared_between_tables_in_auth_or_api() {
    let output = std::process::Command::new("node")
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auth"))
        .args(["--input-type=module", "-e"])
        .arg(r#"import { authPrefixes } from "./src/ids.ts"; process.stdout.write(JSON.stringify(authPrefixes));"#)
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let auth: HashMap<String, String> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!auth.is_empty());

    let tables = auth
        .iter()
        .map(|(model, prefix)| (format!("auth.{model}"), prefix.as_str()))
        .chain(
            PREFIXES
                .iter()
                .map(|(table, prefix)| (format!("pipewhere.{table}"), *prefix)),
        );
    let mut owners = HashMap::new();
    for (table, prefix) in tables {
        assert!(
            prefix.len() >= 3
                && prefix.len() <= 5
                && prefix.bytes().all(|b| b.is_ascii_lowercase()),
            "{table}'s prefix {prefix:?} is not 3 to 5 lowercase letters"
        );
        if let Some(other) = owners.insert(prefix, table.clone()) {
            panic!("{other} and {table} share the prefix {prefix:?}");
        }
    }
}

#[tokio::test]
async fn the_id_check_accepts_only_a_tables_own_ids() {
    let instance = Instance::start().await;
    let is_typeid = async |id: &str, prefix: &str| -> bool {
        sqlx::query_scalar("SELECT pipewhere.is_typeid($1, $2)")
            .bind(id)
            .bind(prefix)
            .fetch_one(&instance.api_db)
            .await
            .unwrap()
    };

    for (table, prefix) in PREFIXES {
        assert!(is_typeid(&new_id(table), prefix).await, "{table}");
    }
    let id = new_id("channel");
    let suffix = id.strip_prefix("chn_").unwrap();
    for rejected in [
        new_id("content"),
        id.to_uppercase(),
        format!("chn_8{}", &suffix[1..]),
        format!("chn_{}", &suffix[1..]),
        format!("chn_{suffix}0"),
        format!("chn_{}u", &suffix[..25]),
        format!("chn-{suffix}"),
    ] {
        assert!(!is_typeid(&rejected, "chn").await, "{rejected}");
    }
    assert!(!is_typeid("x_00000000000000000000000000", "x").await);
    instance.close().await;
}
