use rustview::ui::icons::{IconAsset, IconManager};

#[test]
fn test_icon_asset_metadata_and_spot_tint_rules() {
    // 1. Verify monochrome action/toolbar icons are tintable
    assert!(IconAsset::Settings.is_tintable());
    assert!(IconAsset::Refresh.is_tintable());
    assert!(IconAsset::GridView.is_tintable());
    assert!(IconAsset::ListView.is_tintable());
    assert!(IconAsset::Plus.is_tintable());
    assert!(IconAsset::Minus.is_tintable());
    assert!(IconAsset::CaretUp.is_tintable());
    assert!(IconAsset::CaretDown.is_tintable());
    assert!(IconAsset::Regex.is_tintable());
    assert!(IconAsset::QueryFolder.is_tintable());
    assert!(IconAsset::QueryFolderAndSubfolders.is_tintable());
    assert!(IconAsset::QueryGlobal.is_tintable());

    // 2. Verify multi-color semantic icons MUST NEVER be tinted
    assert!(!IconAsset::Tag.is_tintable());
    assert!(!IconAsset::Rating(0).is_tintable());
    assert!(!IconAsset::Rating(1).is_tintable());
    assert!(!IconAsset::Rating(2).is_tintable());
    assert!(!IconAsset::Rating(3).is_tintable());
    assert!(!IconAsset::Rating(4).is_tintable());
    assert!(!IconAsset::Rating(5).is_tintable());
}

#[test]
fn test_icon_manager_resolution_by_name() {
    assert_eq!(IconAsset::from_name("settings"), Some(IconAsset::Settings));
    assert_eq!(IconAsset::from_name("refresh"), Some(IconAsset::Refresh));
    assert_eq!(IconAsset::from_name("grid_view"), Some(IconAsset::GridView));
    assert_eq!(IconAsset::from_name("list_view"), Some(IconAsset::ListView));
    assert_eq!(IconAsset::from_name("tag"), Some(IconAsset::Tag));
    assert_eq!(IconAsset::from_name("rate_5"), Some(IconAsset::Rating(5)));
    assert_eq!(IconAsset::from_name("unknown_icon"), None);

    assert!(IconManager::is_tintable("settings"));
    assert!(!IconManager::is_tintable("tag"));
    assert!(!IconManager::is_tintable("rating_4"));
}

#[test]
fn test_all_svg_relative_paths_well_formed() {
    for asset in IconAsset::all() {
        let meta = asset.metadata();
        assert!(meta.relative_path.starts_with("ui/icons/"));
        assert!(meta.relative_path.ends_with(".svg"));
        assert!(!meta.description.is_empty());
    }
}
