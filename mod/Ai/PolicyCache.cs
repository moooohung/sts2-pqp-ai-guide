using System.Net.Http;
using System.Security.Cryptography;
using System.Text.Json;

namespace HumanPlayRecorder.Ai;

internal sealed class PolicyLoadException(AiFailureKind kind, Exception error)
    : Exception(error.Message, error)
{
    internal AiFailureKind Kind { get; } = kind;
}

internal static class PolicyCache
{
    const string Origin = "https://sts.pqp.me";
    static readonly HttpClient Http = new(new HttpClientHandler { AllowAutoRedirect = false }) {
        Timeout = TimeSpan.FromSeconds(45), MaxResponseContentBufferSize = 64 * 1024 * 1024
    };

    internal static byte[] Load(string directory, string gameVersion, CancellationToken ct = default)
    {
        ct.ThrowIfCancellationRequested();
        Directory.CreateDirectory(directory);
        string current = Path.Combine(directory, "current.json");
        Exception? downloadFailure = null;
        try {
            byte[] latest = Download(Origin + "/api/aiplay/policy/latest", 16 * 1024, ct);
            using var manifest = JsonDocument.Parse(latest);
            Validate(manifest.RootElement, gameVersion);
            string hash = manifest.RootElement.GetProperty("weights_sha256").GetString()!;
            string destination = Path.Combine(directory, hash + ".safetensors");
            byte[]? bytes = null;
            if (File.Exists(destination)) {
                var cached = File.ReadAllBytesAsync(destination, ct).GetAwaiter().GetResult();
                if (Hash(cached) == hash) bytes = cached;
            }
            if (bytes is null) {
                try {
                    var url = new Uri(manifest.RootElement.GetProperty("url").GetString()!);
                    if (url.Scheme != "https" || url.Host != "files.catbox.moe" || !url.IsDefaultPort || !string.IsNullOrEmpty(url.UserInfo))
                        throw new InvalidDataException("INVALID_CHECKPOINT_URL");
                    bytes = Download(url.AbsoluteUri, 64 * 1024 * 1024, ct);
                    if (Hash(bytes) != hash) throw new InvalidDataException("CHECKPOINT_HASH_MISMATCH");
                } catch (Exception first) when (!ct.IsCancellationRequested) {
                    try {
                        bytes = Download($"{Origin}/api/aiplay/policy/weights/{hash}.safetensors", 64 * 1024 * 1024, ct);
                        if (Hash(bytes) != hash) throw new InvalidDataException("CHECKPOINT_HASH_MISMATCH");
                    } catch (Exception fallback) when (!ct.IsCancellationRequested) {
                        throw new PolicyLoadException(IsConnectivity(first) && IsConnectivity(fallback)
                            ? AiFailureKind.Offline : AiFailureKind.CheckpointInvalid,
                            new AggregateException(first, fallback));
                    }
                }
                ct.ThrowIfCancellationRequested();
                ValidateWeights(bytes, manifest.RootElement, gameVersion);
                AtomicWrite(destination, bytes, ct);
            }
            ct.ThrowIfCancellationRequested();
            ValidateWeights(bytes, manifest.RootElement, gameVersion);
            AtomicWrite(current, latest, ct);
            return bytes;
        } catch (Exception error) when (!ct.IsCancellationRequested) { downloadFailure = error; }
        ct.ThrowIfCancellationRequested();
        try {
            return LoadCached(directory, gameVersion, ct);
        } catch (Exception error) when (!ct.IsCancellationRequested) {
            var kind = downloadFailure is PolicyLoadException classified ? classified.Kind :
                IsConnectivity(downloadFailure!) ? AiFailureKind.Offline :
                downloadFailure is IOException ? AiFailureKind.Internal : AiFailureKind.CheckpointInvalid;
            // Corrupt cached bytes are an integrity failure, not an offline indicator.
            if (error is PolicyLoadException { Kind: AiFailureKind.CheckpointInvalid } &&
                error.InnerException is not (FileNotFoundException or DirectoryNotFoundException))
                kind = AiFailureKind.CheckpointInvalid;
            throw new PolicyLoadException(kind, new AggregateException(downloadFailure!, error));
        }
    }
    internal static bool IsConnectivity(Exception error) =>
        error is PolicyLoadException { Kind: AiFailureKind.Offline } or TaskCanceledException ||
        error is HttpRequestException { StatusCode: null, HttpRequestError:
            HttpRequestError.ConnectionError or HttpRequestError.NameResolutionError };

    internal static byte[] LoadCached(string directory, string gameVersion, CancellationToken ct = default)
    {
        ct.ThrowIfCancellationRequested();
        try {
            using var cached = JsonDocument.Parse(File.ReadAllBytesAsync(Path.Combine(directory, "current.json"), ct).GetAwaiter().GetResult());
            Validate(cached.RootElement, gameVersion);
            string hash = cached.RootElement.GetProperty("weights_sha256").GetString()!;
            byte[] bytes = File.ReadAllBytesAsync(Path.Combine(directory, hash + ".safetensors"), ct).GetAwaiter().GetResult();
            ct.ThrowIfCancellationRequested();
            if (Hash(bytes) != hash) throw new InvalidDataException("CACHED_CHECKPOINT_HASH_MISMATCH");
            ValidateWeights(bytes, cached.RootElement, gameVersion);
            return bytes;
        } catch (Exception error) when (!ct.IsCancellationRequested) {
            throw new PolicyLoadException(error is IOException
                and not FileNotFoundException and not DirectoryNotFoundException
                ? AiFailureKind.Internal : AiFailureKind.CheckpointInvalid, error);
        }
    }
    static string Hash(byte[] bytes) => Convert.ToHexStringLower(SHA256.HashData(bytes));
    static void Validate(JsonElement manifest, string version)
    {
        string hash = manifest.GetProperty("weights_sha256").GetString()!;
        if (manifest.GetProperty("schema").GetString() != "sts2-policy-web-v2" ||
            manifest.GetProperty("game_version").GetString() != version ||
            hash.Length != 64 || hash.Any(c => !char.IsAsciiHexDigitLower(c)))
            throw new InvalidDataException("INCOMPATIBLE_CHECKPOINT");
    }
    static void ValidateWeights(byte[] bytes, JsonElement manifest, string version)
    {
        if (bytes.Length < 8) throw new InvalidDataException("TRUNCATED_CHECKPOINT");
        ulong length = System.Buffers.Binary.BinaryPrimitives.ReadUInt64LittleEndian(bytes);
        if (length > 1024 * 1024 || length > (ulong)bytes.Length - 8) throw new InvalidDataException("INVALID_CHECKPOINT_HEADER");
        using var header = JsonDocument.Parse(bytes.AsMemory(8, (int)length));
        var metadata = header.RootElement.GetProperty("__metadata__");
        if (metadata.GetProperty("schema").GetString() != "sts2-policy-web-v2" ||
            metadata.GetProperty("game_version").GetString() != version ||
            metadata.GetProperty("checkpoint_id").GetString() != manifest.GetProperty("checkpoint_id").GetString() ||
            metadata.GetProperty("step").GetString() != manifest.GetProperty("step").GetUInt64().ToString(System.Globalization.CultureInfo.InvariantCulture))
            throw new InvalidDataException("CHECKPOINT_METADATA_MISMATCH");
    }
    static byte[] Download(string url, int limit, CancellationToken ct)
    {
        using var response = Http.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, ct).GetAwaiter().GetResult();
        response.EnsureSuccessStatusCode();
        if (response.Content.Headers.ContentLength > limit) throw new InvalidDataException("DOWNLOAD_LIMIT");
        using var stream = response.Content.ReadAsStream(ct);
        using var output = new MemoryStream();
        byte[] buffer = new byte[8192];
        int count;
        while ((count = stream.ReadAsync(buffer.AsMemory(), ct).AsTask().GetAwaiter().GetResult()) != 0) {
            if (output.Length + count > limit) throw new InvalidDataException("DOWNLOAD_LIMIT");
            output.Write(buffer, 0, count);
        }
        return output.ToArray();
    }
    static void AtomicWrite(string path, byte[] bytes, CancellationToken ct)
    {
        string temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try {
            File.WriteAllBytesAsync(temporary, bytes, ct).GetAwaiter().GetResult();
            ct.ThrowIfCancellationRequested();
            File.Move(temporary, path, true);
        } finally {
            if (File.Exists(temporary)) File.Delete(temporary);
        }
    }
}
