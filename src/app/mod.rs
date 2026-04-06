mod action;
mod network;

pub use action::AppAction;

use crate::api::client::ApiClient;
use crate::storage::{AppConfig, Credentials, Keybindings};
use crate::ui::{
    Component, DynamicPage, HistoryPage, HomePage, LiveDetailPage, LivePage, LoginPage, NavItem,
    Page, SearchPage, SettingsPage, Sidebar, Theme, ThemeVariant, VideoCard, VideoDetailPage,
};
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent},
    prelude::*,
    DefaultTerminal, Frame,
};
use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::sync::Arc;

/// Previous page for back navigation
#[derive(Clone)]
pub enum PreviousPage {
    Home,
    Search,
    Dynamic,
    History,
    Live,
}

/// Main application state
pub struct App {
    pub current_page: Page,
    pub should_quit: bool,
    pub api_client: Arc<ApiClient>,
    pub credentials: Option<Credentials>,
    pub sidebar: Sidebar,
    pub show_sidebar: bool,

    pub previous_page: Option<PreviousPage>,
    pub theme: Theme,
    pub theme_variant: ThemeVariant,
    pub config: AppConfig,
    pub keybindings: Keybindings,

    /// Cached home page to avoid refresh when switching tabs
    pub cached_home: Option<HomePage>,
    network_command_tx: mpsc::Sender<network::NetworkCommand>,
    network_event_rx: mpsc::Receiver<network::NetworkEvent>,
    request_seq: u64,
    pending_requests: HashMap<&'static str, u64>,
}

impl App {
    pub fn new() -> Self {
        let credentials = crate::storage::load_credentials().ok();
        let api_client = if let Some(ref creds) = credentials {
            ApiClient::with_cookies(creds)
        } else {
            ApiClient::new()
        };
        let api_client = Arc::new(api_client);
        let bridge = network::start_network_worker(api_client.clone());

        // Load config and apply saved theme
        let config = crate::storage::load_config().unwrap_or_default();
        let keybindings = config.keybindings.clone();
        let theme_variant = config
            .theme
            .parse()
            .unwrap_or(ThemeVariant::CatppuccinMocha);
        let theme = Theme::from_variant(theme_variant);

        // Start on login page if no credentials, otherwise go to home
        let current_page = if credentials.is_some() {
            Page::Home(HomePage::new())
        } else {
            Page::Login(LoginPage::new())
        };

        Self {
            current_page,
            should_quit: false,
            api_client,
            credentials,
            sidebar: Sidebar::new(),
            show_sidebar: true,
            previous_page: None,
            theme,
            theme_variant,
            config,
            keybindings,
            cached_home: None,
            network_command_tx: bridge.command_tx,
            network_event_rx: bridge.event_rx,
            request_seq: 0,
            pending_requests: HashMap::new(),
        }
    }

    fn next_request_id(&mut self, key: &'static str) -> u64 {
        self.request_seq = self.request_seq.saturating_add(1);
        self.pending_requests.insert(key, self.request_seq);
        self.request_seq
    }

    fn is_latest_request(&self, key: &'static str, req_id: u64) -> bool {
        self.pending_requests
            .get(key)
            .is_some_and(|latest| *latest == req_id)
    }

    fn send_network_command(&self, command: network::NetworkCommand) {
        let _ = self.network_command_tx.send(command);
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

    /// Main run loop
    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        // Initialize the first page
        self.init_current_page().await;

        // Store the last content area for mouse handling
        let mut last_content_area = Rect::default();

        // Scroll accumulator for high-resolution mouse wheel throttling
        // Many modern mice generate multiple scroll events per physical "click"
        const SCROLL_THRESHOLD: i32 = 15; // Accumulate 15 events before scrolling
        let mut scroll_accumulator: i32 = 0;

        while !self.should_quit {
            terminal.draw(|frame| {
                last_content_area = self.get_content_area(frame.area());
                self.draw(frame);
            })?;

            if event::poll(std::time::Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            self.handle_input(key.code, key.modifiers).await;
                        }
                    }
                    Event::Mouse(mouse) => {
                        use crossterm::event::MouseEventKind;
                        match mouse.kind {
                            MouseEventKind::ScrollDown => {
                                scroll_accumulator += 1;
                                if scroll_accumulator >= SCROLL_THRESHOLD {
                                    scroll_accumulator = 0;
                                    self.handle_mouse(mouse, last_content_area).await;
                                }
                            }
                            MouseEventKind::ScrollUp => {
                                scroll_accumulator -= 1;
                                if scroll_accumulator <= -SCROLL_THRESHOLD {
                                    scroll_accumulator = 0;
                                    self.handle_mouse(mouse, last_content_area).await;
                                }
                            }
                            _ => {
                                // Other mouse events (clicks) are handled immediately
                                self.handle_mouse(mouse, last_content_area).await;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Handle background tasks (like QR code polling)
            self.tick().await;
        }
        Ok(())
    }

    /// Get the content area excluding sidebar
    fn get_content_area(&self, area: Rect) -> Rect {
        // Login page, VideoDetail, and DynamicDetail use full area
        if matches!(
            self.current_page,
            Page::Login(_) | Page::VideoDetail(_) | Page::DynamicDetail(_)
        ) {
            return area;
        }

        // Main layout with sidebar
        if self.show_sidebar {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(16), // Sidebar
                    Constraint::Min(40),    // Content
                ])
                .split(area)[1]
        } else {
            area
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Login page, VideoDetail, and DynamicDetail don't show sidebar
        if matches!(
            self.current_page,
            Page::Login(_) | Page::VideoDetail(_) | Page::DynamicDetail(_)
        ) {
            match &mut self.current_page {
                Page::Login(page) => page.draw(frame, area, &self.theme, &self.keybindings),
                Page::VideoDetail(page) => page.draw(frame, area, &self.theme, &self.keybindings),
                Page::DynamicDetail(page) => page.draw(frame, area, &self.theme, &self.keybindings),
                _ => {}
            }
            return;
        }

        // Main layout with sidebar
        let chunks = if self.show_sidebar {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(16), // Sidebar
                    Constraint::Min(40),    // Content
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(40)])
                .split(area)
        };

        if self.show_sidebar && chunks.len() > 1 {
            self.sidebar.draw(frame, chunks[0], &self.theme);
            self.draw_page(frame, chunks[1]);
        } else {
            self.draw_page(frame, chunks[0]);
        }
    }

    fn draw_page(&mut self, frame: &mut Frame, area: Rect) {
        match &mut self.current_page {
            Page::Login(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::Home(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::Search(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::Dynamic(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::DynamicDetail(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::VideoDetail(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::History(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::Live(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::LiveDetail(page) => page.draw(frame, area, &self.theme, &self.keybindings),
            Page::Settings(page) => page.draw(frame, area, &self.theme, &self.keybindings),
        }
    }

    async fn handle_input(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        let keys = &self.keybindings;
        let action = match &mut self.current_page {
            Page::Login(page) => page.handle_input(key, keys),
            Page::Home(page) => page.handle_input(key, keys),
            Page::Search(page) => page.handle_input(key, keys),
            Page::Dynamic(page) => page.handle_input_with_modifiers(key, modifiers, keys),
            Page::DynamicDetail(page) => page.handle_input(key, keys),
            Page::VideoDetail(page) => page.handle_input(key, keys),
            Page::History(page) => page.handle_input(key, keys),
            Page::Live(page) => page.handle_input(key, keys),
            Page::LiveDetail(page) => page.handle_input(key, keys),
            Page::Settings(page) => page.handle_input(key, keys),
        };

        if let Some(action) = action {
            self.handle_action(action).await;
        }
    }

    async fn handle_mouse(&mut self, event: MouseEvent, area: Rect) {
        let action = match &mut self.current_page {
            Page::Login(page) => page.handle_mouse(event, area),
            Page::Home(page) => page.handle_mouse(event, area),
            Page::Search(page) => page.handle_mouse(event, area),
            Page::Dynamic(page) => page.handle_mouse(event, area),
            Page::DynamicDetail(page) => page.handle_mouse(event, area),
            Page::VideoDetail(page) => page.handle_mouse(event, area),
            Page::History(page) => page.handle_mouse(event, area),
            Page::Live(page) => page.handle_mouse(event, area),
            Page::LiveDetail(page) => page.handle_mouse(event, area),
            Page::Settings(page) => page.handle_mouse(event, area),
        };

        if let Some(action) = action {
            self.handle_action(action).await;
        }
    }

    async fn handle_action(&mut self, action: AppAction) {
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
                let _ = crate::storage::save_credentials(&creds);
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
                let _ = crate::player::play_video(
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
                    let _ = crate::player::play_video(
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
                let detail_page = VideoDetailPage::new(bvid.clone(), aid);
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
                use crate::ui::DynamicDetailPage;
                let detail_page = DynamicDetailPage::new(dynamic_id.clone());
                self.current_page = Page::DynamicDetail(Box::new(detail_page));
                let req_id = self.next_request_id("dynamic_detail");
                self.send_network_command(network::NetworkCommand::LoadDynamicDetail {
                    req_id,
                    dynamic_id,
                });
            }
            AppAction::BackToList => {
                match self.previous_page.take() {
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
                }
            }
            AppAction::LoadMoreRecommendations => {
                if let Page::Home(page) = &mut self.current_page {
                    if let Some(fresh_idx) = page.begin_load_more() {
                        let req_id = self.next_request_id("home_more");
                        self.send_network_command(network::NetworkCommand::LoadHomeMore {
                            req_id,
                            fresh_idx,
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
                self.theme_variant = self.theme_variant.next();
                self.theme = Theme::from_variant(self.theme_variant);
                self.save_theme_to_config();
            }
            AppAction::SetTheme(variant) => {
                self.theme_variant = variant;
                self.theme = Theme::from_variant(variant);
                self.save_theme_to_config();
            }
            AppAction::SwitchToSettings => {
                self.sidebar.select(NavItem::Settings);
                let page = SettingsPage::new(self.keybindings.clone(), self.theme_variant);
                self.current_page = Page::Settings(Box::new(page));
            }
            AppAction::Logout => {
                let _ = crate::storage::delete_credentials();
                self.credentials = None;
                self.current_page = Page::Login(LoginPage::new());
                self.init_current_page().await;
            }
            AppAction::LikeComment {
                oid,
                rpid,
                comment_type,
            } => {
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
                let _ = crate::storage::save_config(&self.config);
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
                let _ = crate::player::play_live(room_id).await;
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
                    let page = SettingsPage::new(self.keybindings.clone(), self.theme_variant);
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

    async fn init_current_page(&mut self) {
        match &mut self.current_page {
            Page::Login(page) => {
                let client = self.api_client.clone();
                page.load_qrcode(&client).await;
            }
            Page::Home(page) => {
                page.begin_loading();
                let req_id = self.next_request_id("home");
                self.send_network_command(network::NetworkCommand::LoadHome { req_id });
            }
            Page::Search(page) => {
                page.start_hotword_loading();
                let req_id = self.next_request_id("hotwords");
                self.send_network_command(network::NetworkCommand::LoadHotwords { req_id });
            }
            Page::Dynamic(page) => {
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

    fn drain_network_events(&mut self) {
        while let Ok(event) = self.network_event_rx.try_recv() {
            self.handle_network_event(event);
        }
    }

    fn handle_network_event(&mut self, event: network::NetworkEvent) {
        match event {
            network::NetworkEvent::HomeLoaded { req_id, videos } => {
                if !self.is_latest_request("home", req_id) {
                    return;
                }
                if let Page::Home(page) = &mut self.current_page {
                    page.apply_recommendations(videos);
                }
            }
            network::NetworkEvent::HomeMoreLoaded { req_id, videos } => {
                if !self.is_latest_request("home_more", req_id) {
                    return;
                }
                if let Page::Home(page) = &mut self.current_page {
                    page.apply_load_more(videos);
                }
            }
            network::NetworkEvent::HotwordsLoaded { req_id, hotwords } => {
                if !self.is_latest_request("hotwords", req_id) {
                    return;
                }
                if let Page::Search(page) = &mut self.current_page {
                    page.set_hotwords(hotwords);
                }
            }
            network::NetworkEvent::SearchLoaded {
                req_id,
                keyword,
                page,
                results,
                total,
            } => {
                if !self.is_latest_request("search", req_id) {
                    return;
                }
                if let Page::Search(search_page) = &mut self.current_page {
                    if search_page.query != keyword {
                        return;
                    }
                    if page <= 1 {
                        search_page.page = 1;
                        search_page.set_results(results, total);
                    } else {
                        search_page.page = page;
                        search_page.total_results = total;
                        search_page.append_results(results);
                    }
                }
            }
            network::NetworkEvent::DynamicLoaded {
                req_id,
                append,
                up_list,
                items,
                offset,
                has_more,
            } => {
                let key = if append {
                    "dynamic_more"
                } else {
                    "dynamic_refresh"
                };
                if !self.is_latest_request(key, req_id)
                    && !self.is_latest_request("dynamic_init", req_id)
                {
                    return;
                }
                if let Page::Dynamic(page) = &mut self.current_page {
                    if let Some(up_list) = up_list {
                        page.set_up_list(up_list);
                    }
                    if append {
                        page.append_feed(items, offset, has_more);
                        page.loading_more = false;
                    } else {
                        page.set_feed(items, offset, has_more);
                    }
                }
            }
            network::NetworkEvent::HistoryLoaded {
                req_id,
                append,
                data,
            } => {
                let key = if append {
                    "history_more"
                } else {
                    "history_init"
                };
                if !self.is_latest_request(key, req_id) {
                    return;
                }
                if let Page::History(page) = &mut self.current_page {
                    if append {
                        page.apply_history_more(data);
                    } else {
                        page.apply_history_init(data);
                    }
                }
            }
            network::NetworkEvent::LiveLoaded {
                req_id,
                append,
                rooms,
            } => {
                let key = if append { "live_more" } else { "live_init" };
                if !self.is_latest_request(key, req_id) {
                    return;
                }
                if let Page::Live(page) = &mut self.current_page {
                    if append {
                        page.apply_live_more(rooms);
                    } else {
                        page.apply_live_init(rooms);
                    }
                }
            }
            network::NetworkEvent::VideoDetailLoaded {
                req_id,
                bvid,
                video_info,
                comments,
                has_more_comments,
                related_videos,
            } => {
                if !self.is_latest_request("video_detail", req_id) {
                    return;
                }
                if let Page::VideoDetail(page) = &mut self.current_page {
                    if page.bvid != bvid {
                        return;
                    }
                    page.video_info = Some(video_info);
                    page.comments = comments;
                    page.comment_page = 1;
                    page.has_more_comments = has_more_comments;
                    page.related_videos = related_videos.clone();
                    page.related_card_grid.clear();
                    for video in &related_videos {
                        let card = VideoCard::new(
                            video.bvid.clone(),
                            video.aid,
                            video.title.clone().unwrap_or_else(|| "无标题".to_string()),
                            video.author_name().to_string(),
                            video.format_views(),
                            video.format_duration(),
                            video.cover_url(),
                        );
                        page.related_card_grid.add_card(card);
                    }
                    page.loading = false;
                    page.error_message = None;
                }
            }
            network::NetworkEvent::DynamicDetailLoaded {
                req_id,
                dynamic_id,
                dynamic_item,
                comments,
                has_more_comments,
                image_urls,
            } => {
                if !self.is_latest_request("dynamic_detail", req_id) {
                    return;
                }
                if let Page::DynamicDetail(page) = &mut self.current_page {
                    if page.dynamic_id != dynamic_id {
                        return;
                    }
                    page.dynamic_item = Some(dynamic_item);
                    page.comments = comments;
                    page.comment_page = 1;
                    page.has_more_comments = has_more_comments;
                    page.image_urls = image_urls;
                    page.image_protocols = (0..page.image_urls.len()).map(|_| None).collect();
                    page.loading = false;
                    page.error_message = None;
                }
            }
            network::NetworkEvent::RequestFailed {
                req_id,
                target,
                error,
            } => {
                if !self.is_latest_request(target, req_id) {
                    return;
                }
                match (&mut self.current_page, target) {
                    (Page::Home(page), "home") => {
                        page.apply_recommendations_error(format!("加载推荐视频失败: {}", error))
                    }
                    (Page::Home(page), "home_more") => page.apply_load_more_error(),
                    (Page::Search(page), "hotwords") => {
                        page.set_hotword_error(format!("加载热搜失败: {}", error))
                    }
                    (Page::Search(page), "search") => {
                        page.set_error(format!("搜索失败: {}", error))
                    }
                    (Page::Dynamic(page), "dynamic_init")
                    | (Page::Dynamic(page), "dynamic_refresh")
                    | (Page::Dynamic(page), "dynamic_more") => {
                        page.loading_more = false;
                        page.set_error(format!("加载动态失败: {}", error));
                    }
                    (Page::History(page), "history_init") => {
                        page.apply_load_more_error(format!("加载历史记录失败: {}", error));
                    }
                    (Page::History(page), "history_more") => {
                        page.apply_load_more_error(format!("加载更多失败: {}", error));
                    }
                    (Page::Live(page), "live_init") => {
                        page.apply_live_init_error(format!("加载直播推荐失败: {}", error));
                    }
                    (Page::Live(page), "live_more") => page.apply_live_more_error(),
                    (Page::VideoDetail(page), "video_detail") => {
                        page.error_message = Some(format!("加载视频信息失败: {}", error));
                        page.loading = false;
                    }
                    (Page::DynamicDetail(page), "dynamic_detail") => {
                        page.error_message = Some(format!("加载动态详情失败: {}", error));
                        page.loading = false;
                    }
                    _ => {}
                }
            }
        }
    }

    async fn tick(&mut self) {
        self.drain_network_events();
        match &mut self.current_page {
            Page::Login(page) => {
                let client = &self.api_client;
                if let Some(action) = page.tick(client).await {
                    self.handle_action(action).await;
                }
            }
            Page::Home(page) => {
                // Non-blocking: poll completed downloads and start new ones
                page.poll_cover_results();
                page.start_cover_downloads();
            }
            Page::Search(page) => {
                page.poll_cover_results();
                page.start_cover_downloads();
            }
            Page::Dynamic(page) => {
                page.poll_cover_results();
                page.start_cover_downloads();
            }
            Page::VideoDetail(page) => {
                page.poll_cover_results();
                page.start_cover_downloads();
            }
            Page::History(page) => {
                page.poll_cover_results();
                page.start_cover_downloads();
            }
            _ => {}
        }
    }

    fn save_theme_to_config(&mut self) {
        self.config.theme = self.theme_variant.to_string();
        if crate::storage::save_config(&self.config).is_err() {}
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::App;

    #[test]
    fn request_tracking_latest_wins_per_key() {
        let mut app = App::new();
        let first = app.next_request_id("search");
        let second = app.next_request_id("search");

        assert!(!app.is_latest_request("search", first));
        assert!(app.is_latest_request("search", second));
    }

    #[test]
    fn request_tracking_isolated_by_key() {
        let mut app = App::new();
        let search_id = app.next_request_id("search");
        let home_id = app.next_request_id("home");

        assert!(app.is_latest_request("search", search_id));
        assert!(app.is_latest_request("home", home_id));
        assert!(!app.is_latest_request("home", search_id));
    }
}
