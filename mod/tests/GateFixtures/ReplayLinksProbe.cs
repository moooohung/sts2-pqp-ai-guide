using System.Reflection;

namespace HumanPlayRecorder.GateFixtures;

// Gate fixture (never shipped): the review's bypass probe. It loads the online edition's assembly
// and opens a replay link through reflection; the offline no-network gate must reject it.
static class ReplayLinksProbe
{
    internal static void Run() =>
        Assembly.Load("HumanPlayRecorder").GetType("HumanPlayRecorder.ReplayLinks", true)!
            .GetMethod("Open", BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)!
            .Invoke(null, null);
}
