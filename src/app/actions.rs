use crate::app::{App, PreviousPage};
use crate::application::{network, AppAction};
use crate::infrastructure::{media, persistence};
use crate::presentation::tui::{
    DynamicDetailPage, DynamicPage, HistoryPage, HomePage, LiveDetailPage, LivePage, LoginPage,
    NavItem, Page, SearchPage, SettingsPage, Theme,
};

impl App {
    fn login_required_message() -> String {
        "该功能需要登录，请前往设置页登录".to_string()
    }

    fn apply_login_required_hint(&mut self) {
        let msg = Self::login_required_message();
        match &mut self.current_page {
            Page::Dynamic(page) => {
                page.loading_up_list = false;
                page.set_error(msg);
            }
            Page::History(page) => page.apply_load_more_error(msg),
            Page::VideoDetail(page) => {
                page.error_message = Some(msg);
                page.loading = false;
            }
            Page::DynamicDetail(page) => {
                page.error_message = Some(msg);
                page.loading = false;
            }
            _ => {}
        }
    }

    /// 记录当前页面以便返回导航
    fn save_previous_page(&mut self) {
        self.previous_page = match &self.current_page {
            Page::Home(_) => Some(PreviousPage::Home),
            Page::Search(_) => Some(PreviousPage::Search),
            Page::Dynamic(_) => Some(PreviousPage::Dynamic),
            Page::History(_) => Some(PreviousPage::History),
            Page::Live(_) => Some(PreviousPage::Live),
            _ => None,
        };
    }

    pub(super) async fn handle_action(&mut self, action: AppAction) {
        match action {
            AppAction::Quit => self.should_quit = true,
            AppAction::SwitchToHome => {
                self.sidebar.select(NavItem::Home);
                // Use cached home page if available
                if let Some(cached) = self.cached_home.take() {
                    self.current_page = Page::Home(cached);
                } else {
                    self.current_page = Page::Home(HomePage::new());
                    self.init_current_page().await;
                }
            }
            AppAction::RefreshHome => {
                self.sidebar.select(NavItem::Home);
                // Clear cache and create fresh home page
                self.cached_home = None;
                self.current_page = Page::Home(HomePage::new());
                self.init_current_page().await;
            }
            AppAction::SwitchToLogin => {
                self.current_page = Page::Login(LoginPage::new());
                self.init_current_page().await;
            }
            AppAction::LoginSuccess(creds) => {
                // Save credentials
                let _ = persistence::save_credentials(&creds);
                self.credentials = Some(creds.clone());
                // Update API client with new cookies
                {
                    let client = self.api_client.clone();
                    client.set_credentials(&creds);
                }
                // Switch to home
                self.current_page = Page::Home(HomePage::new());
                self.init_current_page().await;
            }
            AppAction::PlayVideo {
                bvid,
                aid,
                cid,
                duration,
            } => {
                let api_client = self.api_client.clone();
                let _ = media::play_video(
                    api_client,
                    &bvid,
                    aid,
                    cid,
                    duration,
                    None,
                    self.credentials.as_ref(),
                )
                .await;
            }
            AppAction::PlayVideoWithPages {
                bvid,
                aid,
                pages,
                current_index,
            } => {
                // Play only the selected episode
                if current_index < pages.len() {
                    let page = &pages[current_index];
                    let api_client = self.api_client.clone();
                    let _ = media::play_video(
                        api_client,
                        &bvid,
                        aid,
                        page.cid,
                        page.duration,
                        Some(page.page),
                        self.credentials.as_ref(),
                    )
                    .await;
                    // Update current page index in video detail page
                    if let Page::VideoDetail(detail_page) = &mut self.current_page {
                        if detail_page.bvid == bvid {
                            detail_page.current_page_index = current_index;
                        }
                    }
                }
            }
            AppAction::NavNext => {
                // Don't navigate if on video detail page
                if !matches!(self.current_page, Page::VideoDetail(_)) {
                    self.sidebar.next();
                    self.switch_to_nav_page().await;
                }
            }
            AppAction::NavPrev => {
                if !matches!(self.current_page, Page::VideoDetail(_)) {
                    self.sidebar.prev();
                    self.switch_to_nav_page().await;
                }
            }
            AppAction::Search(keyword) => {
                if let Page::Search(page) = &mut self.current_page {
                    page.query = keyword.clone();
                    page.page = 1;
                    page.loading = true;
                    page.show_hot_list = false;
                    let req_id = self.next_request_id("search");
                    self.send_network_command(network::NetworkCommand::Search {
                        req_id,
                        keyword,
                        page: 1,
                    });
                }
            }
            AppAction::RefreshDynamic => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                if let Page::Dynamic(page) = &mut self.current_page {
                    page.loading = true;
                    let tab = page.current_tab;
                    let host_mid = page.get_selected_up_mid();
                    let req_id = self.next_request_id("dynamic_refresh");
                    self.send_network_command(network::NetworkCommand::LoadDynamicRefresh {
                        req_id,
                        tab,
                        host_mid,
                    });
                }
            }
            AppAction::OpenVideoDetail(bvid, aid) => {
                self.save_previous_page();
                // Cache home page before navigating to video detail
                if let Page::Home(home_page) =
                    std::mem::replace(&mut self.current_page, Page::Home(HomePage::new()))
                {
                    self.cached_home = Some(home_page);
                }
                let detail_page = crate::presentation::tui::VideoDetailPage::new(bvid.clone(), aid);
                self.current_page = Page::VideoDetail(Box::new(detail_page));
                let req_id = self.next_request_id("video_detail");
                self.send_network_command(network::NetworkCommand::LoadVideoDetail {
                    req_id,
                    bvid,
                    aid,
                });
            }
            AppAction::OpenDynamicDetail(dynamic_id) => {
                self.save_previous_page();
                // Cache home page before navigating to dynamic detail
                if let Page::Home(home_page) =
                    std::mem::replace(&mut self.current_page, Page::Home(HomePage::new()))
                {
                    self.cached_home = Some(home_page);
                }
                let detail_page = DynamicDetailPage::new(dynamic_id.clone());
                self.current_page = Page::DynamicDetail(Box::new(detail_page));
                let req_id = self.next_request_id("dynamic_detail");
                self.send_network_command(network::NetworkCommand::LoadDynamicDetail {
                    req_id,
                    dynamic_id,
                });
            }
            AppAction::BackToList => match self.previous_page.take() {
                Some(PreviousPage::Home) => {
                    self.sidebar.select(NavItem::Home);
                    // Use cached home page if available
                    if let Some(cached) = self.cached_home.take() {
                        self.current_page = Page::Home(cached);
                    } else {
                        self.current_page = Page::Home(HomePage::new());
                        self.init_current_page().await;
                    }
                }
                Some(PreviousPage::Search) => {
                    self.sidebar.select(NavItem::Search);
                    self.current_page = Page::Search(SearchPage::new());
                    self.init_current_page().await;
                }
                Some(PreviousPage::Dynamic) => {
                    self.sidebar.select(NavItem::Dynamic);
                    self.current_page = Page::Dynamic(DynamicPage::new());
                    self.init_current_page().await;
                }
                Some(PreviousPage::History) => {
                    self.sidebar.select(NavItem::History);
                    self.current_page = Page::History(HistoryPage::new());
                    self.init_current_page().await;
                }
                Some(PreviousPage::Live) => {
                    self.sidebar.select(NavItem::Live);
                    self.current_page = Page::Live(LivePage::new());
                    self.init_current_page().await;
                }
                None => {
                    // Default to home
                    self.sidebar.select(NavItem::Home);
                    if let Some(cached) = self.cached_home.take() {
                        self.current_page = Page::Home(cached);
                    } else {
                        self.current_page = Page::Home(HomePage::new());
                        self.init_current_page().await;
                    }
                }
            },
            AppAction::LoadMoreRecommendations => {
                if let Page::Home(page) = &mut self.current_page {
                    if let Some(fresh_idx) = page.begin_load_more() {
                        let req_id = self.next_request_id("home_more");
                        self.send_network_command(network::NetworkCommand::LoadHomeMore {
                            req_id,
                            fresh_idx,
                            use_guest_feed: self.credentials.is_none(),
                        });
                    }
                }
            }
            AppAction::LoadMoreSearch => {
                let mut command = None;
                if let Page::Search(page) = &mut self.current_page {
                    if page.loading_more || page.query.is_empty() || page.show_hot_list {
                        return;
                    }
                    if page.grid.cards.len() >= page.total_results as usize {
                        return;
                    }
                    page.loading_more = true;
                    let next_page = page.page + 1;
                    command = Some((page.query.clone(), next_page));
                }
                if let Some((keyword, next_page)) = command {
                    let req_id = self.next_request_id("search");
                    self.send_network_command(network::NetworkCommand::Search {
                        req_id,
                        keyword,
                        page: next_page,
                    });
                }
            }
            AppAction::LoadMoreDynamic => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                let mut command = None;
                if let Page::Dynamic(page) = &mut self.current_page {
                    if page.loading_more || !page.has_more {
                        return;
                    }
                    let Some(offset) = page.offset.clone() else {
                        return;
                    };
                    page.loading_more = true;
                    command = Some((offset, page.current_tab, page.get_selected_up_mid()));
                }
                if let Some((offset, tab, host_mid)) = command {
                    let req_id = self.next_request_id("dynamic_more");
                    self.send_network_command(network::NetworkCommand::LoadDynamicMore {
                        req_id,
                        offset,
                        tab,
                        host_mid,
                    });
                }
            }
            AppAction::LoadMoreHistory => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                if let Page::History(page) = &mut self.current_page {
                    if let Some(cursor) = page.start_load_more_request() {
                        let req_id = self.next_request_id("history_more");
                        self.send_network_command(network::NetworkCommand::LoadHistoryMore {
                            req_id,
                            cursor,
                        });
                    }
                }
            }
            AppAction::SwitchToHistory => {
                self.sidebar.select(NavItem::History);
                self.current_page = Page::History(HistoryPage::new());
                self.init_current_page().await;
            }
            AppAction::LoadMoreComments => {
                if let Page::VideoDetail(page) = &mut self.current_page {
                    let client = self.api_client.clone();
                    page.load_more_comments(&client).await;
                } else if let Page::DynamicDetail(page) = &mut self.current_page {
                    let client = self.api_client.clone();
                    page.load_more_comments(&client).await;
                }
            }
            AppAction::ToggleCommentReplies => {
                if let Page::VideoDetail(page) = &mut self.current_page {
                    let client = self.api_client.clone();
                    page.toggle_comment_replies(&client).await;
                }
            }
            AppAction::SwitchDynamicTab(tab) => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                let mut command = None;
                if let Page::Dynamic(page) = &mut self.current_page {
                    page.switch_tab(tab);
                    let host_mid = page.get_selected_up_mid();
                    command = Some((page.current_tab, host_mid));
                }
                if let Some((tab, host_mid)) = command {
                    let req_id = self.next_request_id("dynamic_refresh");
                    self.send_network_command(network::NetworkCommand::LoadDynamicRefresh {
                        req_id,
                        tab,
                        host_mid,
                    });
                }
            }
            AppAction::SelectUpMaster(index) => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                let mut command = None;
                if let Page::Dynamic(page) = &mut self.current_page {
                    page.select_up(index);
                    let host_mid = page.get_selected_up_mid();
                    command = Some((page.current_tab, host_mid));
                }
                if let Some((tab, host_mid)) = command {
                    let req_id = self.next_request_id("dynamic_refresh");
                    self.send_network_command(network::NetworkCommand::LoadDynamicRefresh {
                        req_id,
                        tab,
                        host_mid,
                    });
                }
            }
            AppAction::NextTheme => {
                self.theme_id = Theme::next_theme_id(&self.theme_id);
                self.theme = Theme::load_or_default(&self.theme_id).0;
                self.save_theme_to_config();
            }
            AppAction::SetTheme(theme_id) => {
                self.theme_id = theme_id;
                self.theme = Theme::load_or_default(&self.theme_id).0;
                self.save_theme_to_config();
            }
            AppAction::SwitchToSettings => {
                self.sidebar.select(NavItem::Settings);
                let page = SettingsPage::new(
                    self.keybindings.clone(),
                    self.theme_id.clone(),
                    self.credentials.is_some(),
                );
                self.current_page = Page::Settings(Box::new(page));
            }
            AppAction::Logout => {
                let _ = persistence::delete_credentials();
                self.credentials = None;
                self.api_client.clear_credentials();
                self.cached_home = None;
                self.current_page = Page::Home(HomePage::new());
                self.init_current_page().await;
            }
            AppAction::LikeComment {
                oid,
                rpid,
                comment_type,
            } => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                let client = self.api_client.clone();
                // Toggle like - if already liked, unlike
                if let Page::VideoDetail(page) = &mut self.current_page {
                    let is_liked = page.liked_comments.contains(&rpid);
                    if let Ok(()) = client
                        .like_comment(oid, rpid, comment_type, !is_liked)
                        .await
                    {
                        if is_liked {
                            page.liked_comments.remove(&rpid);
                        } else {
                            page.liked_comments.insert(rpid);
                        }
                    }
                } else if let Page::DynamicDetail(page) = &mut self.current_page {
                    let is_liked = page.liked_comments.contains(&rpid);
                    if let Ok(()) = client
                        .like_comment(oid, rpid, comment_type, !is_liked)
                        .await
                    {
                        if is_liked {
                            page.liked_comments.remove(&rpid);
                        } else {
                            page.liked_comments.insert(rpid);
                        }
                    }
                }
            }
            AppAction::AddComment {
                oid,
                comment_type,
                message,
                root,
            } => {
                if self.credentials.is_none() {
                    self.apply_login_required_hint();
                    return;
                }
                let client = self.api_client.clone();
                if let Ok(_response) = client
                    .add_comment(oid, comment_type, &message, root, root)
                    .await
                {
                    // Reload comments to show new comment
                    if let Page::VideoDetail(page) = &mut self.current_page {
                        page.load_data(&client).await;
                    } else if let Page::DynamicDetail(page) = &mut self.current_page {
                        page.load_data(&client).await;
                    }
                }
            }
            AppAction::SaveKeybindings(new_keybindings) => {
                self.keybindings = (*new_keybindings).clone();
                self.config.keybindings = *new_keybindings;
                let _ = persistence::save_config(&self.config);
            }
            AppAction::SwitchToLive => {
                self.sidebar.select(NavItem::Live);
                self.current_page = Page::Live(LivePage::new());
                self.init_current_page().await;
            }
            AppAction::OpenLiveDetail(room_id) => {
                self.save_previous_page();
                let mut detail_page = LiveDetailPage::new(room_id);
                let client = &self.api_client;
                detail_page.load_room_info(client).await;
                // Connect WebSocket for real-time messages
                let uid = self
                    .credentials
                    .as_ref()
                    .and_then(|c| c.dede_user_id.parse::<i64>().ok())
                    .unwrap_or(0);
                detail_page.connect_ws(client, uid).await;
                self.current_page = Page::LiveDetail(Box::new(detail_page));
            }
            AppAction::RefreshLive => {
                if let Page::Live(page) = &mut self.current_page {
                    page.begin_loading();
                    let req_id = self.next_request_id("live_init");
                    self.send_network_command(network::NetworkCommand::LoadLiveInit { req_id });
                }
            }
            AppAction::LoadMoreLive => {
                if let Page::Live(page) = &mut self.current_page {
                    if page.begin_load_more() {
                        let req_id = self.next_request_id("live_more");
                        self.send_network_command(network::NetworkCommand::LoadLiveMore { req_id });
                    }
                }
            }
            AppAction::PlayLive { room_id, title: _ } => {
                let _ = media::play_live(room_id).await;
            }
            AppAction::None => {}
        }
    }

    async fn switch_to_nav_page(&mut self) {
        // First, cache home page if we're leaving it
        if matches!(self.current_page, Page::Home(_)) && self.sidebar.selected != NavItem::Home {
            if let Page::Home(home_page) =
                std::mem::replace(&mut self.current_page, Page::Home(HomePage::new()))
            {
                self.cached_home = Some(home_page);
            }
        }

        match self.sidebar.selected {
            NavItem::Home => {
                if !matches!(self.current_page, Page::Home(_)) {
                    // Use cached home page if available
                    if let Some(cached) = self.cached_home.take() {
                        self.current_page = Page::Home(cached);
                    } else {
                        self.current_page = Page::Home(HomePage::new());
                        self.init_current_page().await;
                    }
                }
            }
            NavItem::Search => {
                if !matches!(self.current_page, Page::Search(_)) {
                    self.current_page = Page::Search(SearchPage::new());
                    self.init_current_page().await;
                }
            }
            NavItem::Dynamic => {
                if !matches!(self.current_page, Page::Dynamic(_)) {
                    self.current_page = Page::Dynamic(DynamicPage::new());
                    self.init_current_page().await;
                }
            }
            NavItem::History => {
                if !matches!(self.current_page, Page::History(_)) {
                    self.current_page = Page::History(HistoryPage::new());
                    self.init_current_page().await;
                }
            }
            NavItem::Settings => {
                if !matches!(self.current_page, Page::Settings(_)) {
                    let page = SettingsPage::new(
                        self.keybindings.clone(),
                        self.theme_id.clone(),
                        self.credentials.is_some(),
                    );
                    self.current_page = Page::Settings(Box::new(page));
                }
            }
            NavItem::Live => {
                if !matches!(self.current_page, Page::Live(_)) {
                    self.current_page = Page::Live(LivePage::new());
                    self.init_current_page().await;
                }
            }
        }
    }

    pub(super) async fn init_current_page(&mut self) {
        match &mut self.current_page {
            Page::Login(page) => {
                let client = self.api_client.clone();
                page.load_qrcode(&client).await;
            }
            Page::Home(page) => {
                page.begin_loading();
                let req_id = self.next_request_id("home");
                self.send_network_command(network::NetworkCommand::LoadHome {
                    req_id,
                    use_guest_feed: self.credentials.is_none(),
                });
            }
            Page::Search(page) => {
                page.start_hotword_loading();
                let req_id = self.next_request_id("hotwords");
                self.send_network_command(network::NetworkCommand::LoadHotwords { req_id });
            }
            Page::Dynamic(page) => {
                if self.credentials.is_none() {
                    page.loading_up_list = false;
                    page.set_error(Self::login_required_message());
                    return;
                }
                page.loading_up_list = true;
                let tab = page.current_tab;
                let host_mid = page.get_selected_up_mid();
                let req_id = self.next_request_id("dynamic_init");
                self.send_network_command(network::NetworkCommand::LoadDynamicInit {
                    req_id,
                    tab,
                    host_mid,
                });
            }
            Page::VideoDetail(_) => {
                // VideoDetail is initialized when created
            }
            Page::DynamicDetail(_) => {
                // DynamicDetail is initialized when created
            }
            Page::History(page) => {
                if self.credentials.is_none() {
                    page.apply_load_more_error(Self::login_required_message());
                    return;
                }
                page.begin_loading();
                let req_id = self.next_request_id("history_init");
                self.send_network_command(network::NetworkCommand::LoadHistoryInit { req_id });
            }
            Page::Live(page) => {
                page.begin_loading();
                let req_id = self.next_request_id("live_init");
                self.send_network_command(network::NetworkCommand::LoadLiveInit { req_id });
            }
            Page::LiveDetail(page) => {
                let client = self.api_client.clone();
                page.load_room_info(&client).await;
            }
            Page::Settings(_) => {
                // Settings doesn't need async initialization
            }
        }
    }

    fn save_theme_to_config(&mut self) {
        self.config.theme = self.theme_id.clone();
        if persistence::save_config(&self.config).is_err() {}
    }
}
