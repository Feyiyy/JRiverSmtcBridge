use crate::console::log_warn;
use crate::models::{PlaybackInfo, PlaybackState};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Clone)]
struct ClientState {
    base_url: String,
    auth_header: Option<String>,
    agent: ureq::Agent,
}

pub struct JRiverMcwsClient {
    state: Arc<RwLock<ClientState>>,
    pub auth_failed_notified: AtomicBool,
}

impl JRiverMcwsClient {
    pub fn new(host: &str, port: u16, username: &str, password: &str) -> Self {
        let state = Self::build_state(host, port, username, password, Duration::from_secs(5));
        Self {
            state: Arc::new(RwLock::new(state)),
            auth_failed_notified: AtomicBool::new(false),
        }
    }

    fn build_state(host: &str, port: u16, username: &str, password: &str, timeout: Duration) -> ClientState {
        let base_url = format!("http://{}:{}", host, port);
        let auth_header = if !username.trim().is_empty() {
            let cred = format!("{}:{}", username.trim(), password);
            let b64 = BASE64_STANDARD.encode(cred.as_bytes());
            Some(format!("Basic {}", b64))
        } else {
            None
        };

        let config = ureq::config::Config::builder()
            .timeout_global(Some(timeout))
            .build();
        let agent = config.new_agent();

        ClientState {
            base_url,
            auth_header,
            agent,
        }
    }

    pub fn update_connection(&self, host: &str, port: u16, username: &str, password: &str) {
        let new_state = Self::build_state(host, port, username, password, Duration::from_secs(5));
        let mut w = self.state.write().unwrap();
        *w = new_state;
        self.auth_failed_notified.store(false, Ordering::Relaxed);
    }

    pub fn test_connection(host: &str, port: u16, username: &str, password: &str) -> (bool, String) {
        let state = Self::build_state(host, port, username, password, Duration::from_secs(3));
        let url = format!("{}/MCWS/v1/Alive", state.base_url);

        let mut req = state.agent.get(&url);
        if let Some(ref auth) = state.auth_header {
            req = req.header("Authorization", auth);
        }

        match req.call() {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                if status == 401 {
                    return (false, "身份验证失败 (401)：用户名或密码错误".to_string());
                }
                if !(200..300).contains(&status) {
                    return (false, format!("连接返回状态码: {}", status));
                }

                if let Ok(body) = resp.body_mut().read_to_string() {
                    if body.contains("Status=\"Failed\"") || body.contains("Status='Failed'") {
                        let err = Self::extract_error_message(&body).unwrap_or_else(|| "未知错误".to_string());
                        return (false, format!("服务返回失败: {}", err));
                    }
                }

                (true, "连接成功！JRiver MCWS 服务正常响应".to_string())
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    return (false, "身份验证失败 (401)：用户名或密码错误".to_string());
                }
                if err_str.contains("timed out") || err_str.contains("Timeout") {
                    return (false, "连接超时，请检查服务地址和端口是否正确且 JRiver 已启动".to_string());
                }
                (false, format!("无法连接到 JRiver: {}", err_str))
            }
        }
    }

    fn extract_error_message(xml: &str) -> Option<String> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();

        while let Ok(event) = reader.read_event_into(&mut buf) {
            match event {
                Event::Start(ref e) if e.name().as_ref() == b"Response" => {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"ErrorMessage" || attr.key.as_ref() == b"Error" {
                            if let Ok(v) = attr.decode_and_unescape_value(reader.decoder()) {
                                return Some(v.into_owned());
                            }
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
        None
    }

    pub fn get_playback_info(&self) -> Result<PlaybackInfo, String> {
        let (base_url, auth_header, agent) = {
            let r = self.state.read().unwrap();
            (r.base_url.clone(), r.auth_header.clone(), r.agent.clone())
        };

        let url = format!("{}/MCWS/v1/Playback/Info", base_url);
        let mut req = agent.get(&url);
        if let Some(ref auth) = auth_header {
            req = req.header("Authorization", auth);
        }

        let mut resp = match req.call() {
            Ok(r) => r,
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("401") {
                    return Err("401 Unauthorized".to_string());
                }
                return Err(err_str);
            }
        };

        let status = resp.status().as_u16();
        if status == 401 {
            return Err("401 Unauthorized".to_string());
        }
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {}", status));
        }

        let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Self::parse_playback_info_xml(&body)
    }

    fn parse_playback_info_xml(xml: &str) -> Result<PlaybackInfo, String> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();

        let mut items = HashMap::new();
        let mut current_item_name: Option<String> = None;

        while let Ok(event) = reader.read_event_into(&mut buf) {
            match event {
                Event::Start(ref e) => {
                    if e.name().as_ref() == b"Response" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"Status" {
                                if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                                    if val.eq_ignore_ascii_case("Failed") {
                                        return Err("MCWS response status failed".to_string());
                                    }
                                }
                            }
                        }
                    } else if e.name().as_ref() == b"Item" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"Name" {
                                if let Ok(val) = attr.decode_and_unescape_value(reader.decoder()) {
                                    current_item_name = Some(val.into_owned());
                                }
                            }
                        }
                    }
                }
                Event::Text(ref e) => {
                    if let Some(ref name) = current_item_name {
                        if let Ok(text) = e.unescape() {
                            items.insert(name.to_ascii_lowercase(), text.into_owned());
                        }
                    }
                }
                Event::End(ref e) => {
                    if e.name().as_ref() == b"Item" {
                        current_item_name = None;
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }

        let mut info = PlaybackInfo::default();

        if let Some(state_str) = items.get("state") {
            if let Ok(s) = state_str.parse::<u32>() {
                info.state = PlaybackState::from_u32(s);
            }
        }

        if let Some(status) = items.get("status") {
            info.status = status.clone();
        }

        if let Some(name) = items.get("name") {
            info.title = name.clone();
        }
        if let Some(artist) = items.get("artist") {
            info.artist = artist.clone();
        }
        if let Some(album) = items.get("album") {
            info.album = album.clone();
        }

        if let Some(album_artist) = items.get("album artist").or_else(|| items.get("albumartist")) {
            info.album_artist = album_artist.clone();
        }

        if let Some(track_str) = items.get("track #").or_else(|| items.get("track")).or_else(|| items.get("tracknumber")) {
            if let Ok(tr) = track_str.parse::<u32>() {
                info.track_number = tr;
            }
        } else if let Some(queue_pos) = items.get("playingnowposition") {
            if let Ok(pos) = queue_pos.parse::<u32>() {
                info.track_number = pos + 1;
            }
        }

        if let Some(total_tracks) = items.get("playingnowtracks").or_else(|| items.get("totaltracks")) {
            if let Ok(tot) = total_tracks.parse::<u32>() {
                info.album_track_count = tot;
            }
        }

        if let Some(genre) = items.get("genre") {
            info.genre = genre.clone();
        }

        if let Some(file_key) = items.get("filekey") {
            info.file_key = file_key.clone();
        }

        if let Some(pos_str) = items.get("positionms") {
            if let Ok(pos) = pos_str.parse::<i64>() {
                info.position_ms = pos;
            }
        }

        if let Some(dur_str) = items.get("durationms") {
            if let Ok(dur) = dur_str.parse::<i64>() {
                info.duration_ms = dur;
            }
        }

        Ok(info)
    }

    pub fn get_file_fields(&self, file_key: &str) -> Option<HashMap<String, String>> {
        if file_key.trim().is_empty() {
            return None;
        }

        let (base_url, auth_header, agent) = {
            let r = self.state.read().unwrap();
            (r.base_url.clone(), r.auth_header.clone(), r.agent.clone())
        };

        let url = format!("{}/MCWS/v1/File/GetInfo?File={}&Action=JSON", base_url, file_key);
        let mut req = agent.get(&url);
        if let Some(ref auth) = auth_header {
            req = req.header("Authorization", auth);
        }

        let mut resp = req.call().ok()?;
        let json_str = resp.body_mut().read_to_string().ok()?;
        let val: serde_json::Value = serde_json::from_str(&json_str).ok()?;

        let mut map = HashMap::new();
        if let Some(arr) = val.as_array() {
            for item in arr {
                if let Some(obj) = item.as_object() {
                    for (k, v) in obj {
                        let val_str = match v {
                            serde_json::Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        };
                        map.insert(k.to_ascii_lowercase(), val_str);
                    }
                }
            }
        } else if let Some(obj) = val.as_object() {
            for (k, v) in obj {
                let val_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    _ => v.to_string(),
                };
                map.insert(k.to_ascii_lowercase(), val_str);
            }
        }

        Some(map)
    }

    pub fn get_track_image(&self, file_key: &str) -> Option<Vec<u8>> {
        if file_key.trim().is_empty() {
            return None;
        }

        let (base_url, auth_header, agent) = {
            let r = self.state.read().unwrap();
            (r.base_url.clone(), r.auth_header.clone(), r.agent.clone())
        };

        // 1. 优先尝试 ?File={key}&Width=600&Height=600
        let url_600 = format!("{}/MCWS/v1/File/GetImage?File={}&Width=600&Height=600", base_url, file_key);
        if let Some(bytes) = Self::download_image(&agent, &url_600, auth_header.as_deref()) {
            return Some(bytes);
        }

        // 2. 尝试 Type=Thumbnail
        let url_thumb = format!("{}/MCWS/v1/File/GetImage?File={}&Type=Thumbnail", base_url, file_key);
        if let Some(bytes) = Self::download_image(&agent, &url_thumb, auth_header.as_deref()) {
            return Some(bytes);
        }

        // 3. 尝试 Type=Full
        let url_full = format!("{}/MCWS/v1/File/GetImage?File={}&Type=Full", base_url, file_key);
        if let Some(bytes) = Self::download_image(&agent, &url_full, auth_header.as_deref()) {
            return Some(bytes);
        }

        None
    }

    fn download_image(agent: &ureq::Agent, url: &str, auth_header: Option<&str>) -> Option<Vec<u8>> {
        let mut req = agent.get(url);
        if let Some(auth) = auth_header {
            req = req.header("Authorization", auth);
        }

        let mut resp = req.call().ok()?;
        let status = resp.status().as_u16();
        if status != 200 {
            return None;
        }

        if let Some(ct) = resp.headers().get("content-type") {
            if let Ok(ct_str) = ct.to_str() {
                if !ct_str.starts_with("image/") {
                    return None;
                }
            }
        }

        let bytes = resp.body_mut().read_to_vec().ok()?;
        if !bytes.is_empty() {
            Some(bytes)
        } else {
            None
        }
    }

    pub fn get_album_art(&self, album_name: &str) -> Option<Vec<u8>> {
        if album_name.trim().is_empty() {
            return None;
        }

        let (base_url, auth_header, agent) = {
            let r = self.state.read().unwrap();
            (r.base_url.clone(), r.auth_header.clone(), r.agent.clone())
        };

        let escaped = album_name.replace('\"', "\\\"");
        let raw_query = format!("[Album]=\"{}\"", escaped);
        let encoded_query = utf8_percent_encode(&raw_query, NON_ALPHANUMERIC).to_string();
        let url = format!("{}/MCWS/v1/Files/Search?Query={}&Fields=Key,Image%20File&Action=JSON", base_url, encoded_query);

        let mut req = agent.get(&url);
        if let Some(ref auth) = auth_header {
            req = req.header("Authorization", auth);
        }

        let mut resp = req.call().ok()?;
        let json_str = resp.body_mut().read_to_string().ok()?;
        let val: serde_json::Value = serde_json::from_str(&json_str).ok()?;

        if let Some(arr) = val.as_array() {
            for item in arr {
                if let Some(img_file) = item.get("Image File").and_then(|v| v.as_str()) {
                    if !img_file.trim().is_empty() && !img_file.eq_ignore_ascii_case("NONE") {
                        if let Some(key) = item.get("Key") {
                            let key_str = match key {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Number(n) => n.to_string(),
                                _ => continue,
                            };
                            if let Some(bytes) = self.get_track_image(&key_str) {
                                return Some(bytes);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    #[allow(dead_code)]
    pub fn play_pause(&self) {
        self.send_command("/MCWS/v1/Playback/PlayPause");
    }

    pub fn play(&self) {
        self.send_command("/MCWS/v1/Playback/Play");
    }

    pub fn pause(&self) {
        self.send_command("/MCWS/v1/Playback/Pause");
    }

    pub fn stop(&self) {
        self.send_command("/MCWS/v1/Playback/Stop");
    }

    pub fn next(&self) {
        self.send_command("/MCWS/v1/Playback/Next");
    }

    pub fn previous(&self) {
        self.send_command("/MCWS/v1/Playback/Previous");
    }

    pub fn seek(&self, position_ms: i64) {
        let endpoint = format!("/MCWS/v1/Playback/Position?Position={}", position_ms);
        self.send_command(&endpoint);
    }

    fn send_command(&self, endpoint: &str) {
        let (base_url, auth_header, agent) = {
            let r = self.state.read().unwrap();
            (r.base_url.clone(), r.auth_header.clone(), r.agent.clone())
        };

        let url = format!("{}{}", base_url, endpoint);
        let mut req = agent.get(&url);
        if let Some(ref auth) = auth_header {
            req = req.header("Authorization", auth);
        }

        if let Err(e) = req.call() {
            log_warn(&format!("发送控制命令失败 ({}): {}", endpoint, e));
        }
    }
}
