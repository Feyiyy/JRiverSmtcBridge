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

    private CancellationTokenSource? _coverCts;
    private IRandomAccessStream? _currentCoverStream;
    private readonly Dictionary<string, byte[]> _coverMemoryCache = new();

    public bool IsDebugEnabled { get; set; }

    public PlaybackInfo? CurrentTrack => _lastTrack;
    public PlaybackState CurrentState => _lastState;

    public event Action<PlaybackInfo>? TrackChanged;
    public event Action<PlaybackState>? StateChanged;

    public SmtcManager(JRiverMcwsClient mcwsClient, bool debug = false)
    {
        _mcwsClient = mcwsClient;
        IsDebugEnabled = debug;

        _mediaPlayer = new MediaPlayer();
        _mediaPlayer.CommandManager.IsEnabled = false; // 禁用默认控制，完全由本程序代理

        _smtc = _mediaPlayer.SystemMediaTransportControls;

        // 默认开启全部媒体控制功能
        EnableDefaultControls();

        _smtc.ButtonPressed += OnSmtcButtonPressed;
        _smtc.PlaybackPositionChangeRequested += OnPlaybackPositionChangeRequested;
    }

    private void EnableDefaultControls()
    {
        _smtc.IsEnabled = true;
        _smtc.IsPlayEnabled = true;
        _smtc.IsPauseEnabled = true;
        _smtc.IsNextEnabled = true;
        _smtc.IsPreviousEnabled = true;
        _smtc.IsStopEnabled = true;
    }

    private async void OnSmtcButtonPressed(SystemMediaTransportControls sender, SystemMediaTransportControlsButtonPressedEventArgs args)
    {
        try
        {
            if (IsDebugEnabled)
            {
                Console.WriteLine($"[SMTC Button] 收到按键事件: {args.Button}");
            }

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
            if (IsDebugEnabled) Console.WriteLine($"[SMTC Button] 执行控制命令异常: {ex.Message}");
        }
    }

    private async void OnPlaybackPositionChangeRequested(SystemMediaTransportControls sender, PlaybackPositionChangeRequestedEventArgs args)
    {
        try
        {
            long targetMs = (long)args.RequestedPlaybackPosition.TotalMilliseconds;
            if (IsDebugEnabled)
            {
                Console.WriteLine($"[SMTC Control] 响应进度拖动跳转: {args.RequestedPlaybackPosition:hh\\:mm\\:ss\\.fff} ({targetMs} ms)");
            }
            await _mcwsClient.SeekAsync(targetMs);
        }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[SMTC Control] 进度跳转失败: {ex.Message}");
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
                CancelPendingCoverLoad();
                _currentCoverStream?.Dispose();
                _currentCoverStream = null;

                _smtc.DisplayUpdater.ClearAll();
                _smtc.DisplayUpdater.Update();
                _lastTrack = null;
                _lastState = PlaybackState.Stopped;
                _lastPositionMs = -1;
                _lastDurationMs = -1;

                StateChanged?.Invoke(PlaybackState.Stopped);
            }
            return;
        }

        // 保持默认开启控制功能
        EnableDefaultControls();

        // 判断歌曲是否切换
        bool trackChanged = _lastTrack == null || !info.IsSameTrack(_lastTrack);
        if (trackChanged)
        {
            _smtc.DisplayUpdater.Type = MediaPlaybackType.Music;
            var musicProps = _smtc.DisplayUpdater.MusicProperties;

            // 完整输出元数据给 SMTC
            musicProps.Title = info.Title;
            musicProps.Artist = info.Artist;
            musicProps.AlbumTitle = info.Album;

            if (!string.IsNullOrWhiteSpace(info.AlbumArtist))
            {
                musicProps.AlbumArtist = info.AlbumArtist;
            }
            else if (!string.IsNullOrWhiteSpace(info.Artist))
            {
                musicProps.AlbumArtist = info.Artist;
            }

            if (info.TrackNumber > 0)
            {
                musicProps.TrackNumber = info.TrackNumber;
            }

            if (info.AlbumTrackCount > 0)
            {
                musicProps.AlbumTrackCount = info.AlbumTrackCount;
            }

            musicProps.Genres.Clear();
            if (!string.IsNullOrWhiteSpace(info.Genre))
            {
                musicProps.Genres.Add(info.Genre);
            }

            _smtc.DisplayUpdater.Update();
            _lastTrack = info;
            _lastPositionMs = -1; // 曲目切换，重置时间轴以确保立即同步

            if (IsDebugEnabled)
            {
                Console.WriteLine($"[SMTC] 同步完整曲目信息: {info.Title} - {info.Artist} (专辑: {info.Album}, 音轨: {info.TrackNumber}, 时长: {TimeSpan.FromMilliseconds(info.DurationMs):mm\\:ss})");
            }

            TrackChanged?.Invoke(info);

            // 异步加载封面并补全扩展元数据
            CancelPendingCoverLoad();
            _coverCts = new CancellationTokenSource();
            _ = UpdateCoverArtAndMetadataAsync(info, _coverCts.Token);
        }

        // 同步时间轴给 BetterLyrics 滚动定位
        if (info.DurationMs > 0)
        {
            bool stateChanged = _lastState != info.State;
            bool positionChanged = info.PositionMs != _lastPositionMs;
            bool durationChanged = info.DurationMs != _lastDurationMs;

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
        }

        if (_lastState != info.State)
        {
            _lastState = info.State;
            StateChanged?.Invoke(info.State);
        }
    }

    private void CancelPendingCoverLoad()
    {
        if (_coverCts != null)
        {
            try
            {
                _coverCts.Cancel();
                _coverCts.Dispose();
            }
            catch { }
            _coverCts = null;
        }
    }

    private async Task UpdateCoverArtAndMetadataAsync(PlaybackInfo info, CancellationToken ct)
    {
        // 1. 若部分关键元数据缺失，尝试异步查询文件详细信息补充给 SMTC
        if (!string.IsNullOrEmpty(info.FileKey) && (info.TrackNumber == 0 || string.IsNullOrEmpty(info.AlbumArtist) || string.IsNullOrEmpty(info.Genre)))
        {
            try
            {
                var fields = await _mcwsClient.GetFileFieldsAsync(info.FileKey, ct);
                if (fields != null && !ct.IsCancellationRequested)
                {
                    bool enriched = false;
                    if (string.IsNullOrEmpty(info.AlbumArtist) && fields.TryGetValue("Album Artist", out var aa) && !string.IsNullOrWhiteSpace(aa))
                    {
                        info.AlbumArtist = aa;
                        enriched = true;
                    }
                    if (info.TrackNumber == 0 && (fields.TryGetValue("Track #", out var tr) || fields.TryGetValue("Track", out tr)) && uint.TryParse(tr, out var trNum))
                    {
                        info.TrackNumber = trNum;
                        enriched = true;
                    }
                    if (string.IsNullOrEmpty(info.Genre) && fields.TryGetValue("Genre", out var gn) && !string.IsNullOrWhiteSpace(gn))
                    {
                        info.Genre = gn;
                        enriched = true;
                    }
                    if (info.AlbumTrackCount == 0 && (fields.TryGetValue("Total Tracks", out var tt) || fields.TryGetValue("Tracks", out tt)) && uint.TryParse(tt, out var ttNum))
                    {
                        info.AlbumTrackCount = ttNum;
                        enriched = true;
                    }

                    if (enriched && !ct.IsCancellationRequested)
                    {
                        var musicProps = _smtc.DisplayUpdater.MusicProperties;
                        if (!string.IsNullOrEmpty(info.AlbumArtist)) musicProps.AlbumArtist = info.AlbumArtist;
                        if (info.TrackNumber > 0) musicProps.TrackNumber = info.TrackNumber;
                        if (info.AlbumTrackCount > 0) musicProps.AlbumTrackCount = info.AlbumTrackCount;
                        if (!string.IsNullOrEmpty(info.Genre))
                        {
                            musicProps.Genres.Clear();
                            musicProps.Genres.Add(info.Genre);
                        }
                        _smtc.DisplayUpdater.Update();
                    }
                }
            }
            catch (OperationCanceledException) { }
            catch (Exception ex)
            {
                if (IsDebugEnabled) Console.WriteLine($"[SMTC Metadata] 补全元数据异常: {ex.Message}");
            }
        }

        // 2. 加载封面（歌曲图 -> 专辑图 -> 默认精美背景图）
        try
        {
            byte[]? imageBytes = null;
            string cacheKey = !string.IsNullOrEmpty(info.FileKey) ? info.FileKey : $"{info.Artist}_{info.Album}_{info.Title}";

            // 检查内存缓存
            lock (_coverMemoryCache)
            {
                if (_coverMemoryCache.TryGetValue(cacheKey, out var cached))
                {
                    imageBytes = cached;
                }
            }

            // 第一优先级：获取歌曲专属图像
            if (imageBytes == null && !string.IsNullOrEmpty(info.FileKey))
            {
                imageBytes = await _mcwsClient.GetTrackImageBytesAsync(info.FileKey, ct);
                if (imageBytes != null && IsDebugEnabled)
                {
                    Console.WriteLine($"[SMTC Cover] 成功获取歌曲图像: {info.Title} ({imageBytes.Length / 1024.0:F1} KB)");
                }
            }

            // 第二优先级：歌曲无独立封面时，获取专辑图
            if (imageBytes == null && !string.IsNullOrEmpty(info.Album))
            {
                imageBytes = await _mcwsClient.GetAlbumArtBytesAsync(info.Album, ct);
                if (imageBytes != null && IsDebugEnabled)
                {
                    Console.WriteLine($"[SMTC Cover] 歌曲无专属封面，已使用专辑图像: {info.Album} ({imageBytes.Length / 1024.0:F1} KB)");
                }
            }

            // 第三优先级：若均无封面，使用好看的默认背景图
            if (imageBytes == null || imageBytes.Length == 0)
            {
                imageBytes = DefaultAssets.GetDefaultCoverBytes();
                if (IsDebugEnabled)
                {
                    Console.WriteLine("[SMTC Cover] 歌曲与专辑均无封面，已启用精美默认背景封面。");
                }
            }

            if (ct.IsCancellationRequested || imageBytes == null || imageBytes.Length == 0)
            {
                return;
            }

            // 加入内存缓存
            lock (_coverMemoryCache)
            {
                if (_coverMemoryCache.Count > 100) _coverMemoryCache.Clear();
                _coverMemoryCache[cacheKey] = imageBytes;
            }

            // 转换为 WinRT 随机访问流设置给 SMTC DisplayUpdater
            var stream = new InMemoryRandomAccessStream();
            using (var writer = new DataWriter(stream.GetOutputStreamAt(0)))
            {
                writer.WriteBytes(imageBytes);
                await writer.StoreAsync().AsTask(ct);
                await writer.FlushAsync().AsTask(ct);
                writer.DetachStream();
            }
            stream.Seek(0);

            if (ct.IsCancellationRequested)
            {
                stream.Dispose();
                return;
            }

            _currentCoverStream?.Dispose();
            _currentCoverStream = stream;

            _smtc.DisplayUpdater.Thumbnail = RandomAccessStreamReference.CreateFromStream(_currentCoverStream);
            _smtc.DisplayUpdater.Update();

            // 额外在本地临时目录写入一份，方便调试及其他程序查看
            try
            {
                string cacheDir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "JRiverSmtcBridge");
                Directory.CreateDirectory(cacheDir);
                await File.WriteAllBytesAsync(Path.Combine(cacheDir, "current_cover.jpg"), imageBytes, ct);
            }
            catch { }
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            if (IsDebugEnabled)
            {
                Console.WriteLine($"[SMTC Cover] 更新封面异常: {ex.Message}");
            }
        }
    }

    public void SetDisconnected()
    {
        CancelPendingCoverLoad();
        _currentCoverStream?.Dispose();
        _currentCoverStream = null;

        if (_smtc.PlaybackStatus != MediaPlaybackStatus.Closed)
        {
            _smtc.PlaybackStatus = MediaPlaybackStatus.Closed;
            _smtc.DisplayUpdater.ClearAll();
            _smtc.DisplayUpdater.Update();
            _lastTrack = null;
            _lastState = PlaybackState.Stopped;
            _lastPositionMs = -1;
            _lastDurationMs = -1;

            StateChanged?.Invoke(PlaybackState.Stopped);
        }
    }

    public void Dispose()
    {
        CancelPendingCoverLoad();
        _currentCoverStream?.Dispose();
        _smtc.ButtonPressed -= OnSmtcButtonPressed;
        _smtc.PlaybackPositionChangeRequested -= OnPlaybackPositionChangeRequested;
        _mediaPlayer.Dispose();
    }
}
