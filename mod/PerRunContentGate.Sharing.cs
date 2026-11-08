namespace HumanPlayRecorder;

public sealed partial class PerRunContentGate
{
    static partial void RememberOwner(string id) => StartupConsent.RememberContentMod(id);
}
