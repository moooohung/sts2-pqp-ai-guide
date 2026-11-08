using Godot;

namespace HumanPlayRecorder.Settings;

// The advice UI runs inside the game's own screen code (Harmony patches, signals, game and
// advisor events). Every such entry point goes through here: a failure is logged once per
// stage and never reaches the game.
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
                if (!logged.Add(stage)) return;
            GD.Print($"[HumanPlayRecorder] AI advice UI {stage} failed: {e.GetType().Name}: {e.Message}");
        }
        catch (Exception) { } // recovery must not throw; a lost log line is the only cost
    }
}
