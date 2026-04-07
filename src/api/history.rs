//! Watch history API types
//!
//! API endpoint: GET https://api.bilibili.com/x/web-interface/history/cursor
//! Authentication: Cookie (SESSDATA)

use serde::Deserialize;

/// Response data for history cursor API
#[derive(Debug, Deserialize)]
pub struct HistoryData {
    pub cursor: HistoryCursor,
    pub tab: Option<Vec<HistoryTab>>,
    pub list: Vec<HistoryItem>,
}

/// Cursor for pagination
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryCursor {
    /// Max value for next page
    pub max: i64,
    /// View timestamp for next page
    pub view_at: i64,
    /// Business type
    pub business: String,
    /// Page size
    pub ps: i32,
}

/// Tab types in history
#[derive(Debug, Deserialize)]
pub struct HistoryTab {
    #[serde(rename = "type")]
    pub tab_type: String,
    pub name: String,
}

/// Individual history item
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryItem {
    /// Title of the content
    pub title: String,
    /// Long title (for episodes)
    pub long_title: Option<String>,
    /// Cover image URL
    pub cover: Option<String>,
    /// Alternative covers
    pub covers: Option<Vec<String>>,
    /// URI for navigation
    pub uri: Option<String>,
    /// History metadata
    pub history: HistoryMeta,
    /// Number of videos in the content
    pub videos: i32,
    /// Author name
    pub author_name: String,
    /// Author avatar
    pub author_face: Option<String>,
    /// Author mid
    pub author_mid: i64,
    /// Last view timestamp
    pub view_at: i64,
    /// Watch progress in seconds
    pub progress: i64,
    /// Badge text (e.g., "专栏", "直播中", "国创")
    pub badge: Option<String>,
    /// Show title for episodes
    pub show_title: Option<String>,
    /// Duration in seconds
    pub duration: i64,
    /// Current episode info
    pub current: Option<String>,
    /// Total episodes
    pub total: i32,
    /// New episode description
    pub new_desc: Option<String>,
    /// Whether the series is finished
    pub is_finish: i32,
    /// Whether favorited
    pub is_fav: i32,
    /// Kid for certain types
    pub kid: i64,
    /// Tag name
    pub tag_name: Option<String>,
    /// Live status (0: not live, 1: live)
    pub live_status: i32,
}

/// History metadata containing IDs
#[derive(Debug, Clone, Deserialize)]
pub struct HistoryMeta {
    /// Object ID
    pub oid: i64,
    /// Episode ID (for pgc)
    pub epid: i64,
    /// BV ID (for archive)
    pub bvid: Option<String>,
    /// Page number
    pub page: i32,
    /// CID
    pub cid: i64,
    /// Part name
    pub part: Option<String>,
    /// Business type: archive, pgc, live, article, article-list
    pub business: String,
    /// Device type
    pub dt: i32,
}

impl HistoryItem {
    /// Get the best cover URL
    pub fn get_cover(&self) -> Option<&str> {
        if let Some(ref cover) = self.cover {
            if !cover.is_empty() {
                return Some(cover.as_str());
            }
        }
        if let Some(ref covers) = self.covers {
            if let Some(first) = covers.first() {
                return Some(first.as_str());
            }
        }
        None
    }

    /// Format duration as mm:ss
    pub fn format_duration(&self) -> String {
        if self.duration > 0 {
            let minutes = self.duration / 60;
            let seconds = self.duration % 60;
            format!("{:02}:{:02}", minutes, seconds)
        } else {
            "--:--".to_string()
        }
    }

    /// Format progress as mm:ss
    pub fn format_progress(&self) -> String {
        if self.progress > 0 {
            let minutes = self.progress / 60;
            let seconds = self.progress % 60;
            format!("{:02}:{:02}", minutes, seconds)
        } else {
            "00:00".to_string()
        }
    }

    /// Calculate progress percentage
    pub fn progress_percent(&self) -> f64 {
        if self.duration > 0 {
            (self.progress as f64 / self.duration as f64 * 100.0).min(100.0)
        } else {
            0.0
        }
    }

    /// Format view_at timestamp as relative time
    pub fn format_view_time(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let diff = now - self.view_at;

        if diff < 60 {
            "刚刚".to_string()
        } else if diff < 3600 {
            format!("{}分钟前", diff / 60)
        } else if diff < 86400 {
            format!("{}小时前", diff / 3600)
        } else if diff < 604800 {
            format!("{}天前", diff / 86400)
        } else {
            // Format as date
            let secs = self.view_at;
            let days_since_epoch = secs / 86400;
            let year = 1970 + (days_since_epoch / 365);
            format!("{}年", year)
        }
    }

    /// Check if this is a video (archive or pgc)
    pub fn is_video(&self) -> bool {
        matches!(self.history.business.as_str(), "archive" | "pgc")
    }

    /// Check if this is a live room history entry
    pub fn is_live(&self) -> bool {
        self.history.business == "live"
    }

    /// Get bvid if available
    pub fn get_bvid(&self) -> Option<&str> {
        self.history.bvid.as_deref().filter(|s| !s.is_empty())
    }

    /// Get live room id for live history entries
    pub fn get_live_room_id(&self) -> Option<i64> {
        if self.history.oid > 0 {
            return Some(self.history.oid);
        }

        self.uri
            .as_deref()
            .and_then(Self::parse_live_room_id_from_uri)
    }

    fn parse_live_room_id_from_uri(uri: &str) -> Option<i64> {
        let trimmed = uri.trim();
        if trimmed.is_empty() {
            return None;
        }

        let no_scheme = trimmed
            .strip_prefix("https://")
            .or_else(|| trimmed.strip_prefix("http://"))
            .unwrap_or(trimmed);
        let host_path = no_scheme.strip_prefix("//").unwrap_or(no_scheme);

        let mut parts = host_path.splitn(2, '/');
        let host = parts.next()?.split(':').next()?.to_ascii_lowercase();
        if host != "live.bilibili.com" && host != "m.live.bilibili.com" {
            return None;
        }

        let path = parts.next().unwrap_or_default();
        let first_segment = path
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .split('/')
            .find(|seg| !seg.is_empty())?;

        first_segment.parse::<i64>().ok().filter(|id| *id > 0)
    }
}
