using Godot;

namespace HumanPlayRecorder.Settings;

// The advice UI runs inside the game's own screen code (Harmony patches, signals, game and
// advisor events). Every such entry point goes through here: a failure never reaches the game and
// is written to godot.log in every build with its full exception and stack, once per stage and
// distinct failure (a different exception at the same stage is logged too).
static class UiGuard
{
    static readonly HashSet<string> logged = new();

    internal static void Run(string stage, Action action)
    {
        try { action(); }
        catch (Exception e) { Report(stage, e); }
    }

    internal static T Run<T>(string stage, Func<T> action, T fallback)
    {
        try { return action(); }
        catch (Exception e) { Report(stage, e); return fallback; }
    }

    internal static void Report(string stage, Exception e)
    {
        try
        {
            lock (logged)
                if (!logged.Add(stage + "\n" + e.GetType().FullName + "\n" + e.Message)) return;
            GD.Print($"[HumanPlayRecorder] ERROR: UI {stage} failed: {e}");
        }
        catch (Exception) { } // recovery must not throw; a lost log line is the only cost
    }
}
