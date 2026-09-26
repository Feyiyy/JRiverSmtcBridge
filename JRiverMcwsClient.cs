using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Xml.Linq;
using JRiver.SmtcBridge.Models;

namespace JRiver.SmtcBridge;

public class JRiverMcwsClient : IDisposable
{
    private readonly HttpClient _httpClient;
    private readonly string _baseUrl;

    public JRiverMcwsClient(string host = "127.0.0.1", int port = 52199, string? username = null, string? password = null)
    {
        _baseUrl = $"http://{host}:{port}";

        var handler = new HttpClientHandler();
        if (!string.IsNullOrWhiteSpace(username))
        {
            handler.Credentials = new NetworkCredential(username, password ?? string.Empty);
        }

        _httpClient = new HttpClient(handler)
        {
            BaseAddress = new Uri(_baseUrl),
            Timeout = TimeSpan.FromSeconds(3)
        };

        // 如果配置了账号密码，直接添加 HTTP Basic Auth 头部，避免每次 401 挑战带来的请求延迟
        if (!string.IsNullOrWhiteSpace(username))
        {
            var authBytes = Encoding.UTF8.GetBytes($"{username}:{password ?? string.Empty}");
            _httpClient.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Basic", Convert.ToBase64String(authBytes));
        }
    }

    public string GetCoverArtUrl(string fileKey)
    {
        if (string.IsNullOrEmpty(fileKey)) return string.Empty;
        return $"{_baseUrl}/MCWS/v1/File/GetImage?File={fileKey}&Type=Thumbnail";
    }

    public async Task<PlaybackInfo?> GetPlaybackInfoAsync(CancellationToken cancellationToken = default)
    {
        try
        {
            var response = await _httpClient.GetStringAsync("/MCWS/v1/Playback/Info", cancellationToken);
            return ParsePlaybackInfoXml(response);
        }
        catch (HttpRequestException)
        {
            // JRiver 尚未启动或网络异常
            return null;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[MCWS] 获取播放信息失败: {ex.Message}");
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
            Console.WriteLine($"[MCWS] 解析 XML 失败: {ex.Message}");
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
            Console.WriteLine($"[MCWS] 发送控制命令失败 ({endpoint}): {ex.Message}");
        }
    }

    public void Dispose()
    {
        _httpClient.Dispose();
    }
}
