using System.Drawing;

namespace JRiver.SmtcBridge;

public static class DefaultAssets
{
    private static byte[]? _defaultCoverCache;
    private static Icon? _appIconCache;

    public static byte[] GetDefaultCoverBytes()
    {
        if (_defaultCoverCache != null && _defaultCoverCache.Length > 0)
        {
            return _defaultCoverCache;
        }

        try
        {
            // 1. 优先读取 Assets 目录下的 default_cover.jpg
            string filePath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Assets", "default_cover.jpg");
            if (File.Exists(filePath))
            {
                _defaultCoverCache = File.ReadAllBytes(filePath);
                return _defaultCoverCache;
            }

            // 2. 嵌入资源兜底
            var asm = typeof(DefaultAssets).Assembly;
            using var stream = asm.GetManifestResourceStream("JRiver.SmtcBridge.Assets.default_cover.jpg");
            if (stream != null)
            {
                using var ms = new MemoryStream();
                stream.CopyTo(ms);
                _defaultCoverCache = ms.ToArray();
                return _defaultCoverCache;
            }
        }
        catch { }

        return Array.Empty<byte>();
    }

    public static Icon GetAppIcon()
    {
        if (_appIconCache != null)
        {
            return _appIconCache;
        }

        try
        {
            // 1. 优先读取 Assets 目录下的 app.ico
            string filePath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Assets", "app.ico");
            if (File.Exists(filePath))
            {
                _appIconCache = new Icon(filePath);
                return _appIconCache;
            }

            // 2. 嵌入资源兜底
            var asm = typeof(DefaultAssets).Assembly;
            using var stream = asm.GetManifestResourceStream("JRiver.SmtcBridge.Assets.app.ico");
            if (stream != null)
            {
                _appIconCache = new Icon(stream);
                return _appIconCache;
            }
        }
        catch { }

        return SystemIcons.Application;
    }
}
