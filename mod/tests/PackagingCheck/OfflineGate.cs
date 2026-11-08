using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;
using System.Text.RegularExpressions;

// Allowlist half of the offline no-network gate (metadata only; nothing is loaded or run).
//
// Boundary: the offline edition makes no network connection of its own and sends nothing to any
// host, service or URL. The one exception is the game's own multiplayer session: AI teammates
// pass game messages through the game's multiplayer service (INetGameService / NetHostGameService
// / its NetHost) to the players in the same game session, over the peer connection the player
// opened by joining or hosting. Those sends are allowed only from the reviewed call sites in
// OfflineAllowlist.Sites; a send from any other method fails the gate.
//
// The gate fails on:
// - an assembly reference outside OfflineAllowlist.Assemblies, a native import other than the
//   bridge, or a referenced namespace outside OfflineAllowlist.Namespaces;
// - a sensitive member (Sensitive below: reflection loading and invocation, native loading,
//   process and browser launch, Godot dynamic calls, game networking, a string literal naming
//   a game send method, or a UNC path literal) used from a (caller, member) pair not in OfflineAllowlist.Sites, or used
//   a different number of times than listed there, or a listed site that no longer exists;
// - a game method the mod calls that itself calls a network, Steam, browser or process API.
static class OfflineGate
{
    static readonly Regex compilerName = new(@"^<+([^<>]+)>", RegexOptions.CultureInvariant);

    // Lambdas, local functions, iterators and async state machines count as the method that
    // declares them, so the allowlist names source methods, not compiler-generated ones.
    internal static string Caller(string generated)
    {
        int split = generated.IndexOf("::", StringComparison.Ordinal);
        string type = generated[..split], method = generated[(split + 2)..];
        string? machine = null;
        var kept = new List<string>();
        foreach (string part in type.Split('+'))
        {
            if (!part.StartsWith('<')) { kept.Add(part); continue; }
            if (part.Contains(">d__", StringComparison.Ordinal) && compilerName.Match(part) is { Success: true } m)
                machine = m.Groups[1].Value;
        }
        if (machine is not null && method is "MoveNext" or "SetStateMachine") method = machine;
        else if (compilerName.Match(method) is { Success: true } m) method = m.Groups[1].Value;
        return string.Join("+", kept) + "::" + method;
    }

    static readonly HashSet<string> reflectionLoad = new(StringComparer.Ordinal) {
        "Load", "LoadFrom", "LoadFile", "UnsafeLoadFrom", "LoadWithPartialName", "ReflectionOnlyLoad",
        "ReflectionOnlyLoadFrom", "CreateInstance", "GetType", "GetTypes", "GetExportedTypes", "get_DefinedTypes",
        "get_ExportedTypes", "GetEntryAssembly", "GetCallingAssembly", "GetForwardedTypes", "GetModules",
        "GetLoadedModules", "GetModule", "LoadModule"
    };
    static readonly HashSet<string> godotDynamic = new(StringComparer.Ordinal) {
        "Call", "Callv", "CallDeferred", "CallDeferredv", "Set", "SetDeferred", "Get", "SetScript",
        "Rpc", "RpcId", "EmitSignal", "SetIndexed", "GetIndexed"
    };

    // Why a member needs a reviewed call site, or null when any method may use it.
    internal static string? Sensitive(string member)
    {
        if (member == "calli") return "unmanaged indirect call";
        if (member.StartsWith('"'))
            return Regex.IsMatch(member, @"^""(Send|Broadcast)\w*""$") ? "game send method named by string" :
                member.StartsWith(@"""\\", StringComparison.Ordinal) || member.StartsWith("\"//", StringComparison.Ordinal) ? "UNC network path" : null;
        int split = member.IndexOf("::", StringComparison.Ordinal);
        if (split < 0) return null;
        string type = member[..split], name = member[(split + 2)..];
        bool In(string space) => type == space || type.StartsWith(space + ".", StringComparison.Ordinal) ||
            type.StartsWith(space + "+", StringComparison.Ordinal);
        if (In("System.Net") || In("System.Reflection.Emit") || In("System.Runtime.Loader") || In("System.CodeDom") ||
            type is "System.Activator" or "System.AppDomain" or "System.Diagnostics.Process" or
                "System.Diagnostics.ProcessStartInfo" or "System.Runtime.InteropServices.NativeLibrary" ||
            In("HarmonyLib.Traverse") || In("HarmonyLib.FastAccess") || In("HarmonyLib.MethodInvoker") ||
            type is "Godot.OS" or "Godot.ClassDB" or "Godot.Expression" or "Godot.JavaScriptBridge")
            return "banned API " + type;
        switch (type)
        {
            case "System.Reflection.Assembly" or "System.Reflection.Module" or "System.Reflection.TypeInfo"
                when reflectionLoad.Contains(name) || name.StartsWith("Resolve", StringComparison.Ordinal):
                return "reflection type or assembly lookup";
            case "System.Reflection.MethodBase" or "System.Reflection.MethodInfo" or "System.Reflection.ConstructorInfo"
                when name is "Invoke" or "CreateDelegate":
                return "reflection invocation";
            case "System.Reflection.PropertyInfo" when name is "GetValue" or "SetValue":
                return "reflection accessor invocation";
            case "System.Reflection.EventInfo" when name is "AddEventHandler" or "RemoveEventHandler" or "GetRaiseMethod":
                return "reflection event invocation";
            case "System.Type" when name is "GetType" or "GetTypeFromProgID" or "GetTypeFromCLSID" or "InvokeMember" or
                "ReflectionOnlyGetType":
                return "type lookup by name";
            case "System.Delegate" or "System.MulticastDelegate" when name is "CreateDelegate" or "DynamicInvoke":
                return "reflection delegate";
            case "System.Linq.Expressions.Expression" or "System.Linq.Expressions.LambdaExpression"
                or "System.Linq.Expressions.Expression`1" when name is "Compile" or "CompileToMethod":
                return "compiled expression";
            case "System.Runtime.InteropServices.Marshal" when Regex.IsMatch(name,
                "^(GetDelegateForFunctionPointer|GetObject|GetActiveObject|BindToMoniker|GetTypeFrom|CreateWrapper|GetComInterface|QueryInterface|GetIDispatch|GetIUnknown|GetTypedObject)"):
                return "native or COM invocation";
            case "HarmonyLib.AccessTools" when name is "TypeByName" or "AllTypes" or "AllAssemblies" or
                "GetTypesFromAssembly" or "CreateInstance" or "MethodDelegate" or "HarmonyDelegate" or "TypeSearch":
                return "Harmony lookup or invocation by name";
            case "Godot.Engine" when name.StartsWith("GetSingleton", StringComparison.Ordinal):
                return "Godot singleton by name";
            case "Godot.GD" or "Godot.ResourceLoader" when name.StartsWith("Load", StringComparison.Ordinal):
                return "Godot resource load";
            case "Godot.Callable" when name is ".ctor" or "Call" or "Callv" or "CallDeferred" or "CallDeferredv":
                return "Godot dynamic call";
        }
        if (type.StartsWith("Godot.", StringComparison.Ordinal) && godotDynamic.Contains(name))
            return "Godot dynamic call";
        if (type.StartsWith("MegaCrit.Sts2.Core.Platform", StringComparison.Ordinal))
            return "game platform service";
        if (type.StartsWith("MegaCrit.Sts2.Core.Multiplayer", StringComparison.Ordinal))
        {
            if (!name.StartsWith("get_", StringComparison.Ordinal) &&
                (name.Contains("Send", StringComparison.Ordinal) || name.Contains("Broadcast", StringComparison.Ordinal)))
                return "game multiplayer send";
            if ((type.StartsWith("MegaCrit.Sts2.Core.Multiplayer.Transport", StringComparison.Ordinal) ||
                 type.StartsWith("MegaCrit.Sts2.Core.Multiplayer.Connection", StringComparison.Ordinal)) &&
                !name.StartsWith("get_", StringComparison.Ordinal) && type != "MegaCrit.Sts2.Core.Multiplayer.Transport.NetTransferModeExtensions")
                return "game transport";
        }
        return null;
    }

    // Game methods that themselves call a network, Steam, browser or process API: calling one from
    // the mod would reach that API one step removed.
    static readonly Dictionary<string, HashSet<string>> gameWrappers = new(StringComparer.OrdinalIgnoreCase);
    static HashSet<string> GameWrappers(string gameDll)
    {
        string key = Path.GetFullPath(gameDll);
        if (gameWrappers.TryGetValue(key, out var known)) return known;
        using var pe = new PEReader(File.OpenRead(key));
        var md = pe.GetMetadataReader();
        var wrappers = new HashSet<string>(StringComparer.Ordinal);
        foreach (var reference in IlScan.References(pe, md))
        {
            string member = reference.Member;
            if (member.StartsWith("System.Net", StringComparison.Ordinal) || member.StartsWith("Steamworks.", StringComparison.Ordinal) ||
                member.StartsWith("Godot.OS::ShellOpen", StringComparison.Ordinal) ||
                member.StartsWith("System.Diagnostics.Process::Start", StringComparison.Ordinal) ||
                Gate.NetworkTypes.Contains(member.Split("::")[0].Split('.')[^1]))
                wrappers.Add(reference.Caller);
        }
        return gameWrappers[key] = wrappers;
    }

    // The sensitive (caller, member) uses in an assembly, grouped and counted.
    internal static Dictionary<(string Caller, string Member), int> SensitiveSites(PEReader pe, MetadataReader md) =>
        IlScan.References(pe, md).Where(r => Sensitive(r.Member) is not null)
            .GroupBy(r => (Caller(r.Caller), r.Member)).ToDictionary(g => g.Key, g => g.Count());

    internal static SortedSet<string> Namespaces(MetadataReader md)
    {
        var found = new SortedSet<string>(StringComparer.Ordinal);
        foreach (var handle in md.TypeReferences)
        {
            var type = md.GetTypeReference(handle);
            if (type.ResolutionScope.Kind != HandleKind.AssemblyReference) continue;
            string scope = md.GetString(md.GetAssemblyReference((AssemblyReferenceHandle)type.ResolutionScope).Name);
            found.Add(scope + ": " + md.GetString(type.Namespace));
        }
        return found;
    }

    internal static List<string> Findings(string path, string gameDll)
    {
        var findings = new List<string>();
        using var pe = new PEReader(File.OpenRead(path));
        var md = pe.GetMetadataReader();
        foreach (var handle in md.AssemblyReferences)
        {
            string name = md.GetString(md.GetAssemblyReference(handle).Name);
            if (!OfflineAllowlist.Assemblies.Contains(name)) findings.Add("assembly reference not allowlisted: " + name);
        }
        for (int row = 1; row <= md.GetTableRowCount(TableIndex.ModuleRef); row++)
        {
            string name = md.GetString(md.GetModuleReference(MetadataTokens.ModuleReferenceHandle(row)).Name);
            if (name != OfflineAllowlist.NativeBridge) findings.Add("native import not allowlisted: " + name);
        }
        foreach (string space in Namespaces(md))
            if (!OfflineAllowlist.Namespaces.Any(allowed => allowed == space ||
                    allowed.EndsWith(".*", StringComparison.Ordinal) && space.StartsWith(allowed[..^1], StringComparison.Ordinal)))
                findings.Add("namespace not allowlisted: " + space);

        var sites = SensitiveSites(pe, md);
        foreach (var ((caller, member), count) in sites.OrderBy(s => s.Key.Caller, StringComparer.Ordinal))
        {
            if (!OfflineAllowlist.Sites.TryGetValue((caller, member), out int allowed))
                findings.Add($"{Sensitive(member)} from an unlisted call site: {caller} -> {member} x{count}");
            else if (allowed != count)
                findings.Add($"{Sensitive(member)} at {caller} -> {member}: {count} uses, the reviewed allowlist has {allowed}");
        }
        foreach (var ((caller, member), allowed) in OfflineAllowlist.Sites)
            if (!sites.ContainsKey((caller, member)))
                findings.Add($"allowlisted site no longer exists (remove it from OfflineAllowlist.Sites): {caller} -> {member} x{allowed}");

        var wrappers = GameWrappers(gameDll);
        foreach (var reference in IlScan.References(pe, md))
            if (wrappers.Contains(reference.Member) && !OfflineAllowlist.GameWrappers.Contains(reference.Member))
                findings.Add($"calls a game method that uses a network, Steam, browser or process API: {Caller(reference.Caller)} -> {reference.Member}");
        return findings.Distinct().ToList();
    }

    // sts2.dll for the wrapper scan: the build's game data folder (STS2_GAME_DATA, set by the
    // mod build and the release script).
    internal static string GameDll()
    {
        string data = Environment.GetEnvironmentVariable("STS2_GAME_DATA") is { Length: > 0 } set ? set
            : throw new InvalidOperationException("the offline gate needs STS2_GAME_DATA: the game's data_sts2_windows_x86_64 folder");
        string dll = Path.Combine(data, "sts2.dll");
        if (!File.Exists(dll)) throw new FileNotFoundException("the offline gate needs the game's sts2.dll (set STS2_GAME_DATA)", dll);
        return dll;
    }

    // Prints what the allowlist would have to hold for this assembly (for review, never applied).
    internal static void Inventory(string path, string gameDll)
    {
        using var pe = new PEReader(File.OpenRead(path));
        var md = pe.GetMetadataReader();
        foreach (var handle in md.AssemblyReferences) Console.WriteLine("assembly " + md.GetString(md.GetAssemblyReference(handle).Name));
        foreach (string space in Namespaces(md)) Console.WriteLine("namespace " + space);
        foreach (var ((caller, member), count) in SensitiveSites(pe, md).OrderBy(s => s.Key.Member, StringComparer.Ordinal).ThenBy(s => s.Key.Caller, StringComparer.Ordinal))
            Console.WriteLine($"site [{Sensitive(member)}] {caller} -> {member} x{count}");
        var wrappers = GameWrappers(gameDll);
        foreach (string member in IlScan.References(pe, md).Select(r => r.Member).Where(wrappers.Contains).Distinct())
            Console.WriteLine("game wrapper " + member);
    }
}
