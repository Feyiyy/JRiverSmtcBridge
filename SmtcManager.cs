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

    public SmtcManager(JRiverMcwsClient mcwsClient)
    {
        _mcwsClient = mcwsClient;

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

            Console.WriteLine($"[SMTC] 同步曲目: {info.Title} - {info.Artist} (时长: {TimeSpan.FromMilliseconds(info.DurationMs):mm\\:ss})");
        }

        // 同步时间轴给 BetterLyrics 滚动定位
        if (info.DurationMs > 0)
        {
            var timeline = new SystemMediaTransportControlsTimelineProperties
            {
                StartTime = TimeSpan.Zero,
                MinSeekTime = TimeSpan.Zero,
                Position = TimeSpan.FromMilliseconds(info.PositionMs),
                MaxSeekTime = TimeSpan.FromMilliseconds(info.DurationMs),
                EndTime = TimeSpan.FromMilliseconds(info.DurationMs)
            };
            _smtc.UpdateTimelineProperties(timeline);
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
        }
    }

    public void Dispose()
    {
        _smtc.ButtonPressed -= OnSmtcButtonPressed;
        _mediaPlayer.Dispose();
    }
}
