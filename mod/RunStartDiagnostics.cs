using System.Reflection;

namespace HumanPlayRecorder;

public static class RunStartDiagnostics
{
    public static string Failure(Exception error) => "skipped: initialization failed " + Describe(error);

    /// <summary>Exception type, audited-constant message and throwing method; never a raw message.</summary>
    public static string Describe(Exception error)
    {
        while (error is TargetInvocationException { InnerException: { } inner }) error = inner;
        // Exception.Message can contain account paths, Steam IDs, or payloads.
        // Only verbatim messages proven constant in the pinned source are safe.
        string message = error.Message switch {
            "active mod set is not replay-supported" => error.Message,
            "State is already set." => error.Message,
            "State is not set." => error.Message,
            "Local player not found in player collection." => error.Message,
            "queue_full" => error.Message,
            _ => "[redacted: exception message may contain private data]"
        };
        var method = error.TargetSite;
        string site = method is null ? "unknown" : method.DeclaringType?.FullName + "." + method.Name;
        site = site.Replace('\r', '_').Replace('\n', '_');
        if (site.Length > 256) site = site[..256];
        return $"({error.GetType().Name}); message={message}; method={site}";
    }
}

