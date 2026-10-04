namespace JRiver.SmtcBridge.Models;

public enum PlaybackState
{
    Stopped = 0,
    Paused = 1,
    Playing = 2,
    Waiting = 3
}

public class PlaybackInfo
{
    public PlaybackState State { get; set; } = PlaybackState.Stopped;
    public string Status { get; set; } = "Stopped";
    public string Title { get; set; } = string.Empty;
    public string Artist { get; set; } = string.Empty;
    public string Album { get; set; } = string.Empty;
    public string AlbumArtist { get; set; } = string.Empty;
    public uint TrackNumber { get; set; }
    public uint AlbumTrackCount { get; set; }
    public string Genre { get; set; } = string.Empty;
    public string FileKey { get; set; } = string.Empty;
    public long PositionMs { get; set; }
    public long DurationMs { get; set; }

    public bool IsPlaying => State == PlaybackState.Playing;
    public bool IsPaused => State == PlaybackState.Paused;
    public bool IsStopped => State == PlaybackState.Stopped;

    public bool IsSameTrack(PlaybackInfo other)
    {
        if (other == null) return false;
        if (!string.IsNullOrEmpty(FileKey) && !string.IsNullOrEmpty(other.FileKey))
        {
            return FileKey == other.FileKey;
        }
        return Title == other.Title && Artist == other.Artist && Album == other.Album;
    }
}
