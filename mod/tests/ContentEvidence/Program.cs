using HumanPlayRecorder;
using HumanPlayRecorder.Ai;
using System.Reflection;

var render = typeof(RecorderMod).Assembly.GetType("HumanPlayRecorder.Settings.AdvisorStatusText")!
    .GetMethod("Render", BindingFlags.Static | BindingFlags.NonPublic)!;
string Render(AdvisorStatus status, AiFailureKind failure, string? refusal) =>
    (string)render.Invoke(null, [true, status, failure, refusal, null,
        (Func<string, string>)(key => key), (Func<string, string, string>)((key, value) => key + value)])!;
if (Render(AdvisorStatus.Unavailable, AiFailureKind.None, null) != "" ||
    Render(AdvisorStatus.Unavailable, AiFailureKind.Offline, "NETWORK") != "ai_status_unavailable_offline" ||
    Render(AdvisorStatus.Unavailable, AiFailureKind.Internal, "PRIVATE_DETAIL") != "ai_status_unavailable")
    throw new Exception("idle must be empty, but real failures must remain visible without internal details");
Console.WriteLine("PASS: idle status is empty; offline and internal failures retain their public labels (3 assertions)");

string directory = Path.Combine(Path.GetTempPath(), "hpr-content-evidence-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(directory);
try
{
    string path = Path.Combine(directory, "evidence.json");
    var firstSession = new RememberedContentMods(path);
    firstSession.Remember("Watcher");
    firstSession.Remember("Watcher");
    firstSession.Remember("TheQueen");
    var nextSession = new RememberedContentMods(path);
    if (!nextSession.Offer(["SharedCards"], ["Watcher", "TheQueen", "SharedCards", "Cosmetic"])
            .SequenceEqual(["SharedCards", "TheQueen", "Watcher"]))
        throw new Exception("next startup must offer excluded character mods even when their pool scans are empty");
    if (!nextSession.Offer([], ["Cosmetic"]).SequenceEqual(Array.Empty<string>()))
        throw new Exception("unloaded remembered mods must not be offered");
    if (!nextSession.Read().SequenceEqual(["TheQueen", "Watcher"]))
        throw new Exception("repeat exclusions must retain one persistent entry");
    Console.WriteLine("PASS: remembered exclusions survive restart, merge scan, ignore unloaded mods, deduplicate (3 assertions)");
}
finally { Directory.Delete(directory, recursive: true); }
