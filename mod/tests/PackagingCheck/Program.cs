using System.Reflection.Metadata;
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

static (string[] Initializers, bool DecisionLog, bool ReleaseMarker, bool Localization, string? SourceCommit, bool Advisor) Inspect(string path)
{
    byte[] bytes = File.ReadAllBytes(path);
    using var pe = new PEReader(new MemoryStream(bytes));
    var metadata = pe.GetMetadataReader();
    var initializers = new List<string>();
    bool releaseMarker = false;
    string? sourceCommit = null;
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
    bool decisionLog = bytes.AsSpan().IndexOf(Encoding.Unicode.GetBytes("[HumanPlayRecorder] run start skipped:")) >= 0;
    bool localization = HasLocalization(pe, metadata);
    var typeNames = metadata.TypeDefinitions.Select(h => metadata.GetString(metadata.GetTypeDefinition(h).Name)).ToHashSet();
    var methodNames = metadata.MethodDefinitions.Select(h => metadata.GetString(metadata.GetMethodDefinition(h).Name)).ToHashSet();
    bool advisor = typeNames.Contains("ScreenRows") && methodNames.Contains("RememberSelection") && methodNames.Contains("get_Legal");
    Console.WriteLine($"{path}: bytes={bytes.Length} sha256={Convert.ToHexStringLower(SHA256.HashData(bytes))} initializers=[{string.Join(",", initializers)}] decision_log={decisionLog} release_marker={releaseMarker} localization={localization} source_commit={sourceCommit} advisor={advisor}");
    return (initializers.ToArray(), decisionLog, releaseMarker, localization, sourceCommit, advisor);
}

static bool IsRelease(string path)
{
    var found = Inspect(path);
    return found.Initializers.SequenceEqual(new[] { "HumanPlayRecorder.ReleaseBootstrap.Init" }) &&
        found.Localization &&
        found.DecisionLog && found.ReleaseMarker && found.Advisor && ValidCommit(found.SourceCommit);
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
    Console.WriteLine($"PASS: bundled checkpoint {id} step={step} sha256={hash}");
    return (hash, id, step);
}

try
{
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
