use skiff_core::config;

// Its own test binary: SKIFF_CONFIG is process-wide.
#[test]
fn font_size_round_trips_beside_the_theme() {
    let dir = std::env::temp_dir().join(format!("skiff-appearance-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("projects.toml");
    std::env::set_var("SKIFF_CONFIG", &file);

    assert_eq!(config::load().unwrap().appearance.font_size, None);

    config::set_app_theme(Some("harbor")).unwrap();
    config::set_font_size(Some(15)).unwrap();
    let a = config::load().unwrap().appearance;
    assert_eq!(a.theme.as_deref(), Some("harbor"));
    assert_eq!(a.font_size, Some(15));

    config::set_font_size(None).unwrap();
    let a = config::load().unwrap().appearance;
    assert_eq!(a.theme.as_deref(), Some("harbor"));
    assert_eq!(a.font_size, None);

    std::fs::remove_dir_all(&dir).unwrap();
}
