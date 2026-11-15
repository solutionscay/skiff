use skiff_core::config;

// Its own test binary: SKIFF_CONFIG is process-wide.
/// The Language picker writes client.toml, the file the window reads.
#[test]
fn language_is_written_where_the_window_reads_it() {
    let dir = std::env::temp_dir().join(format!("skiff-language-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("projects.toml");
    std::env::set_var("SKIFF_CONFIG", &file);
    std::fs::write(&file, "[appearance]\nlanguage = \"es\"\n").unwrap();

    // The first write copies projects.toml's language into client.toml.
    config::set_app_theme(Some("harbor")).unwrap();
    assert_eq!(config::client::load().unwrap().appearance.language.as_deref(), Some("es"));

    config::set_language(Some("zh")).unwrap();
    assert_eq!(config::client::load().unwrap().appearance.language.as_deref(), Some("zh"));
    config::set_language(None).unwrap();
    assert_eq!(config::client::load().unwrap().appearance.language, None);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "[appearance]\nlanguage = \"es\"\n");

    std::fs::remove_dir_all(&dir).unwrap();
}
