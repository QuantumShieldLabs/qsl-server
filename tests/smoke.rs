#[test]
fn limits_defaults_are_nonzero() {
    let limits = qsl_server::Limits::from_env();
    assert!(limits.max_body_bytes > 0);
    assert!(limits.max_queue_depth > 0);
}
