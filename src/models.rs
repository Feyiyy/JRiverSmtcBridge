#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PlaybackState {
    Stopped = 0,
    Paused = 1,
    Playing = 2,
    Waiting = 3,
}

impl PlaybackState {
    pub fn from_u32(val: u32) -> Self {
        match val {
            1 => PlaybackState::Paused,
            2 => PlaybackState::Playing,
            3 => PlaybackState::Waiting,
            _ => PlaybackState::Stopped,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackState::Stopped => "Stopped",
            PlaybackState::Paused => "Paused",
            PlaybackState::Playing => "Playing",
            PlaybackState::Waiting => "Waiting",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackInfo {
    pub state: PlaybackState,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_number: u32,
    pub album_track_count: u32,
    pub genre: String,
    pub file_key: String,
    pub position_ms: i64,
    pub duration_ms: i64,
}

impl Default for PlaybackInfo {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            status: "Stopped".to_string(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            track_number: 0,
            album_track_count: 0,
            genre: String::new(),
            file_key: String::new(),
            position_ms: 0,
            duration_ms: 0,
        }
    }
}

impl PlaybackInfo {
    #[allow(dead_code)]
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    #[allow(dead_code)]
    pub fn is_paused(&self) -> bool {
        self.state == PlaybackState::Paused
    }

    #[allow(dead_code)]
    pub fn is_stopped(&self) -> bool {
        self.state == PlaybackState::Stopped
    }

    pub fn is_same_track(&self, other: &PlaybackInfo) -> bool {
        if !self.file_key.is_empty() && !other.file_key.is_empty() {
            return self.file_key == other.file_key;
        }
        self.title == other.title && self.artist == other.artist && self.album == other.album
    }
}
