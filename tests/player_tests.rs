use rustview::player::{HwDecMode, Player, PlayerController, RenderConfig};

#[test]
fn test_libmpv_player_initialization_and_controls() {
    let config = RenderConfig {
        hwdec: HwDecMode::AutoSafe,
        video_output: "null".to_string(), // Headless output for test automation
        ..Default::default()
    };

    let controller =
        PlayerController::new(config).expect("Failed to initialize libmpv player controller");

    // Test Volume & Mute mutation
    assert!(controller.set_volume(75.0).is_ok());
    assert!((controller.raw_mpv().volume() - 75.0).abs() < 1.0);

    // Test Play / Pause / Stop states
    assert!(controller.pause().is_ok());
    assert!(controller.raw_mpv().is_paused());

    assert!(controller.play().is_ok());
    assert!(!controller.raw_mpv().is_paused());

    assert!(controller.stop().is_ok());
}
