using Windows.Media;
using Windows.Media.Playback;
using Windows.Storage.Streams;
using JRiver.SmtcBridge.Models;

namespace JRiver.SmtcBridge;

public class SmtcManager : IDisposable
{
    private readonly MediaPlayer _mediaPlayer;
    private readonly SystemMediaTransportControls _smtc;
    private readonly JRiverMcwsClient _mcwsClient;
    private PlaybackInfo? _lastTrack;
    private PlaybackState _lastState = PlaybackState.Stopped;
    private long _lastPositionMs = -1;
    private long _lastDurationMs = -1;

    public bool IsDebugEnabled { get; set; }

    public SmtcManager(JRiverMcwsClient mcwsClient, bool debug = false)
    {
        _mcwsClient = mcwsClient;
        IsDebugEnabled = debug;

        _mediaPlayer = new MediaPlayer();
        _mediaPlayer.CommandManager.IsEnabled = false; // 禁用默认控制，完全由本程序代理

        _smtc = _mediaPlayer.SystemMediaTransportControls;
        _smtc.IsEnabled = true;
        _smtc.IsPlayEnabled = true;
        _smtc.IsPauseEnabled = true;
        _smtc.IsNextEnabled = true;
        _smtc.IsPreviousEnabled = true;
        _smtc.IsStopEnabled = true;

        _smtc.ButtonPressed += OnSmtcButtonPressed;
    }

    private async void OnSmtcButtonPressed(SystemMediaTransportControls sender, SystemMediaTransportControlsButtonPressedEventArgs args)
    {
        try
        {
            switch (args.Button)
            {
                case SystemMediaTransportControlsButton.Play:
                    await _mcwsClient.PlayAsync();
                    break;
                case SystemMediaTransportControlsButton.Pause:
                    await _mcwsClient.PauseAsync();
                    break;
                case SystemMediaTransportControlsButton.Stop:
                    await _mcwsClient.StopAsync();
                    break;
                case SystemMediaTransportControlsButton.Next:
                    await _mcwsClient.NextAsync();
                    break;
                case SystemMediaTransportControlsButton.Previous:
                    await _mcwsClient.PreviousAsync();
                    break;
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[SMTC Button] 执行控制命令异常: {ex.Message}");
        }
    }

    public void Update(PlaybackInfo info)
    {
        var targetStatus = info.State switch
        {
            PlaybackState.Playing => MediaPlaybackStatus.Playing,
            PlaybackState.Paused => MediaPlaybackStatus.Paused,
            PlaybackState.Stopped => MediaPlaybackStatus.Stopped,
            _ => MediaPlaybackStatus.Closed
        };

        if (_smtc.PlaybackStatus != targetStatus)
        {
            _smtc.PlaybackStatus = targetStatus;
        }

        if (targetStatus == MediaPlaybackStatus.Stopped || targetStatus == MediaPlaybackStatus.Closed)
        {
            if (_lastState != PlaybackState.Stopped)
            {
                _smtc.DisplayUpdater.ClearAll();
                _smtc.DisplayUpdater.Update();
                _lastTrack = null;
                _lastState = PlaybackState.Stopped;
                _lastPositionMs = -1;
                _lastDurationMs = -1;
            }
            return;
        }

        // 判断歌曲是否切换
        bool trackChanged = _lastTrack == null || !info.IsSameTrack(_lastTrack);
        if (trackChanged)
        {
            _smtc.DisplayUpdater.Type = MediaPlaybackType.Music;
            var musicProps = _smtc.DisplayUpdater.MusicProperties;
            musicProps.Title = info.Title;
            musicProps.Artist = info.Artist;
            musicProps.AlbumTitle = info.Album;

            if (!string.IsNullOrEmpty(info.FileKey))
            {
                var coverUrl = _mcwsClient.GetCoverArtUrl(info.FileKey);
                try
                {
                    _smtc.DisplayUpdater.Thumbnail = RandomAccessStreamReference.CreateFromUri(new Uri(coverUrl));
                }
                catch
                {
                    _smtc.DisplayUpdater.Thumbnail = null;
                }
            }
            else
            {
                _smtc.DisplayUpdater.Thumbnail = null;
            }

            _smtc.DisplayUpdater.Update();
            _lastTrack = info;
            _lastPositionMs = -1; // 曲目切换，重置时间轴以确保立即同步

            Console.WriteLine($"[SMTC] 同步曲目: {info.Title} - {info.Artist} (时长: {TimeSpan.FromMilliseconds(info.DurationMs):mm\\:ss})");
        }

        // 同步时间轴给 BetterLyrics 滚动定位
        if (info.DurationMs > 0)
        {
            bool stateChanged = _lastState != info.State;
            bool positionChanged = info.PositionMs != _lastPositionMs;
            bool durationChanged = info.DurationMs != _lastDurationMs;

            // 仅在曲目变化、播放状态改变（如播放/暂停）、时长变化或播放位置实际推进时更新时间轴
            // 避免因 MCWS 轮询间隔小于播放器时间刷新分辨率而重复发送相同的 Position，导致下游歌词软件时间轴回退抖动
            if (trackChanged || stateChanged || positionChanged || durationChanged)
            {
                var posTime = TimeSpan.FromMilliseconds(info.PositionMs);
                var durTime = TimeSpan.FromMilliseconds(info.DurationMs);

                var timeline = new SystemMediaTransportControlsTimelineProperties
                {
                    StartTime = TimeSpan.Zero,
                    MinSeekTime = TimeSpan.Zero,
                    Position = posTime,
                    MaxSeekTime = durTime,
                    EndTime = durTime
                };
                _smtc.UpdateTimelineProperties(timeline);

                if (IsDebugEnabled)
                {
                    Console.WriteLine($"[Timeline] [{DateTime.Now:HH:mm:ss.fff}] 发送时间轴: {posTime:hh\\:mm\\:ss\\.fffffff}/{durTime:hh\\:mm\\:ss\\.fffffff} (状态: {info.State})");
                }

                _lastPositionMs = info.PositionMs;
                _lastDurationMs = info.DurationMs;
            }
            else if (IsDebugEnabled)
            {
                var posTime = TimeSpan.FromMilliseconds(info.PositionMs);
                Console.WriteLine($"[Timeline] [{DateTime.Now:HH:mm:ss.fff}] 跳过重复时间轴: {posTime:hh\\:mm\\:ss\\.fffffff} (位置未改变)");
            }
        }

        _lastState = info.State;
    }

    public void SetDisconnected()
    {
        if (_smtc.PlaybackStatus != MediaPlaybackStatus.Closed)
        {
            _smtc.PlaybackStatus = MediaPlaybackStatus.Closed;
            _smtc.DisplayUpdater.ClearAll();
            _smtc.DisplayUpdater.Update();
            _lastTrack = null;
            _lastState = PlaybackState.Stopped;
            _lastPositionMs = -1;
            _lastDurationMs = -1;
        }
    }

    public void Dispose()
    {
        _smtc.ButtonPressed -= OnSmtcButtonPressed;
        _mediaPlayer.Dispose();
    }
}
