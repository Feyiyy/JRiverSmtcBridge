# JRiver.SmtcBridge

这是一个轻量级的 Windows 控制台后台服务，用于将 **JRiver Media Center** 的播放状态（曲目名称、艺术家、专辑、专辑封面、毫秒级进度）通过其 **MCWS (Media Center Web Service)** 网络接口转换为标准的 **Windows SMTC (System Media Transport Controls)** 系统媒体会话。

### 为什么需要它？
- JRiver 默认在使用 ASIO / WASAPI 独占输出时，不会向 Windows SMTC 同步播放元数据。
- **BetterLyrics** 等现代歌词软件依赖 Windows SMTC 获取正在播放的歌曲和进度。
- 运行本桥接程序后，Windows 任务栏、锁屏控件以及 **BetterLyrics** 能够自动识别 JRiver 的播放进度并滚动歌词，同时支持通过系统媒体快捷键控制播放/暂停/上一曲/下一曲。

---

### 1. JRiver 端配置准备
在使用前，请确保 JRiver 的网络服务已启用：
1. 打开 JRiver -> 顶部菜单 **工具 (Tools)** -> **选项 (Options)**（快捷键 `Ctrl + O`）；
2. 左侧点击 **媒体网络 (Media Network)**；
3. 勾选 **使用媒体网络共享此库并启用 DLNA**；
4. 默认端口为 `52199`；
5. 在下方 **高级 (Advanced)** 中：
   - 如果启用了身份验证（用户名和密码），需要在配置文件中填写；若为个人本地电脑使用，也可关闭只读认证以简化配置。

---

### 2. 配置文件说明 (`appsettings.json`)
```json
{
  "JRiver": {
    "Host": "127.0.0.1",
    "Port": 52199,
    "Username": "",
    "Password": "",
    "PollIntervalMs": 500
  }
}
```
- `Host`：JRiver 服务地址，本地运行使用 `127.0.0.1` 即可。
- `Port`：MCWS 端口，默认 `52199`。
- `Username` / `Password`：JRiver 媒体网络的认证凭据（若 JRiver 未开启密码保护则留空）。
- `PollIntervalMs`：状态轮询间隔（毫秒），默认 `500` ms，保证歌词滚动平滑且 CPU 占用极低。

> **提示（防私密泄漏）**：如果你要将本仓库推送到 GitHub，建议在本地新建一个 `appsettings.local.json` 来保存真实的账号和密码。该文件已被 `.gitignore` 自动忽略，不会被提交。

---

### 3. 运行方法
在终端进入本目录执行：
```bash
cd JRiver.SmtcBridge
dotnet run
```
或者使用 Rider / Visual Studio 打开根目录下的 `BatterLyrics_for_Jriver.sln` 并启动 `JRiver.SmtcBridge`。
