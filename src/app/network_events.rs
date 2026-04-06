use crate::app::App;
use crate::application::network;
use crate::presentation::tui::{Page, VideoCard};

impl App {
    pub(super) fn drain_network_events(&mut self) {
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
}
