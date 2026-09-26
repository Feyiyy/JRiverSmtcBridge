# JRiver.SmtcBridge

[简体中文](#zh-cn) | [English](#en-us)

---

<a id="zh-cn"></a>
## 简体中文

> [!WARNING]
> **免责声明**：本项目完全采用 **Vibe Coding**（AI 编程 / 凭感觉写代码）搞出来的，不保证任何代码质量、稳定性与可用性。不欢迎提交 PR，没必要当成代码来学习，单纯为了个人自用。有 Bug 请自行修改或弃用。

### 简介
JRiver.SmtcBridge 是一个轻量级的 Windows 后台工具，用于将 **JRiver Media Center** 的播放状态（正在播放歌曲、艺术家、专辑、专辑封面、毫秒级播放进度）通过其内置的 **MCWS (Media Center Web Service)** 网络接口实时同步到 **Windows SMTC (系统媒体传输控件)**。

### 推荐配套软件与适用场景
当 JRiver 使用 ASIO 或 WASAPI 独占输出时，Windows 系统通常无法感知其播放状态。借助本工具，可以将播放信息完整无缝对接到各种支持 Windows SMTC 的现代歌词软件与系统控件中：
- **[BetterLyrics](https://github.com/chirs241097/BetterLyrics)**：高度可定制的桌面歌词软件，支持平滑滚动与动态效果。
- **[Lyricify](https://lyricify.app/)**：功能强大的滚动歌词软件，支持桌面悬浮歌词与状态栏显示。
- **Windows 系统原生控件**：任务栏音量/媒体飞出面板、锁屏播放控件以及键盘媒体快捷键（播放/暂停/上一曲/下一曲）。

---

### 使用教程

#### 1. JRiver Media Center 设置
1. 打开 JRiver Media Center，点击顶部菜单 **工具 (Tools)** -> **选项 (Options)**；
2. 在左侧列表中选择 **媒体网络 (Media Network)**；
3. 勾选 **使用 Media Network 共享此媒体库，并启用 DLNA**（此选项会同时开启媒体网络与 **MCWS 网络服务**）；
4. 展开下方的 **高级 (Advanced)**：
   - 确认 **TCP 端口** 为默认的 `52199`（若有修改需同步修改配置文件）；
   - 确认 **MCWS (网络服务)** 处于可用状态；
5. （可选）身份验证设置：
   - 个人本机使用建议取消勾选 **只读身份验证** 与 **身份验证**，无需密码即可直接连接；
   - 若启用了身份验证，请在 `appsettings.json` 中配置对应的用户名与密码。

#### 2. 配置文件说明 (`appsettings.json`)
程序同级目录下自带 `appsettings.json` 配置文件：
```json
{
  "JRiver": {
    "Host": "127.0.0.1",
    "Port": 52199,
    "Username": "",
    "Password": "",
    "PollIntervalMs": 500,
    "Debug": false
  }
}
```
- `Host`：JRiver 服务地址，本机运行保持 `127.0.0.1` 即可。
- `Port`：MCWS 端口，保持与 JRiver 媒体网络中的端口一致（默认 `52199`）。
- `Username` / `Password`：JRiver 媒体网络的用户名和密码（未启用验证则留空）。
- `PollIntervalMs`：状态轮询间隔（毫秒），默认 `500` ms。内置时间轴智能去重算法，保证歌词平滑推算滚动且不抖动。
- `Debug`：调试日志开关，默认 `false`。如需排查连接或查看时间轴同步细节，可设为 `true`。

#### 3. 运行程序
直接双击运行 `JRiver.SmtcBridge.exe`。连接成功后，打开 **BetterLyrics** 或 **Lyricify** 即可畅享歌词同步与媒体控制！

---

### 开源协议
本项目采用 [WTFPL](LICENSE)（Do What The Fuck You Want To Public License）开源协议——你想干嘛就干嘛，爱怎么改怎么改。

---

<a id="en-us"></a>
## English

> [!WARNING]
> **Disclaimer**: This project is entirely cobbled together via **Vibe Coding** (AI-assisted / vibing). The author guarantees **zero code quality, stability, or usability**. PRs are **not welcome**, and there's nothing here worth learning—it was made purely for personal convenience. If you hit a bug, fix it yourself or stop using it.

### Introduction
JRiver.SmtcBridge is a lightweight Windows background utility that seamlessly synchronizes the playback status (track title, artist, album, album art, and millisecond-accurate progress) of **JRiver Media Center** to the standard **Windows SMTC (System Media Transport Controls)** via its built-in **MCWS (Media Center Web Service)** interface.

### Recommended Companion Apps & Features
When JRiver plays audio via ASIO or WASAPI exclusive modes, Windows cannot naturally detect its playback state. JRiver.SmtcBridge bridges this gap, enabling rich integration with modern lyrics apps and system controls:
- **[BetterLyrics](https://github.com/chirs241097/BetterLyrics)**: Highly customizable desktop lyrics application featuring smooth scrolling and dynamic effects.
- **[Lyricify](https://lyricify.app/)**: Powerful synchronized lyrics software with desktop floating and status bar widgets.
- **Native Windows Controls**: Taskbar media/volume flyout, lock screen controls, and hardware media keys (Play/Pause/Next/Previous).

---

### Setup Guide

#### 1. JRiver Media Center Configuration
1. Open JRiver Media Center, go to the top menu: **Tools** -> **Options**;
2. In the left panel, select **Media Network**;
3. Check **Use Media Network to share this library and enable DLNA** (this also activates the **MCWS web service**);
4. Expand **Advanced**:
   - Ensure the **TCP Port** is set to default `52199` (if modified, update the config file accordingly);
   - Verify that **MCWS (Network Services)** is active and available;
5. (Optional) Authentication:
   - For local personal use, you can uncheck **Read-only authentication** and **Authentication** to simplify connection without credentials;
   - If authentication is enabled, enter your username and password into `appsettings.json`.

#### 2. Configuration (`appsettings.json`)
The `appsettings.json` file is located next to the executable:
```json
{
  "JRiver": {
    "Host": "127.0.0.1",
    "Port": 52199,
    "Username": "",
    "Password": "",
    "PollIntervalMs": 500,
    "Debug": false
  }
}
```
- `Host`: JRiver server address, keep `127.0.0.1` for local playback.
- `Port`: MCWS port, matching the port in JRiver Media Network (default `52199`).
- `Username` / `Password`: Credentials for JRiver Media Network (leave blank if authentication is disabled).
- `PollIntervalMs`: Status polling interval in milliseconds (default `500` ms). Features smart timeline deduplication to keep lyrics scrolling smooth without jitter.
- `Debug`: Debug logging toggle (default `false`). Set to `true` to view detailed timeline synchronization logs in console.

#### 3. How to Run
Simply double-click `JRiver.SmtcBridge.exe`. Once connected, open **BetterLyrics** or **Lyricify** to enjoy smooth synchronized lyrics and media controls!

---

### License
Licensed under [WTFPL](LICENSE) (Do What The Fuck You Want To Public License) — do whatever you want with it.
