mod actions;
mod network_events;
mod runtime;

use crate::application::network;
use crate::infrastructure::{
    bilibili::ApiClient,
    persistence::{self, AppConfig, Credentials, Keybindings},
};
use crate::presentation::tui::{HomePage, LoginPage, Page, Sidebar, Theme, ThemeVariant};
use std::collections::HashMap;
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
        let credentials = persistence::load_credentials().ok();
        let api_client = if let Some(ref creds) = credentials {
            ApiClient::with_cookies(creds)
        } else {
            ApiClient::new()
        };
        let api_client = Arc::new(api_client);
        let bridge = network::start_network_worker(api_client.clone());

        // Load config and apply saved theme
        let config = persistence::load_config().unwrap_or_default();
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
