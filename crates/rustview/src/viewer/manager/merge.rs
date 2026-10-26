//! Merging tabs into one Multiview tab, and closing a set of tabs.

use super::ViewerTabManager;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance};
use crate::viewer::multiview::route::{HostMode, OriginKind};
use crate::viewer::tab_item::TabTitleFormatter;
use crate::viewer::tab_merge::{self, MergeError, MergeSource, MergedTabs};
use crate::viewer::types::{MULTIVIEW_TAB_MEDIA_ID, MediaTabSession, TabId};
use rustview_core::settings::schema::{MULTIVIEW_MAX_TILES, MULTIVIEW_TAB_TITLE};
use std::sync::atomic::Ordering;
use std::time::Instant;

impl ViewerTabManager {
    /// Puts the tiles of the tabs in `tab_ids` side by side in one new Multiview tab. Nothing is restarted: the videos with
    /// their running players and surfaces move into the new Multiview, images keep their pixels. The new tab takes the place of the first of
    /// the tabs, the merged tabs are closed (they are empty by then) and the new tab becomes the active one when `activate` is
    /// set or when the active tab was one of the merged. Refused, and nothing changed, when fewer than two tabs are given, the
    /// tiles would not fit (`MULTIVIEW_MAX_TILES`) or a tab is shown in the fullscreen window.
    pub fn merge_tabs(&self, tab_ids: &[TabId], activate: bool) -> Result<MergedTabs, MergeError> {
        let sink = self.event_sink.lock().clone().ok_or(MergeError::NotReady)?;
        let sources: Vec<(TabId, SharedInstance)> = self
            .tabs
            .read()
            .all()
            .iter()
            .filter(|t| tab_ids.contains(&t.id))
            .map(|t| (t.id, t.multiview.instance.clone()))
            .collect();
        let counts: Vec<MergeSource> = sources
            .iter()
            .map(|(tab, instance)| MergeSource {
                tab: *tab,
                tiles: instance.lock().tile_count(),
            })
            .collect();
        let tiles = tab_merge::plan(&counts, MULTIVIEW_MAX_TILES).map_err(MergeError::Refused)?;
        if sources
            .iter()
            .any(|(_, instance)| instance.lock().host_mode() == HostMode::Fullscreen)
        {
            return Err(MergeError::ShownInFullscreen);
        }

        // The instance mutex is a leaf: each source is emptied on its own, the target takes everything afterwards.
        let target = MultiviewInstance::create(sink, Vec::new());
        target.lock().set_origin(OriginKind::Tab);
        let mut moved = Vec::with_capacity(tiles);
        let mut container = None;
        for (_, instance) in &sources {
            let released = instance.lock().release_tiles();
            container.get_or_insert(released.container);
            moved.extend(released.tiles);
        }
        if let Some(container) = container {
            target.lock().adopt_tiles(moved, container);
        }

        let closed: Vec<TabId> = sources.iter().map(|(tab, _)| *tab).collect();
        let new_id = {
            let mut tabs = self.tabs.write();
            let mut active = self.active_tab_id.write();
            let position = closed.first().and_then(|first| tabs.index_of(*first)).unwrap_or(0);
            let active_was_merged = active.is_some_and(|id| closed.contains(&id));
            for tab in &closed {
                if let Some((_, session)) = tabs.remove(*tab) {
                    Self::cleanup_session_resources(&session);
                }
            }
            let new_id = self.tab_counter.fetch_add(1, Ordering::Relaxed);
            let title = TabTitleFormatter::allocate_tab_title(tabs.all(), MULTIVIEW_TAB_MEDIA_ID, MULTIVIEW_TAB_TITLE);
            tabs.insert_at(position, MediaTabSession::for_multiview(new_id, title, target));
            if activate || active_was_merged {
                if let Some(previous) = *active
                    && let Some(tab) = tabs.get_mut(previous)
                {
                    Self::pause_session_playback(tab);
                }
                *active = Some(new_id);
                if let Some(tab) = tabs.get_mut(new_id) {
                    tab.last_accessed = Instant::now();
                }
            }
            new_id
        };
        tracing::info!(
            tab_id = new_id,
            merged = closed.len(),
            tiles,
            "[ViewerTabManager: Merge] Merged the selected tabs into one Multiview tab"
        );
        self.sync_tab_identities();
        self.tab_created();
        Ok(MergedTabs {
            tab: new_id,
            closed,
            tiles,
        })
    }

    /// Closes the tabs in `tab_ids`. When the active tab is among them, the tab that took its place becomes active (else the one
    /// before it). Returns how many tabs were closed and the active tab afterwards.
    pub fn close_tabs(&self, tab_ids: &[TabId]) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();
        let active_index = active.and_then(|id| tabs.index_of(id));
        let active_closed = active.is_some_and(|id| tab_ids.contains(&id));
        let before_active = active_index.map_or(0, |index| {
            tabs.all()[..index].iter().filter(|t| !tab_ids.contains(&t.id)).count()
        });

        let mut closed = 0;
        for tab in tab_ids {
            if let Some((_, session)) = tabs.remove(*tab) {
                Self::cleanup_session_resources(&session);
                closed += 1;
            }
        }
        if active_closed {
            *active = Self::resolve_next_active_tab(&mut tabs, before_active);
        }
        tracing::info!(
            closed,
            remaining = tabs.len(),
            "[ViewerTabManager: CloseTabs] Closed the given tabs"
        );
        (closed, *active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::instance::WeakInstance;
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use crate::viewer::tab_merge::MergeRefusal;
    use rustview_core::library::Library;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    struct Fixture {
        manager: ViewerTabManager,
        _dir: tempfile::TempDir,
    }

    fn seed(media: u64) -> TileSeed {
        TileSeed::new(
            media,
            TileKind::Image,
            1.5,
            PathBuf::from(format!("/nowhere/{media}")),
            format!("m{media}"),
        )
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let library = Arc::new(Library::open(dir.path()).expect("library"));
        let manager = ViewerTabManager::new(library);
        manager.set_event_sink(Arc::new(NullSink));
        Fixture { manager, _dir: dir }
    }

    impl Fixture {
        /// Adds ta tab that shows `media` (one tile each) and returns its id.
        fn tab(&self, title: &str, media: &[u64]) -> TabId {
            let id = self.manager.tab_counter.fetch_add(1, Ordering::Relaxed);
            let instance = MultiviewInstance::create(
                self.manager.event_sink.lock().clone().expect("sink"),
                media.iter().map(|m| seed(*m)).collect(),
            );
            instance.lock().set_origin(OriginKind::Tab);
            self.manager
                .tabs
                .write()
                .push(MediaTabSession::for_multiview(id, title.to_string(), instance));
            id
        }

        fn ids(&self) -> Vec<TabId> {
            self.manager.list_tabs().iter().map(|t| t.id).collect()
        }

        fn titles(&self) -> Vec<String> {
            self.manager.list_tabs().iter().map(|t| t.title.clone()).collect()
        }

        fn members(&self, tab: TabId) -> Vec<u64> {
            self.manager.get_tab(tab).expect("tab").member_ids()
        }
    }

    #[test]
    fn the_merged_tab_takes_the_place_of_the_first_selected_tab_and_holds_every_tile_in_tab_order() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let tc = fx.tab("c", &[3]);
        let td = fx.tab("d", &[4, 5]);
        let te = fx.tab("e", &[6]);

        // selected out of order: the result follows the tab bar
        let merged = fx.manager.merge_tabs(&[td, tb, tc], true).expect("merge");
        assert_eq!(merged.closed, vec![tb, tc, td]);
        assert_eq!(merged.tiles, 4);
        assert_eq!(fx.ids(), vec![ta, merged.tab, te]);
        assert_eq!(fx.members(merged.tab), vec![2, 3, 4, 5]);
        assert_eq!(fx.titles(), vec!["a", "Multiview", "e"]);
        assert_eq!(fx.manager.active_tab_id(), Some(merged.tab));
    }

    #[test]
    fn the_players_and_pictures_of_the_tiles_move_instead_of_being_recreated() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let keys_before: Vec<u64> = [ta, tb]
            .iter()
            .flat_map(|t| {
                let tab = fx.manager.get_tab(*t).expect("tab");
                let mv = tab.multiview.instance.lock();
                (0..mv.tile_count())
                    .filter_map(|i| mv.tile_key_at(i))
                    .collect::<Vec<_>>()
            })
            .collect();
        let merged = fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        let tab = fx.manager.get_tab(merged.tab).expect("tab");
        let mv = tab.multiview.instance.lock();
        let keys_after: Vec<u64> = (0..mv.tile_count()).filter_map(|i| mv.tile_key_at(i)).collect();
        assert_eq!(keys_after, keys_before, "the same tiles, not new ones");
        assert_eq!(mv.origin(), OriginKind::Tab);
    }

    #[test]
    fn a_medium_in_two_tabs_gives_two_tiles() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("a (1)", &[1]);
        let merged = fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        assert_eq!(merged.tiles, 2);
        let tab = fx.manager.get_tab(merged.tab).expect("tab");
        assert_eq!(tab.multiview.instance.lock().tile_count(), 2);
    }

    #[test]
    fn too_many_media_refuse_the_merge_and_change_nothing() {
        let fx = fixture();
        let ta = fx.tab("a", &[1, 2]);
        let tb = fx.tab("b", &[3, 4]);
        let tc = fx.tab("c", &[5]);
        let before = fx.ids();
        let revision = fx.manager.tab_ui_revision();
        let Err(MergeError::Refused(refusal)) = fx.manager.merge_tabs(&[ta, tb, tc], true) else {
            panic!("the merge must be refused");
        };
        assert_eq!(refusal.excess(), 1);
        match refusal {
            MergeRefusal::TooManyMedia { total, cap, sources } => {
                assert_eq!((total, cap), (5, 4));
                assert_eq!(sources.iter().map(|s| s.tab).collect::<Vec<_>>(), vec![ta, tb, tc]);
                assert_eq!(sources.iter().map(|s| s.tiles).collect::<Vec<_>>(), vec![2, 2, 1]);
            }
            MergeRefusal::TooFewTabs => panic!("wrong refusal"),
        }
        assert_eq!(fx.ids(), before);
        assert_eq!(fx.members(ta), vec![1, 2]);
        assert_eq!(fx.members(tb), vec![3, 4]);
        assert_eq!(fx.members(tc), vec![5]);
        assert_eq!(fx.manager.tab_ui_revision(), revision, "no tab was created");
    }

    #[test]
    fn fewer_than_two_existing_tabs_are_refused() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        assert_eq!(
            fx.manager.merge_tabs(&[ta], true),
            Err(MergeError::Refused(MergeRefusal::TooFewTabs))
        );
        assert_eq!(
            fx.manager.merge_tabs(&[ta, 999], true),
            Err(MergeError::Refused(MergeRefusal::TooFewTabs))
        );
        assert_eq!(fx.ids(), vec![ta]);
    }

    #[test]
    fn a_tab_in_the_fullscreen_is_not_merged() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let tab = fx.manager.get_tab(tb).expect("tab");
        tab.multiview
            .instance
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        assert_eq!(
            fx.manager.merge_tabs(&[ta, tb], true),
            Err(MergeError::ShownInFullscreen)
        );
        assert_eq!(fx.ids(), vec![ta, tb]);
        assert_eq!(fx.members(ta), vec![1]);
        assert_eq!(fx.members(tb), vec![2]);
    }

    #[test]
    fn the_merged_tab_is_named_like_a_new_multiview_and_numbered_when_the_name_is_taken() {
        let fx = fixture();
        let _existing = fx.tab("Multiview", &[9, 8]);
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let merged = fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        let title = ViewerTabManager::compute_tab_title(&fx.manager.list_tabs(), merged.tab);
        assert_eq!(title, "Multiview (1)");
    }

    #[test]
    fn a_merged_tab_that_was_named_multiview_does_not_make_the_new_one_a_number() {
        let fx = fixture();
        let ta = fx.tab("Multiview", &[1, 2]);
        let tb = fx.tab("b", &[3]);
        let merged = fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        let title = ViewerTabManager::compute_tab_title(&fx.manager.list_tabs(), merged.tab);
        assert_eq!(title, "Multiview");
    }

    #[test]
    fn the_active_tab_stays_unless_it_was_merged_or_the_caller_wants_the_result_shown() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let tc = fx.tab("c", &[3]);
        fx.manager.set_active_tab(tc);
        // the tab grid is open: the result is not activated, the active tab is not part of the merge
        let merged = fx.manager.merge_tabs(&[ta, tb], false).expect("merge");
        assert_eq!(fx.manager.active_tab_id(), Some(tc));
        assert_eq!(fx.ids(), vec![merged.tab, tc]);

        // the active tab is merged away: the result takes over even without the request
        fx.manager.set_active_tab(tc);
        let again = fx.manager.merge_tabs(&[merged.tab, tc], false).expect("merge");
        assert_eq!(fx.manager.active_tab_id(), Some(again.tab));
    }

    #[test]
    fn merging_from_the_browse_view_shows_the_result_when_asked() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        fx.manager.activate_browse();
        let merged = fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        assert_eq!(fx.manager.active_tab_id(), Some(merged.tab));
    }

    #[test]
    fn the_new_tab_announces_itself_to_the_tab_ui() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let revision = fx.manager.tab_ui_revision();
        fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        assert!(fx.manager.tab_ui_revision() > revision);
    }

    #[test]
    fn a_merge_of_all_tabs_leaves_exactly_the_new_one() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let merged = fx.manager.merge_tabs(&[ta, tb], true).expect("merge");
        assert_eq!(fx.ids(), vec![merged.tab]);
    }

    #[test]
    fn closing_tabs_moves_the_active_tab_to_the_one_that_took_its_place() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        let tc = fx.tab("c", &[3]);
        let td = fx.tab("d", &[4]);
        fx.manager.set_active_tab(tb);
        let (closed, active) = fx.manager.close_tabs(&[tb, tc]);
        assert_eq!(closed, 2);
        assert_eq!(active, Some(td));
        assert_eq!(fx.ids(), vec![ta, td]);

        fx.manager.set_active_tab(td);
        let (closed, active) = fx.manager.close_tabs(&[td]);
        assert_eq!((closed, active), (1, Some(ta)));
    }

    #[test]
    fn closing_tabs_keeps_an_active_tab_that_stays_and_ignores_unknown_ids() {
        let fx = fixture();
        let ta = fx.tab("a", &[1]);
        let tb = fx.tab("b", &[2]);
        fx.manager.set_active_tab(ta);
        let (closed, active) = fx.manager.close_tabs(&[tb, 12345]);
        assert_eq!((closed, active), (1, Some(ta)));
        let (closed, active) = fx.manager.close_tabs(&[ta]);
        assert_eq!((closed, active), (1, None));
        assert_eq!(fx.ids(), Vec::<TabId>::new());
    }
}
