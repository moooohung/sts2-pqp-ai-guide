using HumanPlayRecorder.Recommend;

static class ReasonTipTests
{
    static void Require(bool value, string reason) { if (!value) throw new Exception(reason); }

    public static void Run()
    {
        var log = new List<string>();
        var tip = new ReasonTip(text => { log.Add("show:" + text); return true; }, () => log.Add("hide"));

        // The reason is raised only while hovered, once per hover, and removed the frame the pointer leaves.
        tip.Update("why", false);
        Require(log.Count == 0, "a tip appeared without a hover");
        tip.Update("why", true); tip.Update("why", true);
        Require(log.SequenceEqual(["show:why"]), "a hover must show the tip exactly once: " + string.Join(",", log));
        tip.Update("why", false);
        Require(log.SequenceEqual(["show:why", "hide"]), "leaving did not remove the tip");

        // A changed text (language) replaces the tip; a hidden badge or a percentage (no reason) removes it.
        log.Clear();
        tip.Update("a", true); tip.Update("b", true);
        Require(log.SequenceEqual(["show:a", "hide", "show:b"]), "a new text must replace the old tip: " + string.Join(",", log));
        log.Clear();
        tip.Update(null, true);
        Require(log.SequenceEqual(["hide"]), "a badge without a reason kept its tip");
        tip.Update("c", true);
        log.Clear();
        tip.Release(); tip.Release();
        Require(log.SequenceEqual(["hide"]), "release must remove the tip once");

        // A tip the game blocks (show returns false) is not marked shown: it is retried every frame of the hover,
        // shown once it can be, and only then removed on leaving.
        log.Clear();
        bool blocked = true;
        var retried = new ReasonTip(text => { log.Add("try:" + text); return !blocked; }, () => log.Add("hide"));
        retried.Update("why", true); retried.Update("why", true);
        Require(log.SequenceEqual(["try:why", "try:why"]) && !retried.Shown, "a blocked tip was not retried: " + string.Join(",", log));
        retried.Update("why", false);
        Require(!log.Contains("hide"), "a tip that never appeared must not be removed");
        blocked = false; log.Clear();
        retried.Update("why", true); retried.Update("why", true);
        Require(log.SequenceEqual(["try:why"]) && retried.Shown, "the tip was not shown once unblocked");
        retried.Update("why", false);
        Require(log.SequenceEqual(["try:why", "hide"]), "the shown tip was not removed on leaving");

        // Only the option itself or something inside it counts as hovered, never an overlay above it.
        var parents = new Dictionary<string, string?> { ["root"] = null, ["option"] = "root", ["child"] = "option", ["modal"] = "root" };
        string? Parent(string node) => parents[node];
        Require(ReasonTip.Within("option", "option", Parent) && ReasonTip.Within("child", "option", Parent),
            "the option and its children are the option");
        Require(!ReasonTip.Within("modal", "option", Parent) && !ReasonTip.Within("root", "option", Parent) &&
            !ReasonTip.Within(null, "option", Parent), "an unrelated overlay raised the tip");

        // The tip stays inside the viewport, above the badge (below for a badge under its option).
        ReasonTip.Box B(float l, float t, float r, float b) => new(l, t, r, b);
        var none = Array.Empty<ReasonTip.Box>();
        var (x, y) = ReasonTip.Place(B(100, 300, 200, 330), 360, 80, 1920, 1080, false, none)!.Value;
        Require(x == 0 && y == 300 - ReasonTip.Gap - 80, "tip not centred above the badge, clamped left");
        (x, y) = ReasonTip.Place(B(1800, 10, 1900, 40), 360, 80, 1920, 1080, false, none)!.Value;
        Require(x == 1920 - 360 && y == 0, "tip left the viewport at the top right");
        (x, y) = ReasonTip.Place(B(900, 500, 1000, 530), 360, 80, 1920, 1080, true, none)!.Value;
        Require(x == 770 && y == 530 + ReasonTip.Gap, "tip not centred below the badge");

        // It never intersects the game's own tips, wherever they are, near the top of the viewport included.
        var badge = B(900, 20, 1000, 50);
        ReasonTip.Box[][] layouts =
        [
            [B(700, 0, 1060, 140)],                                 // the game's tip straight above the badge
            [B(700, 0, 1060, 140), B(1000, 60, 1360, 400)],         // and one to the right
            [B(1010, 10, 1370, 300), B(540, 10, 900, 300)],         // tips on both sides
            [B(0, 0, 1920, 300)],                                   // a full-width band of tips
        ];
        foreach (var obstacles in layouts)
        {
            var at = ReasonTip.Place(badge, 360, 80, 1920, 1080, false, obstacles);
            Require(at is not null, "no free spot though the viewport has room");
            var tip360 = B(at!.Value.X, at.Value.Y, at.Value.X + 360, at.Value.Y + 80);
            Require(!obstacles.Any(tip360.Intersects) && tip360.Left >= 0 && tip360.Top >= 0 && tip360.Right <= 1920 && tip360.Bottom <= 1080,
                "the reason intersects a game tip or leaves the viewport: " + tip360);
        }
        Require(ReasonTip.Place(badge, 360, 80, 1920, 1080, false, [B(0, 0, 1920, 1080)]) is null, "an occupied viewport must show nothing");
        Console.WriteLine("PASS: unavailable-badge reason tip follows hover without a mouse target; placed inside the viewport");
    }
}
