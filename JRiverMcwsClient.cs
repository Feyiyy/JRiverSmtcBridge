using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Xml.Linq;
using JRiver.SmtcBridge.Models;

namespace JRiver.SmtcBridge;

public class JRiverMcwsClient : IDisposable
{
    private HttpClient _httpClient;
    private string _baseUrl;

    public bool IsDebugEnabled { get; set; }
    public event Action? AuthFailed;

    public JRiverMcwsClient(string host = "127.0.0.1", int port = 52199, string? username = null, string? password = null)
    {
        _baseUrl = $"http://{host}:{port}";
        _httpClient = CreateHttpClient(_baseUrl, username, password);
    }

    public void UpdateConnection(string host, int port, string? username, string? password)
    {
        _baseUrl = $"http://{host}:{port}";
        var newClient = CreateHttpClient(_baseUrl, username, password);
        var oldClient = Interlocked.Exchange(ref _httpClient, newClient);
        try { oldClient.Dispose(); } catch { }
    }

    private static HttpClient CreateHttpClient(string baseUrl, string? username, string? password)
    {
        var handler = new HttpClientHandler();
        if (!string.IsNullOrWhiteSpace(username))
        {
            handler.Credentials = new NetworkCredential(username, password ?? string.Empty);
        }

        var client = new HttpClient(handler)
        {
            BaseAddress = new Uri(baseUrl),
            Timeout = TimeSpan.FromSeconds(5)
        };

        if (!string.IsNullOrWhiteSpace(username))
        {
            var authBytes = Encoding.UTF8.GetBytes($"{username}:{password ?? string.Empty}");
            client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Basic", Convert.ToBase64String(authBytes));
        }

        return client;
    }

    public string GetCoverArtUrl(string fileKey)
    {
        if (string.IsNullOrEmpty(fileKey)) return string.Empty;
        return $"{_baseUrl}/MCWS/v1/File/GetImage?File={fileKey}&Type=Thumbnail";
    }

    /// <summary>
    /// 获取当前曲目的图像（首选原图/大图，失败则获取缩略图）
    /// </summary>
    public async Task<byte[]?> GetTrackImageBytesAsync(string fileKey, CancellationToken ct = default)
    {
        if (string.IsNullOrEmpty(fileKey)) return null;

        // 1. 尝试获取完整大图
        try
        {
            var response = await _httpClient.GetAsync($"/MCWS/v1/File/GetImage?File={fileKey}&Type=Full", ct);
            if (response.IsSuccessStatusCode && response.Content.Headers.ContentType?.MediaType?.StartsWith("image/") == true)
            {
                var bytes = await response.Content.ReadAsByteArrayAsync(ct);
                if (bytes != null && bytes.Length > 0)
                {
                    return bytes;
                }
            }
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS Image] 获取高清图像失败 ({fileKey}): {ex.Message}");
        }

        // 2. 尝试获取缩略图
        try
        {
            var response = await _httpClient.GetAsync($"/MCWS/v1/File/GetImage?File={fileKey}&Type=Thumbnail", ct);
            if (response.IsSuccessStatusCode && response.Content.Headers.ContentType?.MediaType?.StartsWith("image/") == true)
            {
                var bytes = await response.Content.ReadAsByteArrayAsync(ct);
                if (bytes != null && bytes.Length > 0)
                {
                    return bytes;
                }
            }
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS Image] 获取缩略图图像失败 ({fileKey}): {ex.Message}");
        }

        return null;
    }

    /// <summary>
    /// 歌曲无独立图像时，通过同专辑曲目查询并提取专辑封面图
    /// </summary>
    public async Task<byte[]?> GetAlbumArtBytesAsync(string albumName, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(albumName)) return null;

        try
        {
            // 通过 MCWS 搜索相同专辑名称的歌曲以获得专辑封面
            string escaped = albumName.Replace("\"", "\\\"");
            string query = Uri.EscapeDataString($"[Album]=\"{escaped}\"");
            string url = $"/MCWS/v1/Files/Search?Query={query}&Fields=Key,Image%20File&Action=JSON";

            var jsonStr = await _httpClient.GetStringAsync(url, ct);
            using var doc = JsonDocument.Parse(jsonStr);

            if (doc.RootElement.ValueKind == JsonValueKind.Array)
            {
                foreach (var elem in doc.RootElement.EnumerateArray())
                {
                    if (elem.TryGetProperty("Image File", out var imgFile))
                    {
                        var imgFileStr = imgFile.GetString();
                        if (!string.IsNullOrWhiteSpace(imgFileStr) && !imgFileStr.Equals("NONE", StringComparison.OrdinalIgnoreCase))
                        {
                            if (elem.TryGetProperty("Key", out var keyProp))
                            {
                                string candidateKey = keyProp.ToString();
                                var bytes = await GetTrackImageBytesAsync(candidateKey, ct);
                                if (bytes != null && bytes.Length > 0)
                                {
                                    return bytes;
                                }
                            }
                        }
                    }
                }
            }
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS Image] 检索专辑封面失败 ({albumName}): {ex.Message}");
        }

        return null;
    }

    /// <summary>
    /// 获取指定文件的详细元数据（如专辑艺术家、音轨编号、流派等）
    /// </summary>
    public async Task<Dictionary<string, string>?> GetFileFieldsAsync(string fileKey, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(fileKey)) return null;

        try
        {
            var url = $"/MCWS/v1/File/GetInfo?File={fileKey}&Action=JSON";
            var jsonStr = await _httpClient.GetStringAsync(url, ct);
            using var doc = JsonDocument.Parse(jsonStr);

            var dict = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            if (doc.RootElement.ValueKind == JsonValueKind.Array)
            {
                foreach (var item in doc.RootElement.EnumerateArray())
                {
                    foreach (var prop in item.EnumerateObject())
                    {
                        dict[prop.Name] = prop.Value.ToString();
                    }
                }
            }
            else if (doc.RootElement.ValueKind == JsonValueKind.Object)
            {
                foreach (var prop in doc.RootElement.EnumerateObject())
                {
                    dict[prop.Name] = prop.Value.ToString();
                }
            }
            return dict;
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS] 获取文件元数据详情失败 ({fileKey}): {ex.Message}");
            return null;
        }
    }

    public async Task<PlaybackInfo?> GetPlaybackInfoAsync(CancellationToken cancellationToken = default)
    {
        try
        {
            var response = await _httpClient.GetStringAsync("/MCWS/v1/Playback/Info", cancellationToken);
            return ParsePlaybackInfoXml(response);
        }
        catch (HttpRequestException ex)
        {
            if (ex.StatusCode == HttpStatusCode.Unauthorized)
            {
                if (IsDebugEnabled) Console.WriteLine($"[MCWS] JRiver 身份验证失败 (401 Unauthorized)，请检查用户名和密码。");
                AuthFailed?.Invoke();
            }
            return null;
        }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS] 获取播放信息失败: {ex.Message}");
            return null;
        }
    }

    private PlaybackInfo ParsePlaybackInfoXml(string xml)
    {
        var info = new PlaybackInfo();
        if (string.IsNullOrWhiteSpace(xml)) return info;

        try
        {
            var doc = XDocument.Parse(xml);
            var items = doc.Descendants("Item")
                .Where(x => x.Attribute("Name") != null)
                .ToDictionary(x => x.Attribute("Name")!.Value, x => x.Value, StringComparer.OrdinalIgnoreCase);

            if (items.TryGetValue("State", out var stateStr) && int.TryParse(stateStr, out var stateVal))
            {
                info.State = (PlaybackState)stateVal;
            }

            if (items.TryGetValue("Status", out var status))
            {
                info.Status = status;
            }

            if (items.TryGetValue("Name", out var name)) info.Title = name;
            if (items.TryGetValue("Artist", out var artist)) info.Artist = artist;
            if (items.TryGetValue("Album", out var album)) info.Album = album;

            if (items.TryGetValue("Album Artist", out var albumArtist) || items.TryGetValue("AlbumArtist", out albumArtist))
            {
                info.AlbumArtist = albumArtist;
            }

            if ((items.TryGetValue("Track #", out var trackStr) || items.TryGetValue("Track", out trackStr) || items.TryGetValue("TrackNumber", out trackStr)) && uint.TryParse(trackStr, out var trackNum))
            {
                info.TrackNumber = trackNum;
            }
            else if (items.TryGetValue("PlayingNowPosition", out var queuePosStr) && uint.TryParse(queuePosStr, out var queuePos))
            {
                info.TrackNumber = queuePos + 1;
            }

            if ((items.TryGetValue("PlayingNowTracks", out var totalTracksStr) || items.TryGetValue("TotalTracks", out totalTracksStr)) && uint.TryParse(totalTracksStr, out var totalTracks))
            {
                info.AlbumTrackCount = totalTracks;
            }

            if (items.TryGetValue("Genre", out var genre))
            {
                info.Genre = genre;
            }

            if (items.TryGetValue("FileKey", out var fileKey)) info.FileKey = fileKey;

            if (items.TryGetValue("PositionMS", out var posStr) && long.TryParse(posStr, out var pos))
            {
                info.PositionMs = pos;
            }

            if (items.TryGetValue("DurationMS", out var durStr) && long.TryParse(durStr, out var dur))
            {
                info.DurationMs = dur;
            }
        }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS] 解析 XML 失败: {ex.Message}");
        }

        return info;
    }

    public async Task PlayPauseAsync() => await SendCommandAsync("/MCWS/v1/Playback/PlayPause");
    public async Task PlayAsync() => await SendCommandAsync("/MCWS/v1/Playback/Play");
    public async Task PauseAsync() => await SendCommandAsync("/MCWS/v1/Playback/Pause");
    public async Task StopAsync() => await SendCommandAsync("/MCWS/v1/Playback/Stop");
    public async Task NextAsync() => await SendCommandAsync("/MCWS/v1/Playback/Next");
    public async Task PreviousAsync() => await SendCommandAsync("/MCWS/v1/Playback/Previous");

    public async Task SeekAsync(long positionMs)
    {
        await SendCommandAsync($"/MCWS/v1/Playback/Position?Position={positionMs}");
    }

    private async Task SendCommandAsync(string endpoint)
    {
        try
        {
            await _httpClient.GetAsync(endpoint);
        }
        catch (Exception ex)
        {
            if (IsDebugEnabled) Console.WriteLine($"[MCWS] 发送控制命令失败 ({endpoint}): {ex.Message}");
        }
    }

    public void Dispose()
    {
        _httpClient.Dispose();
    }
}
