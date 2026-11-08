using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Runtime.InteropServices;

static string TypeName(MetadataReader metadata, EntityHandle handle)
{
    if (handle.Kind == HandleKind.TypeReference)
    {
        var type = metadata.GetTypeReference((TypeReferenceHandle)handle);
        return metadata.GetString(type.Namespace) + "." + metadata.GetString(type.Name);
    }
    if (handle.Kind == HandleKind.TypeDefinition)
    {
        var type = metadata.GetTypeDefinition((TypeDefinitionHandle)handle);
        return metadata.GetString(type.Namespace) + "." + metadata.GetString(type.Name);
    }
    return handle.Kind.ToString();
}

// The mod's UI strings ship inside the DLL; the Workshop package carries only the DLL and manifest.
static bool HasLocalization(PEReader pe, MetadataReader metadata)
{
    foreach (var handle in metadata.ManifestResources)
    {
        var resource = metadata.GetManifestResource(handle);
        if (!resource.Implementation.IsNil || metadata.GetString(resource.Name) != "HumanPlayRecorder.ui_strings.json") continue;
        var directory = pe.PEHeaders.CorHeader!.ResourcesDirectory;
        var reader = pe.GetSectionData(directory.RelativeVirtualAddress)
            .GetReader((int)resource.Offset, directory.Size - (int)resource.Offset);
        using var strings = JsonDocument.Parse(reader.ReadBytes(reader.ReadInt32()));
        return strings.RootElement.TryGetProperty("eng", out var eng) &&
            eng.TryGetProperty("mod_name", out var name) && name.ValueKind == JsonValueKind.String;
    }
    return false;
}

static (string[] Initializers, bool DecisionLog, bool ReleaseMarker, bool Localization, string? SourceCommit, bool Advisor, string Edition, string AssemblyName) Inspect(string path)
{
    byte[] bytes = File.ReadAllBytes(path);
    using var pe = new PEReader(new MemoryStream(bytes));
    var metadata = pe.GetMetadataReader();
    var initializers = new List<string>();
    bool releaseMarker = false;
    string? sourceCommit = null;
    // Builds from before the edition switch carry no marker and are the online edition.
    string edition = "Online";
    string assemblyName = metadata.GetString(metadata.GetAssemblyDefinition().Name);
    foreach (var handle in metadata.GetAssemblyDefinition().GetCustomAttributes())
    {
        var attr = metadata.GetCustomAttribute(handle);
        if (attr.Constructor.Kind != HandleKind.MemberReference) continue;
        var ctor = metadata.GetMemberReference((MemberReferenceHandle)attr.Constructor);
        if (TypeName(metadata, ctor.Parent) != "System.Reflection.AssemblyMetadataAttribute") continue;
        var blob = metadata.GetBlobReader(attr.Value);
        if (blob.ReadUInt16() != 1) continue;
        string? key = blob.ReadSerializedString(), value = blob.ReadSerializedString();
        if (key == "HumanPlayRecorder.ReleaseVariant" && value == "release-v1") releaseMarker = true;
        if (key == "HumanPlayRecorder.SourceCommit") sourceCommit = value;
        if (key == "HumanPlayRecorder.Edition" && value is not null) edition = value;
    }
    foreach (var handle in metadata.TypeDefinitions)
    {
        var type = metadata.GetTypeDefinition(handle);
        foreach (var attribute in type.GetCustomAttributes())
        {
            var attr = metadata.GetCustomAttribute(attribute);
            string name;
            if (attr.Constructor.Kind == HandleKind.MemberReference)
                name = TypeName(metadata, metadata.GetMemberReference((MemberReferenceHandle)attr.Constructor).Parent);
            else if (attr.Constructor.Kind == HandleKind.MethodDefinition)
                name = TypeName(metadata, metadata.GetMethodDefinition((MethodDefinitionHandle)attr.Constructor).GetDeclaringType());
            else continue;
            if (name is not ("MegaCrit.Sts2.Core.Modding.ModInitializerAttribute" or "MegaCrit.Sts2.Core.Modding.ModInitializer")) continue;
            var blob = metadata.GetBlobReader(attr.Value);
            if (blob.ReadUInt16() != 1) throw new InvalidDataException("invalid initializer attribute");
            initializers.Add(TypeName(metadata, handle) + "." + blob.ReadSerializedString());
        }
    }
    bool Has(string text) => bytes.AsSpan().IndexOf(Encoding.Unicode.GetBytes(text)) >= 0;
    // The online edition logs skipped run starts from several places; the offline edition skips
    // every run start after the advisor with one fixed reason.
    bool decisionLog = edition == "Offline"
        ? Has("[HumanPlayRecorder] run start ") && Has("offline edition")
        : Has("[HumanPlayRecorder] run start skipped:");
    bool localization = HasLocalization(pe, metadata);
    var typeNames = metadata.TypeDefinitions.Select(h => metadata.GetString(metadata.GetTypeDefinition(h).Name)).ToHashSet();
    var methodNames = metadata.MethodDefinitions.Select(h => metadata.GetString(metadata.GetMethodDefinition(h).Name)).ToHashSet();
    bool advisor = typeNames.Contains("ScreenRows") && methodNames.Contains("RememberSelection") && methodNames.Contains("get_Legal");
    Console.WriteLine($"{path}: bytes={bytes.Length} sha256={Convert.ToHexStringLower(SHA256.HashData(bytes))} assembly={assemblyName} edition={edition} initializers=[{string.Join(",", initializers)}] decision_log={decisionLog} release_marker={releaseMarker} localization={localization} source_commit={sourceCommit} advisor={advisor}");
    return (initializers.ToArray(), decisionLog, releaseMarker, localization, sourceCommit, advisor, edition, assemblyName);
}

static bool IsRelease(string path)
{
    var found = Inspect(path);
    bool identity = found.Edition switch {
        "Online" => found.AssemblyName == "HumanPlayRecorder",
        "Offline" => found.AssemblyName == Gate.OfflineModId && NoNetwork(path),
        _ => false
    };
    return identity && found.Initializers.SequenceEqual(new[] { "HumanPlayRecorder.ReleaseBootstrap.Init" }) &&
        found.Localization &&
        found.DecisionLog && found.ReleaseMarker && found.Advisor && ValidCommit(found.SourceCommit);
}

// No-network gate for the offline edition. Metadata only: the DLL is never loaded or run.
// Any one of these fails it: a System.Net* assembly or type reference, a known network type
// (HttpClient, WebRequest, sockets, DNS, TLS streams, WebSocket), Godot OS.ShellOpen or
// Process.Start (opening a URL in a browser), a native import of a Windows network library,
// an http(s)/ws(s)/ftp/steam URL string anywhere in the file (user strings,
// attribute blobs, embedded resources, UTF-8 or UTF-16), or any OfflineGate allowlist finding.
static List<string> NetworkFindings(string path)
{
    byte[] bytes = File.ReadAllBytes(path);
    using var pe = new PEReader(new MemoryStream(bytes));
    var metadata = pe.GetMetadataReader();
    var findings = new List<string>();
    foreach (var handle in metadata.AssemblyReferences)
    {
        string name = metadata.GetString(metadata.GetAssemblyReference(handle).Name);
        if (name.StartsWith("System.Net", StringComparison.Ordinal)) findings.Add("assembly reference " + name);
    }
    foreach (var handle in metadata.TypeReferences)
    {
        var type = metadata.GetTypeReference(handle);
        string space = metadata.GetString(type.Namespace), name = metadata.GetString(type.Name);
        if (space.StartsWith("System.Net", StringComparison.Ordinal) || Gate.NetworkTypes.Contains(name))
            findings.Add("type reference " + space + "." + name);
    }
    foreach (var handle in metadata.MemberReferences)
    {
        var member = metadata.GetMemberReference(handle);
        string name = metadata.GetString(member.Name);
        string parent = member.Parent.Kind is HandleKind.TypeReference or HandleKind.TypeDefinition
            ? TypeName(metadata, member.Parent) : member.Parent.Kind.ToString();
        if (name == "ShellOpen" || parent == "System.Diagnostics.Process" && name == "Start")
            findings.Add("member reference " + parent + "." + name);
    }
    for (int row = 1; row <= metadata.GetTableRowCount(TableIndex.ModuleRef); row++)
    {
        string name = metadata.GetString(metadata.GetModuleReference(MetadataTokens.ModuleReferenceHandle(row)).Name);
        if (IsNetworkLibrary(name)) findings.Add("native import " + name);
    }
    foreach (string scheme in new[] { "http://", "https://", "ws://", "wss://", "ftp://", "steam://" })
        foreach (var encoding in new Encoding[] { Encoding.UTF8, Encoding.Unicode })
        {
            byte[] needle = encoding.GetBytes(scheme);
            for (int at = bytes.AsSpan().IndexOf(needle), offset = 0; at >= 0;
                 offset += at + needle.Length, at = bytes.AsSpan(offset).IndexOf(needle))
            {
                int start = offset + at;
                string text = encoding.GetString(bytes, start, Math.Min(120, bytes.Length - start));
                text = new string(text.TakeWhile(c => c >= ' ' && c != '"').ToArray());
                if (!Gate.LicenseUrls.Contains(text)) findings.Add($"url string {text}");
            }
        }
    return findings;
}
static bool NoNetwork(string path)
{
    var findings = NetworkFindings(path).Concat(OfflineGate.Findings(path, OfflineGate.GameDll())).ToList();
    foreach (string finding in findings) Console.WriteLine("network finding: " + finding);
    return findings.Count == 0;
}
static bool IsNetworkLibrary(string name)
{
    string file = Path.GetFileNameWithoutExtension(name.ToLowerInvariant());
    return Gate.NetworkLibraries.Contains(file);
}

// Native bridge: the PE import directory (normal and delay-load) names no network library.
static List<string> NativeImports(string path)
{
    using var stream = File.OpenRead(path);
    using var pe = new PEReader(stream);
    var names = new List<string>();
    var header = pe.PEHeaders.PEHeader!;
    void Read(DirectoryEntry directory, int size, int nameOffset)
    {
        if (directory.Size == 0) return;
        var table = pe.GetSectionData(directory.RelativeVirtualAddress).GetReader(0, directory.Size);
        while (table.RemainingBytes >= size)
        {
            byte[] entry = table.ReadBytes(size);
            int rva = BitConverter.ToInt32(entry, nameOffset);
            if (entry.All(b => b == 0)) break;
            var reader = pe.GetSectionData(rva).GetReader();
            var text = new StringBuilder();
            byte value;
            while ((value = reader.ReadByte()) != 0 && text.Length < 256) text.Append((char)value);
            names.Add(text.ToString());
        }
    }
    Read(header.ImportTableDirectory, 20, 12);
    Read(header.DelayImportTableDirectory, 32, 4);
    return names;
}
static bool ValidCommit(string? value) => value is { Length: 40 } && value.All(c => char.IsAsciiHexDigit(c));
static string NativeCommit(string path)
{
    IntPtr library = NativeLibrary.Load(Path.GetFullPath(path));
    try {
        var entry = Marshal.GetDelegateForFunctionPointer<SourceCommitDelegate>(
            NativeLibrary.GetExport(library, "sts2_advisor_source_commit"));
        return Marshal.PtrToStringUTF8(entry()) ?? throw new InvalidDataException("missing native source stamp");
    } finally { NativeLibrary.Free(library); }
}
static void Attest(string managed, string native, string managedCommit, string nativeCommit)
{
    if (!ValidCommit(managedCommit) || !ValidCommit(nativeCommit)) throw new InvalidDataException("expected source commits must be full commit IDs");
    if (!IsRelease(managed)) throw new InvalidDataException("package requires the release variant");
    var found = Inspect(managed);
    if (!found.Advisor || found.SourceCommit != managedCommit) throw new InvalidDataException("managed binary does not match reviewed source commit");
    if (NativeCommit(native) != nativeCommit) throw new InvalidDataException("native binary does not match reviewed source commit");
    if (File.ReadAllBytes(native).AsSpan().IndexOf(Encoding.UTF8.GetBytes("INVALID_LIVE_POLICY_SLOT")) < 0)
        throw new InvalidDataException("native bridge lacks live policy slot validation");
    RejectFaultInjection(native);
}
// Decision-search fault injection lives only in the Rust crates' own unit tests, and every
// injected fault names this marker; a shipped bridge must never contain one.
static void RejectFaultInjection(string native)
{
    if (File.ReadAllBytes(native).AsSpan().IndexOf("STS2_SEARCH_FAULT_INJECTION"u8) >= 0)
        throw new InvalidDataException("native bridge contains search fault injection: " + native);
}

static void InspectNative(string path)
{
    using var stream = File.OpenRead(path);
    using var pe = new PEReader(stream);
    if (pe.HasMetadata || pe.PEHeaders.CoffHeader.Machine != Machine.Amd64 ||
        (pe.PEHeaders.CoffHeader.Characteristics & Characteristics.Dll) == 0)
        throw new InvalidDataException("advisor bridge must be a native Windows x64 DLL");
    var exports = pe.PEHeaders.PEHeader!.ExportTableDirectory;
    if (exports.Size < 40) throw new InvalidDataException("advisor bridge has no export table");
    var table = pe.GetSectionData(exports.RelativeVirtualAddress).GetReader(0, 40);
    table.Offset = 24;
    uint count = table.ReadUInt32();
    table.Offset = 32;
    int namesRva = table.ReadInt32();
    if (count > 4096) throw new InvalidDataException("advisor export table exceeds inspection limit");
    var pointers = pe.GetSectionData(namesRva).GetReader(0, checked((int)count * 4));
    var names = new HashSet<string>(StringComparer.Ordinal);
    for (int i = 0; i < count; i++)
    {
        var name = pe.GetSectionData(pointers.ReadInt32()).GetReader();
        var text = new StringBuilder();
        byte value;
        while ((value = name.ReadByte()) != 0)
        {
            if (text.Length >= 256 || value > 127) throw new InvalidDataException("invalid native export name");
            text.Append((char)value);
        }
        names.Add(text.ToString());
    }
    foreach (var name in new[] { "sts2_advisor_create", "sts2_advisor_request", "sts2_advisor_destroy", "sts2_advisor_source_commit" })
        if (!names.Contains(name)) throw new InvalidDataException("missing native advisor export: " + name);
    RejectFaultInjection(path);
    Console.WriteLine("PASS: native advisor x64 DLL, C ABI exports and no search fault injection: " + path);
}

static (string Hash, string Id, ulong Step) Checkpoint(string directory, string gameHash)
{
    using var current = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(directory, "current.json")));
    var manifest = current.RootElement;
    string hash = manifest.GetProperty("weights_sha256").GetString()!;
    string version = "public-beta+" + gameHash[..8];
    if (manifest.GetProperty("schema").GetString() != "sts2-policy-web-v2" ||
        manifest.GetProperty("game_version").GetString() != version ||
        hash.Length != 64 || hash.Any(c => !char.IsAsciiHexDigitLower(c)))
        throw new InvalidDataException("incompatible bundled checkpoint manifest");
    byte[] weights = File.ReadAllBytes(Path.Combine(directory, hash + ".safetensors"));
    if (Convert.ToHexStringLower(SHA256.HashData(weights)) != hash || weights.Length < 8)
        throw new InvalidDataException("bundled checkpoint hash mismatch");
    ulong length = System.Buffers.Binary.BinaryPrimitives.ReadUInt64LittleEndian(weights);
    if (length > 1024 * 1024 || length > (ulong)weights.Length - 8)
        throw new InvalidDataException("invalid bundled checkpoint header");
    using var header = JsonDocument.Parse(weights.AsMemory(8, (int)length));
    var metadata = header.RootElement.GetProperty("__metadata__");
    string id = manifest.GetProperty("checkpoint_id").GetString()!;
    ulong step = manifest.GetProperty("step").GetUInt64();
    if (metadata.GetProperty("schema").GetString() != "sts2-policy-web-v2" ||
        metadata.GetProperty("game_version").GetString() != version ||
        metadata.GetProperty("checkpoint_id").GetString() != id ||
        metadata.GetProperty("step").GetString() != step.ToString(System.Globalization.CultureInfo.InvariantCulture))
        throw new InvalidDataException("bundled checkpoint metadata mismatch");
    // Decision-time search discounts with the training gamma; weights exported without it run
    // the plain policy only (AiAdvisor), so a bundled checkpoint must carry it.
    if (!metadata.TryGetProperty("gamma", out var gammaText) ||
        !double.TryParse(gammaText.GetString(), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out double gamma) ||
        !(gamma > 0 && gamma <= 1))
        throw new InvalidDataException("bundled checkpoint lacks the training gamma decision search needs");
    Console.WriteLine($"PASS: bundled checkpoint {id} step={step} gamma={gamma.ToString(System.Globalization.CultureInfo.InvariantCulture)} sha256={hash}");
    return (hash, id, step);
}

// Release archive whose bytes depend only on the folder's files and the given time: entries
// "<folder name>/<relative path>" in ordinal order, stored (no compression, so no zlib version
// enters the bytes), all stamped with the same UTC time.
static void ZipFolder(string folder, string zipPath, string isoTime)
{
    folder = Path.GetFullPath(folder);
    var stamp = DateTimeOffset.Parse(isoTime, System.Globalization.CultureInfo.InvariantCulture).ToUniversalTime();
    string root = Path.GetFileName(folder);
    var files = Directory.GetFiles(folder, "*", SearchOption.AllDirectories)
        .Select(path => (Path: path, Name: root + "/" + Path.GetRelativePath(folder, path).Replace('\\', '/')))
        .OrderBy(file => file.Name, StringComparer.Ordinal).ToArray();
    using (var output = new FileStream(zipPath, FileMode.CreateNew))
    using (var zip = new System.IO.Compression.ZipArchive(output, System.IO.Compression.ZipArchiveMode.Create))
        foreach (var file in files)
        {
            var entry = zip.CreateEntry(file.Name, System.IO.Compression.CompressionLevel.NoCompression);
            entry.LastWriteTime = stamp;
            using var source = File.OpenRead(file.Path);
            using var target = entry.Open();
            source.CopyTo(target);
        }
    Console.WriteLine($"PASS: {zipPath}: {files.Length} stored entries under {root}/, stamped {stamp:O}");
}

static void NativeNetworkGate(string path)
{
    var imports = NativeImports(path);
    Console.WriteLine($"{path}: native imports [{string.Join(",", imports)}]");
    var network = imports.Where(IsNetworkLibrary).ToArray();
    if (network.Length != 0) throw new InvalidDataException("native bridge imports network libraries: " + string.Join(",", network));
    Console.WriteLine("PASS: native bridge imports no network library: " + path);
}

static string Sha(byte[] body) => Convert.ToHexStringLower(SHA256.HashData(body));

// The offline mod folder (mods/PqpAiGuideOffline, as unzipped into the game): DLL, manifest, bridge,
// the code's AGPL LICENSE, and policy/{current.json, weights, MODEL-LICENSE.md}.
static string[] OfflineModFiles(string checkpointHash) => [
    Gate.OfflineModId + ".dll", Gate.OfflineModId + ".json", "sts2_advisor_bridge.dll", Gate.CodeLicense,
    Gate.PolicyFolder + "/current.json", Gate.PolicyFolder + "/" + checkpointHash + ".safetensors",
    Gate.PolicyFolder + "/" + Gate.ModelLicense];

static string[] RelativeFiles(string root) => Directory.GetFiles(root, "*", SearchOption.AllDirectories)
    .Select(f => Path.GetRelativePath(root, f).Replace('\\', '/')).Order(StringComparer.Ordinal).ToArray();

static void AssembleOffline(string[] a)
{
    // a: PACKAGE MANAGED BRIDGE MANAGED_COMMIT BRIDGE_COMMIT CHECKPOINT_DIR MODEL_LICENSE MOD_JSON GAME_SHA256 CODE_LICENSE
    string package = Path.GetFullPath(a[0]);
    Attest(a[1], a[2], a[3], a[4]);
    InspectNative(a[2]);
    NativeNetworkGate(a[2]);
    var checkpoint = Checkpoint(a[5], a[8]);
    RequireAgpl(File.ReadAllText(a[9]));
    var sources = new Dictionary<string, string> {
        [Gate.OfflineModId + ".dll"] = a[1], [Gate.OfflineModId + ".json"] = a[7], ["sts2_advisor_bridge.dll"] = a[2],
        [Gate.CodeLicense] = a[9],
        [Gate.PolicyFolder + "/current.json"] = Path.Combine(a[5], "current.json"),
        [Gate.PolicyFolder + "/" + checkpoint.Hash + ".safetensors"] = Path.Combine(a[5], checkpoint.Hash + ".safetensors"),
        [Gate.PolicyFolder + "/" + Gate.ModelLicense] = a[6]
    };
    foreach (string folder in new[] { "content", "mods" })
    {
        string target = Path.Combine(package, folder);
        if (Directory.Exists(target)) Directory.Delete(target, recursive: true);
    }
    foreach (string root in new[] { Path.Combine(package, "content"), Path.Combine(package, "mods", Gate.OfflineModId) })
        foreach (var (relative, source) in sources)
        {
            string target = Path.Combine(root, relative);
            Directory.CreateDirectory(Path.GetDirectoryName(target)!);
            File.Copy(source, target, overwrite: false);
        }
    using var mod = JsonDocument.Parse(File.ReadAllBytes(a[7]));
    string manifestPath = Path.Combine(package, "package-manifest.json");
    var files = new JsonArray();
    foreach (string relative in RelativeFiles(package).Where(f => f != "package-manifest.json"))
    {
        byte[] body = File.ReadAllBytes(Path.Combine(package, relative));
        var entry = new JsonObject { ["bytes"] = body.Length, ["path"] = relative, ["sha256"] = Sha(body) };
        if (relative.EndsWith(Gate.OfflineModId + ".dll", StringComparison.Ordinal)) entry["source_commit"] = a[3];
        if (relative.EndsWith("sts2_advisor_bridge.dll", StringComparison.Ordinal)) entry["source_commit"] = a[4];
        files.Add(entry);
    }
    var manifest = new JsonObject {
        ["code_license"] = "AGPL-3.0-or-later",
        ["edition"] = "offline",
        ["files"] = files,
        ["game_dll_sha256"] = a[8],
        ["game_version"] = "0.1.0.0",
        ["mod_id"] = Gate.OfflineModId,
        ["policy_checkpoint"] = new JsonObject {
            ["cache_directory"] = "content/" + Gate.PolicyFolder, ["checkpoint_id"] = checkpoint.Id,
            ["model_license"] = "content/" + Gate.PolicyFolder + "/" + Gate.ModelLicense,
            ["step"] = checkpoint.Step, ["weights_sha256"] = checkpoint.Hash
        },
        ["schema"] = Gate.ReleaseSchema,
        ["version"] = mod.RootElement.GetProperty("version").GetString()
    };
    File.WriteAllText(manifestPath, manifest.ToJsonString(new JsonSerializerOptions { WriteIndented = true }).Replace("\r\n", "\n") + "\n");
    Console.WriteLine("PASS: offline package assembled from attested binaries and the verified bundled checkpoint.");
}

// The mod folder ships the code's license: the full GNU AGPL v3 text.
static void RequireAgpl(string text)
{
    if (!text.TrimStart().StartsWith("GNU AFFERO GENERAL PUBLIC LICENSE", StringComparison.Ordinal) ||
        !text.Contains("Version 3, 19 November 2007", StringComparison.Ordinal) ||
        !text.Contains("END OF TERMS AND CONDITIONS", StringComparison.Ordinal))
        throw new InvalidDataException(Gate.CodeLicense + " must be the full GNU AGPL v3 text");
}

static void CheckOfflinePackage(string packageArg, string managedCommit, string bridgeCommit)
{
    string root = Path.GetFullPath(packageArg);
    using var inventory = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(root, "package-manifest.json")));
    var manifest = inventory.RootElement;
    if (manifest.GetProperty("schema").GetString() != Gate.ReleaseSchema ||
        manifest.GetProperty("mod_id").GetString() != Gate.OfflineModId ||
        manifest.GetProperty("edition").GetString() != "offline")
        throw new InvalidDataException("not an offline edition package manifest");
    string gameHash = manifest.GetProperty("game_dll_sha256").GetString()!;
    string flat = Path.Combine(root, "content");
    string staged = Path.Combine(root, "mods", Gate.OfflineModId);
    if (Directory.GetFileSystemEntries(Path.Combine(root, "mods")).Length != 1)
        throw new InvalidDataException("mods must hold only the " + Gate.OfflineModId + " folder");
    var bundled = Checkpoint(Path.Combine(flat, Gate.PolicyFolder), gameHash);
    var stagedCheckpoint = Checkpoint(Path.Combine(staged, Gate.PolicyFolder), gameHash);
    if (stagedCheckpoint != bundled) throw new InvalidDataException("flat/staged checkpoint mismatch");
    string[] expected = OfflineModFiles(bundled.Hash).Order(StringComparer.Ordinal).ToArray();
    foreach (string folder in new[] { flat, staged })
        if (!RelativeFiles(folder).SequenceEqual(expected))
            throw new InvalidDataException($"{folder} must contain exactly: {string.Join(", ", expected)}");
    foreach (string relative in expected)
        if (!File.ReadAllBytes(Path.Combine(flat, relative)).AsSpan().SequenceEqual(File.ReadAllBytes(Path.Combine(staged, relative))))
            throw new InvalidDataException("flat/staged difference: " + relative);
    string dll = Path.Combine(staged, Gate.OfflineModId + ".dll");
    string bridge = Path.Combine(staged, "sts2_advisor_bridge.dll");
    Attest(dll, bridge, managedCommit, bridgeCommit);
    if (!NoNetwork(dll)) throw new InvalidDataException("offline DLL fails the no-network gate");
    InspectNative(bridge);
    NativeNetworkGate(bridge);
    using var mod = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(staged, Gate.OfflineModId + ".json")));
    if (mod.RootElement.GetProperty("id").GetString() != Gate.OfflineModId ||
        !mod.RootElement.GetProperty("has_dll").GetBoolean() || mod.RootElement.GetProperty("affects_gameplay").GetBoolean() ||
        mod.RootElement.GetProperty("version").GetString() != manifest.GetProperty("version").GetString())
        throw new InvalidDataException("offline mod manifest mismatch");
    RequireAgpl(File.ReadAllText(Path.Combine(staged, Gate.CodeLicense)));
    string license = File.ReadAllText(Path.Combine(staged, Gate.PolicyFolder, Gate.ModelLicense));
    if (!license.Contains("AGPL-3.0", StringComparison.Ordinal) || !license.Contains("redistribut", StringComparison.OrdinalIgnoreCase))
        throw new InvalidDataException("MODEL-LICENSE.md must state the weights' terms and the code license");
    var policy = manifest.GetProperty("policy_checkpoint");
    if (policy.GetProperty("cache_directory").GetString() != "content/" + Gate.PolicyFolder ||
        policy.GetProperty("checkpoint_id").GetString() != bundled.Id ||
        policy.GetProperty("step").GetUInt64() != bundled.Step ||
        policy.GetProperty("weights_sha256").GetString() != bundled.Hash)
        throw new InvalidDataException("package checkpoint inventory mismatch");
    var listed = new HashSet<string>(StringComparer.Ordinal);
    foreach (var entry in manifest.GetProperty("files").EnumerateArray())
    {
        string path = entry.GetProperty("path").GetString()!;
        listed.Add(path);
        byte[] body = File.ReadAllBytes(Path.Combine(root, path));
        if (entry.GetProperty("bytes").GetInt64() != body.Length || entry.GetProperty("sha256").GetString() != Sha(body))
            throw new InvalidDataException("package inventory mismatch: " + path);
        if (path.EndsWith(Gate.OfflineModId + ".dll", StringComparison.Ordinal) && entry.GetProperty("source_commit").GetString() != managedCommit ||
            path.EndsWith("sts2_advisor_bridge.dll", StringComparison.Ordinal) && entry.GetProperty("source_commit").GetString() != bridgeCommit)
            throw new InvalidDataException("package source commit mismatch: " + path);
    }
    var present = RelativeFiles(root).Where(f => f != "package-manifest.json").ToHashSet(StringComparer.Ordinal);
    if (!present.SetEquals(listed)) throw new InvalidDataException("package files and hash inventory differ");
    Console.WriteLine("PASS: offline package: release entry point, no-network gate, staged bytes, bundled checkpoint, model license and inventory.");
}

// Release gate for user-facing text: Writer placeholders (TODO_COPY) in the shipped mod manifest,
// MODEL-LICENSE.md, or the UI strings embedded in the DLL block a release.
static void FinalText(string packageArg)
{
    string staged = Path.Combine(Path.GetFullPath(packageArg), "mods", Gate.OfflineModId);
    var left = new List<string>();
    foreach (string relative in RelativeFiles(staged).Where(f => f.EndsWith(".json", StringComparison.Ordinal) || f.EndsWith(".md", StringComparison.Ordinal)))
        if (File.ReadAllText(Path.Combine(staged, relative)).Contains("TODO_COPY", StringComparison.Ordinal)) left.Add(relative);
    using var pe = new PEReader(new MemoryStream(File.ReadAllBytes(Path.Combine(staged, Gate.OfflineModId + ".dll"))));
    var metadata = pe.GetMetadataReader();
    foreach (var handle in metadata.ManifestResources)
    {
        var resource = metadata.GetManifestResource(handle);
        if (metadata.GetString(resource.Name) != "HumanPlayRecorder.ui_strings.json") continue;
        var directory = pe.PEHeaders.CorHeader!.ResourcesDirectory;
        var reader = pe.GetSectionData(directory.RelativeVirtualAddress).GetReader((int)resource.Offset, directory.Size - (int)resource.Offset);
        using var strings = JsonDocument.Parse(reader.ReadBytes(reader.ReadInt32()));
        foreach (var language in strings.RootElement.EnumerateObject())
            foreach (var entry in language.Value.EnumerateObject())
                if (entry.Value.GetString()?.Contains("TODO_COPY", StringComparison.Ordinal) == true)
                    left.Add($"ui_strings {language.Name}.{entry.Name}");
    }
    if (left.Count != 0)
        throw new InvalidDataException($"{left.Count} shipped texts are still TODO_COPY placeholders: {string.Join(", ", left.Take(8))}");
    Console.WriteLine("PASS: no TODO_COPY placeholder in the shipped mod manifest, model license or UI strings");
}

try
{
    if (args.Length == 2 && args[0] == "--final-text") { FinalText(args[1]); return 0; }
    if (args.Length == 4 && args[0] == "--zip-folder") { ZipFolder(args[1], args[2], args[3]); return 0; }
    if (args.Length == 2 && args[0] == "--offline-gate-inventory") { OfflineGate.Inventory(args[1], OfflineGate.GameDll()); return 0; }
    if (args.Length == 2 && args[0] == "--network-gate")
    {
        if (!NoNetwork(args[1])) throw new InvalidDataException("network access found in " + args[1]);
        Console.WriteLine("PASS: no network references: " + args[1]);
        return 0;
    }
    // A gate fixture build (tests/GateFixtures) must fail the gate, and only because of the
    // fixture's own method: every finding names it and each expected member is among them.
    if (args.Length >= 4 && args[0] == "--gate-fixture")
    {
        var findings = NetworkFindings(args[1]).Concat(OfflineGate.Findings(args[1], OfflineGate.GameDll())).ToList();
        foreach (string finding in findings) Console.WriteLine("fixture finding: " + finding);
        if (findings.Count == 0) throw new InvalidDataException("gate fixture passed the no-network gate: " + args[1]);
        if (findings.FirstOrDefault(f => !f.Contains(args[2] + " -> ", StringComparison.Ordinal)) is { } other)
            throw new InvalidDataException("gate fixture failed for another reason: " + other);
        foreach (string member in args[3..])
            if (!findings.Any(f => f.Contains(args[2] + " -> " + member, StringComparison.Ordinal)))
                throw new InvalidDataException($"gate fixture finding missing: {args[2]} -> {member}");
        Console.WriteLine($"PASS: no-network gate rejects {args[2]} ({string.Join(", ", args[3..])})");
        return 0;
    }
    if (args.Length == 2 && args[0] == "--network-gate-rejects")
    {
        if (NetworkFindings(args[1]).Count == 0) throw new InvalidDataException("negative control has no network findings");
        Console.WriteLine("PASS: no-network gate rejects the network-using control: " + args[1]);
        return 0;
    }
    if (args.Length == 2 && args[0] == "--native-network-gate") { NativeNetworkGate(args[1]); return 0; }
    if (args.Length == 3 && args[0] == "--checkpoint") { Checkpoint(args[1], args[2]); return 0; }
    if (args.Length == 11 && args[0] == "--assemble-offline") { AssembleOffline(args[1..]); return 0; }
    if (args.Length == 4 && args[0] == "--package-offline") { CheckOfflinePackage(args[1], args[2], args[3]); return 0; }
    if (args.Length == 3 && args[0] == "--checkpoint-rejected") {
        bool rejected = false;
        try { Checkpoint(args[1], args[2]); } catch (Exception) { rejected = true; }
        if (!rejected) throw new Exception("invalid bundled checkpoint unexpectedly accepted");
        Console.WriteLine("PASS: missing/corrupt/incompatible bundled checkpoint rejected.");
        return 0;
    }
    if (args.Length == 7 && args[0] == "--assemble") {
        Attest(args[2], args[3], args[4], args[5]);
        string package = Path.GetFullPath(args[1]);
        var builtInventory = JsonNode.Parse(File.ReadAllBytes(Path.Combine(package, "package-manifest.json")))!;
        builtInventory["published"] = false;
        var checkpoint = Checkpoint(args[6], builtInventory["game_dll_sha256"]!.GetValue<string>());
        foreach (var entry in builtInventory["files"]!.AsArray()) {
            string relative = entry!["path"]!.GetValue<string>();
            string name = Path.GetFileName(relative);
            if (name is not ("HumanPlayRecorder.dll" or "sts2_advisor_bridge.dll")) continue;
            string target = Path.Combine(package, relative);
            File.Copy(name == "HumanPlayRecorder.dll" ? args[2] : args[3], target, overwrite: true);
            byte[] body = File.ReadAllBytes(target);
            entry["bytes"] = body.Length;
            entry["sha256"] = Convert.ToHexStringLower(SHA256.HashData(body));
            entry["source_commit"] = name == "HumanPlayRecorder.dll" ? args[4] : args[5];
        }
        string cache = Path.Combine(package, "policy-cache");
        Directory.CreateDirectory(cache);
        foreach (string name in new[] { "current.json", checkpoint.Hash + ".safetensors" }) {
            string relative = "policy-cache/" + name;
            string target = Path.Combine(cache, name);
            File.Copy(Path.Combine(args[6], name), target, overwrite: true);
            byte[] body = File.ReadAllBytes(target);
            var files = builtInventory["files"]!.AsArray();
            var entry = files.FirstOrDefault(node => node!["path"]!.GetValue<string>() == relative);
            if (entry is null) { entry = new JsonObject { ["path"] = relative }; files.Add(entry); }
            entry["bytes"] = body.Length;
            entry["sha256"] = Convert.ToHexStringLower(SHA256.HashData(body));
        }
        builtInventory["policy_checkpoint"] = new JsonObject {
            ["cache_directory"] = "policy-cache", ["checkpoint_id"] = checkpoint.Id,
            ["step"] = checkpoint.Step, ["weights_sha256"] = checkpoint.Hash
        };
        File.WriteAllText(Path.Combine(package, "package-manifest.json"), builtInventory.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n");
        Console.WriteLine("PASS: source-attested binaries and validated offline checkpoint assembled locally.");
        return 0;
    }
    if (args.Length == 5 && args[0] == "--attestation-rejected") {
        bool rejected = false;
        try { Attest(args[1], args[2], args[3], args[4]); } catch (Exception) { rejected = true; }
        if (!rejected) throw new Exception("stale binaries unexpectedly accepted");
        Console.WriteLine("PASS: stale/mismatched binary attestation rejected.");
        return 0;
    }
    if (args.Length == 2 && args[0] == "--native")
    {
        InspectNative(args[1]);
        return 0;
    }
    if (args.Length > 1 && args[0] == "--inspect")
    {
        foreach (string path in args.Skip(1)) Inspect(path);
        return 0;
    }
    if (args.Length == 2 && args[0] == "--expect-rejected")
    {
        if (IsRelease(args[1])) throw new Exception("negative fixture unexpectedly accepted");
        Console.WriteLine("PASS: non-release artifact rejected without loading/executing it.");
        return 0;
    }
    if (args.Length == 2 && args[0] == "--expect-release")
    {
        if (!IsRelease(args[1])) throw new Exception("DLL is not a marked release entry point");
        Console.WriteLine("PASS: release DLL entry point, decision logging and embedded UI strings.");
        return 0;
    }
    if (args.Length != 4 || args[0] != "--package")
        throw new ArgumentException("usage: --package PACKAGE MANAGED_SOURCE_COMMIT BRIDGE_SOURCE_COMMIT | --assemble PACKAGE MANAGED_DLL BRIDGE_DLL MANAGED_COMMIT BRIDGE_COMMIT CHECKPOINT_CACHE");
    string root = Path.GetFullPath(args[1]);
    string flat = Path.Combine(root, "content", "HumanPlayRecorder.dll");
    string staged = Path.Combine(root, "steam_content", "HumanPlayRecorder", "HumanPlayRecorder.dll");
    if (!IsRelease(flat) || !IsRelease(staged)) throw new Exception("package is not the release entry point");
    if (!File.ReadAllBytes(flat).AsSpan().SequenceEqual(File.ReadAllBytes(staged)))
        throw new Exception("flat/staged DLL mismatch");
    string nativeFlat = Path.Combine(root, "content", "sts2_advisor_bridge.dll");
    string nativeStaged = Path.Combine(root, "steam_content", "HumanPlayRecorder", "sts2_advisor_bridge.dll");
    Attest(flat, nativeFlat, args[2], args[3]);
    Attest(staged, nativeStaged, args[2], args[3]);
    InspectNative(nativeFlat);
    InspectNative(nativeStaged);
    if (!File.ReadAllBytes(nativeFlat).AsSpan().SequenceEqual(File.ReadAllBytes(nativeStaged)))
        throw new Exception("flat/staged native advisor mismatch");
    string flatJSON = Path.Combine(root, "content", "HumanPlayRecorder.json");
    string stagedJSON = Path.Combine(root, "steam_content", "HumanPlayRecorder", "HumanPlayRecorder.json");
    if (!File.ReadAllBytes(flatJSON).AsSpan().SequenceEqual(File.ReadAllBytes(stagedJSON)))
        throw new Exception("flat/staged manifests differ");
    using var manifest = JsonDocument.Parse(File.ReadAllBytes(flatJSON));
    var mod = manifest.RootElement;
    if (mod.GetProperty("id").GetString() != "HumanPlayRecorder" ||
        mod.GetProperty("name").GetString() != "pqp AI Guide & Replay" ||
        !mod.GetProperty("has_dll").GetBoolean() || mod.GetProperty("affects_gameplay").GetBoolean())
        throw new Exception("package manifest is not the release manifest");
    using var inventory = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(root, "package-manifest.json")));
    var bundled = Checkpoint(Path.Combine(root, "policy-cache"),
        inventory.RootElement.GetProperty("game_dll_sha256").GetString()!);
    var policy = inventory.RootElement.GetProperty("policy_checkpoint");
    if (policy.GetProperty("cache_directory").GetString() != "policy-cache" ||
        policy.GetProperty("checkpoint_id").GetString() != bundled.Id ||
        policy.GetProperty("step").GetUInt64() != bundled.Step ||
        policy.GetProperty("weights_sha256").GetString() != bundled.Hash)
        throw new InvalidDataException("package checkpoint inventory mismatch");
    var inventoryPaths = inventory.RootElement.GetProperty("files").EnumerateArray()
        .Select(entry => entry.GetProperty("path").GetString()).ToHashSet(StringComparer.Ordinal);
    if (!inventoryPaths.Contains("policy-cache/current.json") ||
        !inventoryPaths.Contains("policy-cache/" + bundled.Hash + ".safetensors"))
        throw new InvalidDataException("checkpoint files missing from package hash inventory");
    foreach (var entry in inventory.RootElement.GetProperty("files").EnumerateArray())
    {
        byte[] body = File.ReadAllBytes(Path.Combine(root, entry.GetProperty("path").GetString()!));
        if (entry.GetProperty("bytes").GetInt64() != body.Length ||
            entry.GetProperty("sha256").GetString() != Convert.ToHexStringLower(SHA256.HashData(body)))
            throw new Exception("package inventory mismatch");
        string path = entry.GetProperty("path").GetString()!;
        if (path.EndsWith("HumanPlayRecorder.dll", StringComparison.Ordinal) && entry.GetProperty("source_commit").GetString() != args[2] ||
            path.EndsWith("sts2_advisor_bridge.dll", StringComparison.Ordinal) && entry.GetProperty("source_commit").GetString() != args[3])
            throw new Exception("package source commit mismatch");
    }
    Console.WriteLine("PASS: release entry point, decision log, marker, manifest, staged bytes and inventory.");
    return 0;
}
catch (Exception error)
{
    Console.Error.WriteLine("FAIL: " + error.Message);
    return 1;
}
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
delegate IntPtr SourceCommitDelegate();

static class Gate
{
    internal const string OfflineModId = "PqpAiGuideOffline";
    internal const string PolicyFolder = "policy";
    internal const string ModelLicense = "MODEL-LICENSE.md";
    internal const string CodeLicense = "LICENSE";
    // release_offline/package-manifest.json, the GitHub release input (the offline edition is not on the Workshop).
    internal const string ReleaseSchema = "pqp-ai-guide-offline-release-v1";
    internal static readonly HashSet<string> NetworkTypes = new(StringComparer.Ordinal) {
        "HttpClient", "HttpClientHandler", "SocketsHttpHandler", "HttpMessageHandler", "WebRequest",
        "HttpWebRequest", "WebClient", "Socket", "TcpClient", "TcpListener", "UdpClient", "Dns",
        "ClientWebSocket", "WebSocket", "SslStream", "NetworkStream", "Ping", "HttpListener",
        // Godot's own network classes.
        "HttpRequest", "StreamPeerTcp", "StreamPeerTls", "PacketPeerUdp", "TcpServer",
        "WebSocketPeer", "UdpServer", "ENetConnection", "WebRtcPeerConnection"
    };
    internal static readonly HashSet<string> NetworkLibraries = new(StringComparer.Ordinal) {
        "ws2_32", "wsock32", "mswsock", "winhttp", "wininet", "urlmon", "dnsapi", "iphlpapi",
        "websocket", "httpapi", "netapi32", "fwpuclnt", "rasapi32", "wlanapi", "webio"
    };
    // License or attribution text may name a URL; the offline edition currently has none.
    internal static readonly HashSet<string> LicenseUrls = new(StringComparer.Ordinal);
}
