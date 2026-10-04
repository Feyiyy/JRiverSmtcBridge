namespace JRiver.SmtcBridge.Models;

public class AppConfig
{
    public string Host { get; set; } = "127.0.0.1";
    public int Port { get; set; } = 52199;
    public string? Username { get; set; }
    public string? Password { get; set; }
    public int PollIntervalMs { get; set; } = 500;
    public bool Debug { get; set; } = false;
    public bool ShowNotifications { get; set; } = true;
}
