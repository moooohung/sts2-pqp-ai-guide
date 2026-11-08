using System.Runtime.CompilerServices;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static T? CaptureBoundary<T>(Func<T?> capture, [CallerMemberName] string boundary = "") where T : class
    {
        try { return capture(); }
        catch (Exception error) { CaptureBoundaryFault(error, boundary); return null; }
    }

    static void CaptureBoundary(Action capture, [CallerMemberName] string boundary = "")
    {
        try { capture(); }
        catch (Exception error) { CaptureBoundaryFault(error, boundary); }
    }

    static void CaptureBoundaryFault(Exception error, string boundary)
    {
        try { journal?.Invalidate(); } catch (Exception) { }
        try { SwitchAdvisorToLive(); } catch (Exception) { }
        NoteSwallowed("capture boundary " + boundary, error);
        try { Console.Error.WriteLine("[HumanPlayRecorder] capture boundary failed: " +
            boundary + ": " + error.GetType().Name); } catch (Exception) { }
    }

    // A failed checkpoint never reaches the game: the advisor falls back to live mode. Every such fallback is
    // reported here, so the log and the verification harness see the same failures (the last few are kept).
    static readonly List<string> swallowedFailures = [];
    internal static string[] SwallowedFailures { get { lock (swallowedFailures) return [.. swallowedFailures]; } }
    internal static void ClearSwallowedFailures() { lock (swallowedFailures) swallowedFailures.Clear(); }
    static void NoteSwallowed(string where, Exception error)
    {
        lock (swallowedFailures) {
            if (swallowedFailures.Count >= 32) swallowedFailures.RemoveAt(0);
            swallowedFailures.Add(where + ": " + error.GetType().Name);
        }
    }
    internal static void SwallowedCheckpointFailure(string where, Exception error)
    {
        NoteSwallowed(where, error);
        try { Console.Error.WriteLine("[HumanPlayRecorder] checkpoint failure: " + RunStartDiagnostics.Failure(error) + "; at=" + where); }
        catch (Exception) { }
    }
}
