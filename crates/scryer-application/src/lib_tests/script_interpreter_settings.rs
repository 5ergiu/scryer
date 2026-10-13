use super::*;

/// A failed read of one interpreter pin falls back only for that language;
/// the other configured pins still decide which interpreter runs.
#[tokio::test]
async fn unreadable_interpreter_pin_does_not_drop_other_pins() {
    let (mut app, _actor) = bootstrap();
    let settings = Arc::new(StoredSettingsRepo::default());
    app.services.config.settings = settings.clone();
    settings
        .set_value(
            SETTINGS_SCOPE_SYSTEM,
            crate::SCRIPT_INTERPRETER_PYTHON_KEY,
            "\"/opt/python/bin/python3\"",
        )
        .await;
    settings
        .set_value(
            SETTINGS_SCOPE_SYSTEM,
            crate::SCRIPT_INTERPRETER_GO_KEY,
            "\"/opt/go/bin/go\"",
        )
        .await;
    *settings.read_error_key.lock().await = Some(crate::SCRIPT_INTERPRETER_GO_KEY.into());

    let config = app.script_interpreter_config().await;

    assert_eq!(
        config.python.as_deref(),
        Some(Path::new("/opt/python/bin/python3"))
    );
    assert_eq!(config.go, None);
    assert_eq!(config.powershell, None);
    assert_eq!(config.batch, None);
}
