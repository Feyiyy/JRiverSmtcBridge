using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;
using JRiver.SmtcBridge.Models;

namespace JRiver.SmtcBridge;

internal static class Program
{
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AllocConsole();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool FreeConsole();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool AttachConsole(int dwProcessId);

    [DllImport("kernel32.dll")]
    private static extern IntPtr GetConsoleWindow();

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    private static extern IntPtr GetSystemMenu(IntPtr hWnd, bool bRevert);

    [DllImport("user32.dll")]
    private static extern bool DeleteMenu(IntPtr hMenu, uint uPosition, uint uFlags);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetConsoleOutputCP(uint wCodePageID);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetConsoleCP(uint wCodePageID);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetStdHandle(int nStdHandle, IntPtr hHandle);

    [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(
        string lpFileName,
        uint dwDesiredAccess,
        uint dwShareMode,
        IntPtr lpSecurityAttributes,
        uint dwCreationDisposition,
        uint dwFlagsAndAttributes,
        IntPtr hTemplateFile);

    private const int ATTACH_PARENT_PROCESS = -1;
    private const int SW_HIDE = 0;
    private const int SW_SHOW = 5;
    private const uint SC_CLOSE = 0xF060;
    private const uint MF_BYCOMMAND = 0x00000000;

    private const int STD_OUTPUT_HANDLE = -11;
    private const int STD_ERROR_HANDLE = -12;
    private const uint GENERIC_WRITE = 0x40000000;
    private const uint FILE_SHARE_WRITE = 0x00000002;
    private const uint OPEN_EXISTING = 3;

    private static bool _isConsoleVisible = false;
    private static bool _consoleInitialized = false;
    private static bool _isDebugActive = false;
    private static bool _alwaysDebug = false;

    private static JRiverMcwsClient? _mcwsClient;
    private static SmtcManager? _smtcManager;
    private static AppConfig _config = new();
    private static PlaybackInfo? _latestPlaybackInfo;
    private static bool _isConnected = false;

    [STAThread]
    static async Task Main(string[] args)
    {
        // 1. 初始化 WinForms 运行环境（采用 PerMonitorV2 确保各显示器与高 DPI 自适应缩放）
        Application.SetHighDpiMode(HighDpiMode.PerMonitorV2);
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);

        // 2. 解析命令行参数
        bool? debugArg = null;
        for (int i = 0; i < args.Length; i++)
        {
            var arg = args[i].ToLowerInvariant();
            if (arg is "--debug" or "-d")
            {
                debugArg = true;
            }
            else if (arg is "--no-debug")
            {
                debugArg = false;
            }
            else if (arg is "--help" or "-h")
            {
                EnsureConsole();
                ShowHelp();
                return;
            }
        }

        // 3. 加载配置（首次启动时主动弹出高 DPI 自适应配置窗提醒输入用户名与密码）
        _config = LoadConfiguration();
        _alwaysDebug = debugArg ?? _config.Debug;
        _isDebugActive = _alwaysDebug;

        // 4. 初始化为 Debug 模式时直接展开控制台
        if (_alwaysDebug)
        {
            EnsureConsole();
            PrintBanner(_config);
        }

        using var cts = new CancellationTokenSource();

        // 5. 创建系统托盘图标
        using var notifyIcon = new NotifyIcon
        {
            Icon = DefaultAssets.GetAppIcon(),
            Text = "JRiver SMTC Bridge",
            Visible = true
        };

        // 托盘右键菜单
        var contextMenu = new ContextMenuStrip();
        var titleItem = new ToolStripMenuItem("JRiver SMTC Bridge")
        {
            Enabled = false,
            Font = new Font(Control.DefaultFont, FontStyle.Bold)
        };
        var statusItem = new ToolStripMenuItem("状态: 正在连接 JRiver...") { Enabled = false };
        var trackItem = new ToolStripMenuItem("曲目: 等待播放...") { Enabled = false };

        var configItem = new ToolStripMenuItem("连接与认证设置...");
        configItem.Click += (s, e) =>
        {
            if (ShowConfigDialog(_config, isFirstRun: false))
            {
                _mcwsClient?.UpdateConnection(_config.Host, _config.Port, _config.Username, _config.Password);
                notifyIcon.ShowBalloonTip(2000, "JRiver SMTC Bridge", "连接设置已更新，正在尝试重新连接...", ToolTipIcon.Info);
            }
        };

        var toggleConsoleItem = new ToolStripMenuItem(_isConsoleVisible ? "隐藏调试控制台" : "显示调试控制台");
        toggleConsoleItem.Click += (s, e) =>
        {
            ToggleConsole();
            toggleConsoleItem.Text = _isConsoleVisible ? "隐藏调试控制台" : "显示调试控制台";
        };

        var exitItem = new ToolStripMenuItem("退出 (Exit)");
        exitItem.Click += (s, e) =>
        {
            cts.Cancel();
            Application.Exit();
        };

        contextMenu.Items.Add(titleItem);
        contextMenu.Items.Add(statusItem);
        contextMenu.Items.Add(trackItem);
        contextMenu.Items.Add(new ToolStripSeparator());
        contextMenu.Items.Add(configItem);
        contextMenu.Items.Add(toggleConsoleItem);
        contextMenu.Items.Add(new ToolStripSeparator());
        contextMenu.Items.Add(exitItem);

        notifyIcon.ContextMenuStrip = contextMenu;

        // 双击托盘图标切换控制台显示/隐藏
        notifyIcon.DoubleClick += (s, e) =>
        {
            ToggleConsole();
            toggleConsoleItem.Text = _isConsoleVisible ? "隐藏调试控制台" : "显示调试控制台";
        };

        // 6. 初始化 MCWS 客户端与 SMTC 管理器
        _mcwsClient = new JRiverMcwsClient(
            _config.Host,
            _config.Port,
            _config.Username,
            _config.Password
        )
        {
            IsDebugEnabled = _isDebugActive
        };

        _mcwsClient.AuthFailed += () =>
        {
            notifyIcon.ShowBalloonTip(3000, "JRiver 认证失败", "MCWS 身份验证失败 (401)，请在托盘右键菜单中打开“连接与认证设置”输入正确的用户名和密码。", ToolTipIcon.Warning);
        };

        _smtcManager = new SmtcManager(_mcwsClient, _isDebugActive);

        // 绑定状态到托盘更新
        _smtcManager.TrackChanged += info =>
        {
            try
            {
                trackItem.Text = $"曲目: {info.Title} - {info.Artist}";
                string tooltip = $"JRiver SMTC Bridge\n正在播放: {info.Title} - {info.Artist}";
                if (tooltip.Length > 63) tooltip = tooltip.Substring(0, 60) + "...";
                notifyIcon.Text = tooltip;
            }
            catch { }
        };

        _smtcManager.StateChanged += state =>
        {
            try
            {
                statusItem.Text = state switch
                {
                    PlaybackState.Playing => "状态: 正在播放",
                    PlaybackState.Paused => "状态: 已暂停",
                    PlaybackState.Stopped => "状态: 已停止",
                    _ => "状态: 未连接"
                };
            }
            catch { }
        };

        // 处理控制台 Ctrl+C
        Console.CancelKeyPress += (s, e) =>
        {
            e.Cancel = true;
            cts.Cancel();
            Application.Exit();
        };

        // 7. 在后台任务运行主同步轮询循环
        var syncTask = Task.Run(async () =>
        {
            int pollInterval = Math.Max(_config.PollIntervalMs, 100);

            while (!cts.IsCancellationRequested)
            {
                try
                {
                    var playbackInfo = await _mcwsClient.GetPlaybackInfoAsync(cts.Token);
                    _latestPlaybackInfo = playbackInfo;

                    if (playbackInfo != null)
                    {
                        if (!_isConnected)
                        {
                            _isConnected = true;
                            statusItem.Text = "状态: 已连接 JRiver";
                            if (_isDebugActive)
                            {
                                Console.ForegroundColor = ConsoleColor.Green;
                                Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] 成功连接至 JRiver Media Center!");
                                Console.ResetColor();
                            }
                            notifyIcon.ShowBalloonTip(2000, "JRiver SMTC Bridge", "已连接至 JRiver Media Center", ToolTipIcon.Info);
                        }

                        _smtcManager.Update(playbackInfo);
                    }
                    else
                    {
                        if (_isConnected)
                        {
                            _isConnected = false;
                            statusItem.Text = "状态: JRiver 已断开";
                            trackItem.Text = "曲目: (无)";
                            notifyIcon.Text = "JRiver SMTC Bridge\n等待连接 JRiver...";
                            _smtcManager.SetDisconnected();
                            if (_isDebugActive)
                            {
                                Console.ForegroundColor = ConsoleColor.Yellow;
                                Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] 与 JRiver 的连接已断开，等待重新连接...");
                                Console.ResetColor();
                            }
                        }
                    }
                }
                catch (OperationCanceledException)
                {
                    break;
                }
                catch (Exception ex)
                {
                    if (_isDebugActive)
                    {
                        Console.WriteLine($"[Error] 同步异常: {ex.Message}");
                    }
                }

                try
                {
                    await Task.Delay(pollInterval, cts.Token);
                }
                catch (OperationCanceledException)
                {
                    break;
                }
            }

            _smtcManager.SetDisconnected();
        }, cts.Token);

        // 8. 启动消息循环保持托盘图标响应
        Application.Run();

        // 退出清理
        cts.Cancel();
        notifyIcon.Visible = false;
        try { await syncTask; } catch { }

        _smtcManager.Dispose();
        _mcwsClient.Dispose();
    }

    private static void SetDebugActive(bool active)
    {
        _isDebugActive = active;
        if (_mcwsClient != null) _mcwsClient.IsDebugEnabled = active;
        if (_smtcManager != null) _smtcManager.IsDebugEnabled = active;
    }

    private static void EnsureConsole()
    {
        if (_consoleInitialized)
        {
            var h = GetConsoleWindow();
            if (h != IntPtr.Zero)
            {
                ShowWindow(h, SW_SHOW);
                _isConsoleVisible = true;
            }
            return;
        }

        // 优先附加到父进程控制台（命令行启动）
        if (!AttachConsole(ATTACH_PARENT_PROCESS))
        {
            // 否则分配新的控制台窗口（双击启动）
            AllocConsole();
        }

        // 设置控制台为 UTF-8 编码
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);

        var hWnd = GetConsoleWindow();
        if (hWnd != IntPtr.Zero)
        {
            // 禁用控制台右上角关闭按钮，防止误关导致主程序闪退
            var hMenu = GetSystemMenu(hWnd, false);
            if (hMenu != IntPtr.Zero)
            {
                DeleteMenu(hMenu, SC_CLOSE, MF_BYCOMMAND);
            }

            ShowWindow(hWnd, SW_SHOW);
            _isConsoleVisible = true;
        }

        try
        {
            var handle = CreateFile("CONOUT$", GENERIC_WRITE, FILE_SHARE_WRITE, IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
            if (!handle.IsInvalid)
            {
                SetStdHandle(STD_OUTPUT_HANDLE, handle.DangerousGetHandle());
                SetStdHandle(STD_ERROR_HANDLE, handle.DangerousGetHandle());
                var fs = new FileStream(handle, FileAccess.Write);
                var writer = new StreamWriter(fs, new UTF8Encoding(false)) { AutoFlush = true };
                Console.SetOut(writer);
                Console.SetError(writer);
            }
        }
        catch { }

        Console.OutputEncoding = new UTF8Encoding(false);
        Console.InputEncoding = new UTF8Encoding(false);
        Console.Title = "JRiver SMTC Bridge - 调试日志";
        _consoleInitialized = true;
    }

    private static void ToggleConsole()
    {
        if (!_consoleInitialized)
        {
            EnsureConsole();
            SetDebugActive(true);
            PrintBanner(_config);
            PrintCurrentSnapshot();
            return;
        }

        var hWnd = GetConsoleWindow();
        if (hWnd == IntPtr.Zero) return;

        if (_isConsoleVisible)
        {
            ShowWindow(hWnd, SW_HIDE);
            _isConsoleVisible = false;

            // 若非命令行强制 debug 或配置文件开启 debug，隐藏控制台时关闭冗余输出
            if (!_alwaysDebug)
            {
                SetDebugActive(false);
            }
        }
        else
        {
            ShowWindow(hWnd, SW_SHOW);
            _isConsoleVisible = true;
            SetDebugActive(true);

            PrintBanner(_config);
            PrintCurrentSnapshot();
        }
    }

    private static void PrintBanner(AppConfig config)
    {
        Console.ForegroundColor = ConsoleColor.Cyan;
        Console.WriteLine("\n==================================================");
        Console.WriteLine("  JRiver MCWS -> Windows SMTC 状态同步桥接器");
        Console.WriteLine("  Cute Anime Edition (实时调试日志模式)");
        Console.WriteLine("==================================================");
        Console.ResetColor();
        Console.WriteLine($"[Config] 连接目标: http://{config.Host}:{config.Port}");
        Console.WriteLine($"[Config] 认证配置: {(string.IsNullOrEmpty(config.Username) ? "无认证" : $"用户 {config.Username}")}");
        Console.WriteLine($"[Config] 轮询间隔: {config.PollIntervalMs} ms");
    }

    private static void PrintCurrentSnapshot()
    {
        Console.ForegroundColor = ConsoleColor.Cyan;
        Console.WriteLine($"[{DateTime.Now:HH:mm:ss}] --- 当前运行状态快照 ---");
        Console.ResetColor();

        if (_isConnected)
        {
            Console.ForegroundColor = ConsoleColor.Green;
            Console.WriteLine($"[连接状态] 已连接至 JRiver Media Center");
            Console.ResetColor();
        }
        else
        {
            Console.ForegroundColor = ConsoleColor.Yellow;
            Console.WriteLine($"[连接状态] 正在尝试连接 JRiver Media Center...");
            Console.ResetColor();
        }

        if (_latestPlaybackInfo != null && _latestPlaybackInfo.State != PlaybackState.Stopped)
        {
            var pos = TimeSpan.FromMilliseconds(_latestPlaybackInfo.PositionMs);
            var dur = TimeSpan.FromMilliseconds(_latestPlaybackInfo.DurationMs);
            Console.WriteLine($"[当前曲目] {_latestPlaybackInfo.Title} - {_latestPlaybackInfo.Artist}");
            Console.WriteLine($"[当前专辑] {_latestPlaybackInfo.Album}");
            Console.WriteLine($"[当前进度] {pos:mm\\:ss} / {dur:mm\\:ss} (状态: {_latestPlaybackInfo.State})");
        }
        else
        {
            Console.WriteLine("[播放状态] 当前未在播放任何曲目 (Stopped)");
        }

        Console.ForegroundColor = ConsoleColor.DarkGray;
        Console.WriteLine($"[实时日志] 实时事件与时间轴日志已激活 (双击托盘图标可隐藏此窗口)\n");
        Console.ResetColor();
    }

    private static void ShowHelp()
    {
        Console.WriteLine("JRiver SMTC Bridge - 使用说明");
        Console.WriteLine("用法: JRiver.SmtcBridge.exe [选项]");
        Console.WriteLine("选项:");
        Console.WriteLine("  -d, --debug     在控制台输出实时同步与调试日志");
        Console.WriteLine("  --no-debug      完全静默后台运行（仅托盘图标）");
        Console.WriteLine("  -h, --help      显示本帮助信息");
    }

    private static AppConfig LoadConfiguration()
    {
        var config = new AppConfig();
        var baseDir = AppDomain.CurrentDomain.BaseDirectory;
        var defaultPath = Path.Combine(baseDir, "appsettings.json");
        var localPath = Path.Combine(baseDir, "appsettings.local.json");

        bool isFirstRun = !File.Exists(defaultPath) && !File.Exists(localPath);

        if (isFirstRun)
        {
            // 首次启动：主动弹出自适应 DPI 配置窗口让用户输入用户名和密码
            ShowConfigDialog(config, isFirstRun: true);
        }
        else
        {
            LoadFromFile(defaultPath, config);
            LoadFromFile(localPath, config);
        }

        return config;
    }

    public static bool ShowConfigDialog(AppConfig config, bool isFirstRun)
    {
        using var form = new Form
        {
            Text = isFirstRun ? "JRiver SMTC Bridge - 首次连接设置" : "JRiver SMTC Bridge - 连接与认证设置",
            Icon = DefaultAssets.GetAppIcon(),
            FormBorderStyle = FormBorderStyle.FixedDialog,
            StartPosition = FormStartPosition.CenterScreen,
            MaximizeBox = false,
            MinimizeBox = false,
            TopMost = true,
            AutoScaleMode = AutoScaleMode.Dpi,
            Font = new Font("Segoe UI", 9.5F, FontStyle.Regular),
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            MinimumSize = new Size(480, 0)
        };

        var mainPanel = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            ColumnCount = 1,
            RowCount = 4,
            Padding = new Padding(24, 20, 24, 20)
        };
        mainPanel.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));

        var lblTitle = new Label
        {
            Text = isFirstRun ? "欢迎使用 JRiver SMTC Bridge！" : "JRiver MCWS 连接设置",
            Font = new Font(form.Font.FontFamily, 12F, FontStyle.Bold),
            AutoSize = true,
            Margin = new Padding(0, 0, 0, 6)
        };

        var lblTip = new Label
        {
            Text = "请配置 JRiver Media Center 的 MCWS 连接信息。\n提示：若 JRiver 未启用身份验证，用户名和密码直接留空即可。",
            AutoSize = true,
            ForeColor = Color.DimGray,
            Margin = new Padding(0, 0, 0, 16)
        };

        var grid = new TableLayoutPanel
        {
            Dock = DockStyle.Fill,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            ColumnCount = 2,
            RowCount = 5,
            Margin = new Padding(0, 0, 0, 16)
        };
        grid.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        grid.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100F));

        var lblHost = new Label
        {
            Text = "服务地址 (Host):",
            Anchor = AnchorStyles.Left,
            AutoSize = true,
            Margin = new Padding(0, 6, 12, 6)
        };
        var txtHost = new TextBox
        {
            Text = config.Host,
            Anchor = AnchorStyles.Left | AnchorStyles.Right,
            MinimumSize = new Size(240, 0),
            Margin = new Padding(0, 4, 0, 6)
        };

        var lblPort = new Label
        {
            Text = "MCWS 端口 (Port):",
            Anchor = AnchorStyles.Left,
            AutoSize = true,
            Margin = new Padding(0, 6, 12, 6)
        };
        var txtPort = new TextBox
        {
            Text = config.Port.ToString(),
            Anchor = AnchorStyles.Left | AnchorStyles.Right,
            Margin = new Padding(0, 4, 0, 6)
        };

        var lblUser = new Label
        {
            Text = "用户名 (Username):",
            Anchor = AnchorStyles.Left,
            AutoSize = true,
            Margin = new Padding(0, 6, 12, 6)
        };
        var txtUser = new TextBox
        {
            Text = config.Username ?? "",
            Anchor = AnchorStyles.Left | AnchorStyles.Right,
            Margin = new Padding(0, 4, 0, 6)
        };

        var lblPass = new Label
        {
            Text = "密码 (Password):",
            Anchor = AnchorStyles.Left,
            AutoSize = true,
            Margin = new Padding(0, 6, 12, 6)
        };
        var txtPass = new TextBox
        {
            Text = config.Password ?? "",
            Anchor = AnchorStyles.Left | AnchorStyles.Right,
            UseSystemPasswordChar = true,
            Margin = new Padding(0, 4, 0, 6)
        };

        var chkShowPass = new CheckBox
        {
            Text = "显示密码",
            Anchor = AnchorStyles.Left,
            AutoSize = true,
            Margin = new Padding(0, 2, 0, 6)
        };
        chkShowPass.CheckedChanged += (s, e) => txtPass.UseSystemPasswordChar = !chkShowPass.Checked;

        grid.Controls.Add(lblHost, 0, 0);
        grid.Controls.Add(txtHost, 1, 0);
        grid.Controls.Add(lblPort, 0, 1);
        grid.Controls.Add(txtPort, 1, 1);
        grid.Controls.Add(lblUser, 0, 2);
        grid.Controls.Add(txtUser, 1, 2);
        grid.Controls.Add(lblPass, 0, 3);
        grid.Controls.Add(txtPass, 1, 3);
        grid.Controls.Add(chkShowPass, 1, 4);

        var buttonPanel = new FlowLayoutPanel
        {
            FlowDirection = FlowDirection.RightToLeft,
            Dock = DockStyle.Fill,
            AutoSize = true,
            AutoSizeMode = AutoSizeMode.GrowAndShrink,
            Margin = new Padding(0, 8, 0, 0)
        };

        var btnCancel = new Button
        {
            Text = isFirstRun ? "跳过 (默认)" : "取消",
            DialogResult = DialogResult.Cancel,
            AutoSize = true,
            Padding = new Padding(16, 6, 16, 6),
            Margin = new Padding(0, 0, 0, 0),
            Cursor = Cursors.Hand
        };

        var btnOk = new Button
        {
            Text = isFirstRun ? "保存并启动" : "保存设置",
            DialogResult = DialogResult.OK,
            AutoSize = true,
            Padding = new Padding(16, 6, 16, 6),
            Margin = new Padding(0, 0, 10, 0),
            Cursor = Cursors.Hand
        };

        buttonPanel.Controls.Add(btnCancel);
        buttonPanel.Controls.Add(btnOk);

        form.AcceptButton = btnOk;
        form.CancelButton = btnCancel;

        mainPanel.Controls.Add(lblTitle, 0, 0);
        mainPanel.Controls.Add(lblTip, 0, 1);
        mainPanel.Controls.Add(grid, 0, 2);
        mainPanel.Controls.Add(buttonPanel, 0, 3);

        form.Controls.Add(mainPanel);

        var result = form.ShowDialog();
        if (result == DialogResult.OK)
        {
            config.Host = string.IsNullOrWhiteSpace(txtHost.Text) ? "127.0.0.1" : txtHost.Text.Trim();
            if (int.TryParse(txtPort.Text.Trim(), out int port) && port > 0)
            {
                config.Port = port;
            }
            config.Username = string.IsNullOrWhiteSpace(txtUser.Text) ? "" : txtUser.Text.Trim();
            config.Password = txtPass.Text;
            SaveConfiguration(config);
            return true;
        }
        else if (isFirstRun)
        {
            // 首次启动跳过时保存默认配置模版，避免下次重复弹窗
            SaveConfiguration(config);
        }

        return false;
    }

    private static void SaveConfiguration(AppConfig config)
    {
        try
        {
            var baseDir = AppDomain.CurrentDomain.BaseDirectory;
            var defaultPath = Path.Combine(baseDir, "appsettings.json");
            var options = new JsonSerializerOptions { WriteIndented = true };
            var root = new { JRiver = config };
            string json = JsonSerializer.Serialize(root, options);
            File.WriteAllText(defaultPath, json, Encoding.UTF8);
        }
        catch (Exception ex)
        {
            MessageBox.Show($"保存配置文件失败: {ex.Message}", "错误", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private static void LoadFromFile(string path, AppConfig target)
    {
        if (!File.Exists(path)) return;

        try
        {
            var json = File.ReadAllText(path);
            using var doc = JsonDocument.Parse(json);
            if (doc.RootElement.TryGetProperty("JRiver", out var jriverSection))
            {
                if (jriverSection.TryGetProperty("Host", out var host)) target.Host = host.GetString() ?? target.Host;
                if (jriverSection.TryGetProperty("Port", out var port)) target.Port = port.GetInt32();
                if (jriverSection.TryGetProperty("Username", out var username)) target.Username = username.GetString();
                if (jriverSection.TryGetProperty("Password", out var password)) target.Password = password.GetString();
                if (jriverSection.TryGetProperty("PollIntervalMs", out var poll)) target.PollIntervalMs = poll.GetInt32();
                if (jriverSection.TryGetProperty("Debug", out var debug)) target.Debug = debug.GetBoolean();
            }
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[Config] 加载配置文件失败 ({path}): {ex.Message}");
        }
    }
}
