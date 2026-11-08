using System.Text.Json;

namespace HumanPlayRecorder;

// Evidence supplements the registry scan; it never disables a mod itself.
public sealed class RememberedContentMods(string path)
{
    static readonly object Sync = new();
    public string[] Read()
    {
        lock (Sync)
            return File.Exists(path)
                ? JsonSerializer.Deserialize<string[]>(File.ReadAllBytes(path)) ?? []
                : [];
    }

    public void Remember(string id)
    {
        if (string.IsNullOrWhiteSpace(id)) return;
        lock (Sync)
        {
            var previous = Read();
            if (previous.Contains(id, StringComparer.Ordinal)) return;
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            string temp = path + ".tmp";
            File.WriteAllBytes(temp, JsonSerializer.SerializeToUtf8Bytes(
                previous.Append(id).Distinct(StringComparer.Ordinal).Order(StringComparer.Ordinal).ToArray()));
            File.Move(temp, path, overwrite: true);
        }
    }

    public string[] Offer(IEnumerable<string> scanned, IEnumerable<string> loaded) =>
        scanned.Concat(Read()).Intersect(loaded, StringComparer.Ordinal)
            .Order(StringComparer.Ordinal).ToArray();
}
