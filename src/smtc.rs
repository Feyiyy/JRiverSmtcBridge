use crate::console::{log_info, log_timeline};
use crate::models::{PlaybackInfo, PlaybackState};
use std::sync::mpsc::Sender;
use windows::core::HSTRING;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{InMemoryRandomAccessStream, RandomAccessStreamReference};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;

#[derive(Debug, Clone, Copy)]
pub enum SmtcControlCommand {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek(i64),
}

pub struct SmtcManager {
    smtc: SystemMediaTransportControls,
    current_stream: Option<InMemoryRandomAccessStream>,
    last_track: Option<PlaybackInfo>,
    last_state: PlaybackState,
    last_position_ms: i64,
    last_duration_ms: i64,
}

impl SmtcManager {
    /// 优先使用 ISystemMediaTransportControlsInterop::GetForWindow(hwnd) 绑定顶层窗口。
    /// 严禁使用 HWND_MESSAGE（Message-only 窗口无桌面层级，会导致 SMTC 无法被系统桌面/任务栏/快捷键感知）。
    ///
    /// 备选方案说明 (Alternative MediaPlayer approach):
    /// 若某些老旧非顶层窗口场景无法使用 GetForWindow，可改用 Windows.Media.Playback.MediaPlayer：
    /// ```rust
    /// // let player = windows::Media::Playback::MediaPlayer::new()?;
    /// // player.CommandManager()?.SetIsEnabled(false)?;
    /// // let smtc = player.SystemMediaTransportControls()?;
    /// ```
    pub fn new(hwnd: HWND, cmd_tx: Sender<SmtcControlCommand>) -> windows::core::Result<Self> {
        let interop: ISystemMediaTransportControlsInterop =
            windows::core::factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };

        // 默认开启全部媒体控制功能
        smtc.SetIsEnabled(true)?;
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsNextEnabled(true)?;
        smtc.SetIsPreviousEnabled(true)?;
        smtc.SetIsStopEnabled(true)?;

        // 注册按键回调（仅通过 mpsc 投递，严禁阻塞）
        let tx_btn = cmd_tx.clone();
        smtc.ButtonPressed(&TypedEventHandler::new(
            move |_sender, args: windows::core::Ref<'_, SystemMediaTransportControlsButtonPressedEventArgs>| {
                if let Ok(args) = args.ok() {
                    if let Ok(btn) = args.Button() {
                        let cmd = match btn {
                            SystemMediaTransportControlsButton::Play => SmtcControlCommand::Play,
                            SystemMediaTransportControlsButton::Pause => SmtcControlCommand::Pause,
                            SystemMediaTransportControlsButton::Stop => SmtcControlCommand::Stop,
                            SystemMediaTransportControlsButton::Next => SmtcControlCommand::Next,
                            SystemMediaTransportControlsButton::Previous => SmtcControlCommand::Previous,
                            _ => return Ok(()),
                        };
                        let _ = tx_btn.send(cmd);
                    }
                }
                Ok(())
            },
        ))?;

        // 注册进度跳转回调
        let tx_seek = cmd_tx;
        smtc.PlaybackPositionChangeRequested(&TypedEventHandler::new(
            move |_sender, args: windows::core::Ref<'_, PlaybackPositionChangeRequestedEventArgs>| {
                if let Ok(args) = args.ok() {
                    if let Ok(pos) = args.RequestedPlaybackPosition() {
                        // TimeSpan.Duration 单位是 100 纳秒，转换为毫秒
                        let ms = pos.Duration / 10_000;
                        let _ = tx_seek.send(SmtcControlCommand::Seek(ms));
                    }
                }
                Ok(())
            },
        ))?;

        Ok(Self {
            smtc,
            current_stream: None,
            last_track: None,
            last_state: PlaybackState::Stopped,
            last_position_ms: -1,
            last_duration_ms: -1,
        })
    }

    pub fn last_track(&self) -> Option<&PlaybackInfo> {
        self.last_track.as_ref()
    }

    pub fn last_state(&self) -> PlaybackState {
        self.last_state
    }

    pub fn update(&mut self, info: &PlaybackInfo) -> (bool, bool) {
        let mut state_changed = false;

        // Waiting(3) 映射为 MediaPlaybackStatus::Changing（不要清屏）
        let target_status = match info.state {
            PlaybackState::Playing => MediaPlaybackStatus::Playing,
            PlaybackState::Paused => MediaPlaybackStatus::Paused,
            PlaybackState::Stopped => MediaPlaybackStatus::Stopped,
            PlaybackState::Waiting => MediaPlaybackStatus::Changing,
        };

        if let Ok(current_status) = self.smtc.PlaybackStatus() {
            if current_status != target_status {
                let _ = self.smtc.SetPlaybackStatus(target_status);
            }
        }

        if target_status == MediaPlaybackStatus::Stopped || target_status == MediaPlaybackStatus::Closed {
            if self.last_state != PlaybackState::Stopped {
                self.current_stream = None;
                if let Ok(updater) = self.smtc.DisplayUpdater() {
                    let _ = updater.ClearAll();
                    let _ = updater.Update();
                }
                self.last_track = None;
                self.last_state = PlaybackState::Stopped;
                self.last_position_ms = -1;
                self.last_duration_ms = -1;
                state_changed = true;
            }
            return (false, state_changed);
        }

        // 保持默认开启控制按钮
        let _ = self.smtc.SetIsEnabled(true);
        let _ = self.smtc.SetIsPlayEnabled(true);
        let _ = self.smtc.SetIsPauseEnabled(true);
        let _ = self.smtc.SetIsNextEnabled(true);
        let _ = self.smtc.SetIsPreviousEnabled(true);
        let _ = self.smtc.SetIsStopEnabled(true);

        // 判断歌曲是否切换
        let track_changed = match &self.last_track {
            None => true,
            Some(last) => !info.is_same_track(last),
        };

        if track_changed {
            if let Ok(updater) = self.smtc.DisplayUpdater() {
                let _ = updater.SetType(MediaPlaybackType::Music);
                if let Ok(music_props) = updater.MusicProperties() {
                    let _ = music_props.SetTitle(&HSTRING::from(&info.title));
                    let _ = music_props.SetArtist(&HSTRING::from(&info.artist));
                    let _ = music_props.SetAlbumTitle(&HSTRING::from(&info.album));

                    let album_artist = if !info.album_artist.is_empty() {
                        &info.album_artist
                    } else {
                        &info.artist
                    };
                    let _ = music_props.SetAlbumArtist(&HSTRING::from(album_artist));

                    if info.track_number > 0 {
                        let _ = music_props.SetTrackNumber(info.track_number);
                    }
                    if info.album_track_count > 0 {
                        let _ = music_props.SetAlbumTrackCount(info.album_track_count);
                    }

                    if let Ok(genres) = music_props.Genres() {
                        let _ = genres.Clear();
                        if !info.genre.is_empty() {
                            let _ = genres.Append(&HSTRING::from(&info.genre));
                        }
                    }

                    let _ = updater.Update();
                }
            }

            self.last_track = Some(info.clone());
            self.last_position_ms = -1; // 曲目切换，重置时间轴以确保立即同步

            log_info(&format!(
                "[SMTC] 同步完整曲目信息: {} - {} (专辑: {}, 音轨: {}, 时长: {} ms)",
                info.title, info.artist, info.album, info.track_number, info.duration_ms
            ));
        }

        // 同步时间轴给 BetterLyrics 滚动定位
        if info.duration_ms > 0 {
            let s_changed = self.last_state != info.state;
            let pos_changed = info.position_ms != self.last_position_ms;
            let dur_changed = info.duration_ms != self.last_duration_ms;

            if track_changed || s_changed || pos_changed || dur_changed {
                if let Ok(timeline) = SystemMediaTransportControlsTimelineProperties::new() {
                    let zero = TimeSpan { Duration: 0 };
                    let pos_time = TimeSpan {
                        Duration: info.position_ms * 10_000,
                    };
                    let dur_time = TimeSpan {
                        Duration: info.duration_ms * 10_000,
                    };

                    let _ = timeline.SetStartTime(zero);
                    let _ = timeline.SetMinSeekTime(zero);
                    let _ = timeline.SetPosition(pos_time);
                    let _ = timeline.SetMaxSeekTime(dur_time);
                    let _ = timeline.SetEndTime(dur_time);

                    let _ = self.smtc.UpdateTimelineProperties(&timeline);

                    log_timeline(&format!(
                        "发送时间轴: {} ms / {} ms (状态: {:?})",
                        info.position_ms, info.duration_ms, info.state
                    ));

                    self.last_position_ms = info.position_ms;
                    self.last_duration_ms = info.duration_ms;
                }
            }
        }

        if self.last_state != info.state {
            self.last_state = info.state;
            state_changed = true;
        }

        (track_changed, state_changed)
    }

    pub fn set_cover_stream(&mut self, stream: InMemoryRandomAccessStream) {
        if let Ok(stream_ref) = RandomAccessStreamReference::CreateFromStream(&stream) {
            if let Ok(updater) = self.smtc.DisplayUpdater() {
                let _ = updater.SetThumbnail(&stream_ref);
                let _ = updater.Update();
            }
        }
        // 持久持有 stream，防止缩略图流被提前 Drop 导致系统无法读取
        self.current_stream = Some(stream);
    }

    pub fn set_disconnected(&mut self) {
        self.current_stream = None;
        let _ = self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed);
        if let Ok(updater) = self.smtc.DisplayUpdater() {
            let _ = updater.ClearAll();
            let _ = updater.Update();
        }
        self.last_track = None;
        self.last_state = PlaybackState::Stopped;
        self.last_position_ms = -1;
        self.last_duration_ms = -1;
    }
}
