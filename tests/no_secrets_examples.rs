use std::fs;
use std::path::PathBuf;

fn manifest_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn has_hex_token_like_value(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() >= 16 && trimmed.chars().all(|c| c.is_ascii_hexdigit())
}

fn has_bearer_hex_literal(s: &str) -> bool {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|w| w[0] == "Bearer" && has_hex_token_like_value(w[1]))
}

#[test]
fn relay_env_example_has_no_token_literal() {
    let env_example = fs::read_to_string(manifest_path("packaging/systemd/relay.env.example"))
        .expect("read relay.env.example");

    for line in env_example.lines() {
        if let Some(v) = line.strip_prefix("RELAY_TOKEN=") {
            assert!(
                v.trim().is_empty() || !has_hex_token_like_value(v),
                "relay.env.example contains token-like RELAY_TOKEN value"
            );
        }
    }
}

#[test]
fn packaging_and_runbook_do_not_embed_bearer_hex_literals() {
    let files = [
        "packaging/caddy/Caddyfile.example",
        "packaging/runbook_ubuntu.md",
        "README.md",
    ];

    for file in files {
        let contents = fs::read_to_string(manifest_path(file)).expect("read file");
        assert!(
            !has_bearer_hex_literal(&contents),
            "found token-like bearer literal in {file}"
        );
    }
}
