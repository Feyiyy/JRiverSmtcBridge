using System.Text.Json;
using JRiver.SmtcBridge;

Console.OutputEncoding = System.Text.Encoding.UTF8;
Console.WriteLine("==================================================");
Console.WriteLine("  JRiver MCWS -> Windows SMTC 状态同步桥接器");
Console.WriteLine("  适配 BetterLyrics 歌词显示与系统媒体控制");
Console.WriteLine("==================================================");

// 默认配置
string host = "127.0.0.1";
int port = 52199;
string? username = null;
string? password = null;
int pollIntervalMs = 500;
#if DEBUG
bool debug = true;
#else
bool debug = false;
#endif

// 命令行参数检查（支持 --debug / -d 或 --no-debug）
if (args.Contains("--debug", StringComparer.OrdinalIgnoreCase) || args.Contains("-d", StringComparer.OrdinalIgnoreCase))
{
    debug = true;
}
else if (args.Contains("--no-debug", StringComparer.OrdinalIgnoreCase))
{
    debug = false;
}

void LoadConfig(string filePath)
{
    if (!File.Exists(filePath)) return;
    try
    {
        var json = File.ReadAllText(filePath);
        using var doc = JsonDocument.Parse(json);
        if (doc.RootElement.TryGetProperty("JRiver", out var jriverConfig))
        {
            if (jriverConfig.TryGetProperty("Host", out var h) && !string.IsNullOrWhiteSpace(h.GetString())) host = h.GetString()!;
            if (jriverConfig.TryGetProperty("Port", out var p)) port = p.GetInt32();
            if (jriverConfig.TryGetProperty("Username", out var u)) username = u.GetString();
            if (jriverConfig.TryGetProperty("Password", out var pwd)) password = pwd.GetString();
            if (jriverConfig.TryGetProperty("PollIntervalMs", out var interval)) pollIntervalMs = interval.GetInt32();
            if (jriverConfig.TryGetProperty("Debug", out var dbg)) debug = dbg.GetBoolean();
        }
    }
    catch (Exception ex)
    {
        Console.WriteLine($"[Config] 读取配置文件 ({Path.GetFileName(filePath)}) 失败: {ex.Message}");
    }
}

// 优先加载 appsettings.json，随后允许 appsettings.local.json 覆盖（避免将私有密码提交到 GitHub）
string baseDir = AppDomain.CurrentDomain.BaseDirectory;
LoadConfig(Path.Combine(baseDir, "appsettings.json"));
LoadConfig(Path.Combine(baseDir, "appsettings.local.json"));

Console.WriteLine($"[Config] 连接目标: http://{host}:{port}");
Console.WriteLine($"[Config] 认证配置: {(string.IsNullOrEmpty(username) ? "无认证" : $"已配置用户: {username}")}");
Console.WriteLine($"[Config] 轮询间隔: {pollIntervalMs} ms");
Console.WriteLine($"[Config] 调试日志: {(debug ? "开启 (输出时间轴与详细状态)" : "关闭")}");
Console.WriteLine();

using var cts = new CancellationTokenSource();
Console.CancelKeyPress += (s, e) =>
{
    e.Cancel = true;
    Console.WriteLine("\n正在退出服务...");
    cts.Cancel();
};

using var mcwsClient = new JRiverMcwsClient(host, port, username, password);
using var smtcManager = new SmtcManager(mcwsClient, debug);

bool wasConnected = false;

try
{
    while (!cts.IsCancellationRequested)
    {
        var info = await mcwsClient.GetPlaybackInfoAsync(cts.Token);

        if (info == null)
        {
            if (wasConnected)
            {
                Console.WriteLine("[MCWS] 与 JRiver 断开连接，已清空 SMTC 会话状态。");
                smtcManager.SetDisconnected();
                wasConnected = false;
            }
        }
        else
        {
            if (!wasConnected)
            {
                Console.WriteLine("[MCWS] 成功连接至 JRiver Media Center！");
                wasConnected = true;
            }

            smtcManager.Update(info);
        }

        try
        {
            await Task.Delay(pollIntervalMs, cts.Token);
        }
        catch (OperationCanceledException)
        {
            break;
        }
    }
}
catch (OperationCanceledException)
{
    // 正常退出
}
finally
{
    smtcManager.SetDisconnected();
    Console.WriteLine("服务已停止。");
}
