using Godot;
using MegaCrit.Sts2.Core.Entities.Multiplayer;
using MegaCrit.Sts2.Core.Map;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer.Game.PeerInput;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Flavor;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Sync;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder.Ai;

internal sealed class AiTeammatePresence(Action<INetMessage> submit, Func<double> cosmeticRandom)
{
    AiAdvisor.LiveOption? decision, runnerUp;
    object? room;
    long started, duration, nextUpdate;
    bool restHovered, pinged, hidden;
    Vector2? position, origin;
    public bool Active => decision is not null;
    public NetScreenType Screen { get; private set; }
    internal bool Ready(long now) => Active && now - started >= duration;

    internal bool Think(object currentRoom, AiAdvisor.LiveOption chosen, AiAdvisor.LiveOption? alternative,
        float confidence, NetScreenType screen, RunLocation location, Vector2? from, Vector2? destination,
        long now, long frequency, long readyBy = long.MaxValue)
    {
        if (!ReferenceEquals(room, currentRoom) || decision is null ||
            decision.Binding != chosen.Binding || !ReferenceEquals(decision.Option, chosen.Option) ||
            !ReferenceEquals(decision.Target, chosen.Target) || Screen != screen) {
            Clear(screen, location, now, frequency);
            room = currentRoom;
            decision = chosen;
            // Cosmetic randomness is deliberately separate from every game RNG.
            runnerUp = confidence < .7f && cosmeticRandom() < .45 ? alternative : null;
            started = now;
            // 350–1600 ms, clamped below to the seat's policy fallback deadline (readyBy).
            duration = (long)(frequency * (.35 + 1 - Math.Clamp(confidence, 0, 1) + .25 * cosmeticRandom()));
            origin = screen == Screen ? position ?? from : from;
            Screen = screen;
            nextUpdate = now;
            pinged = false;
        }
        // Cosmetic think time never outlasts the seat's fallback deadline (advice may arrive late).
        duration = Math.Min(duration, Math.Max(0, readyBy - started));
        if (hidden) { nextUpdate = now; hidden = false; }
        double progress = duration > 0 ? Math.Clamp((double)(now - started) / duration, 0, 1) : 1;
        if (now >= nextUpdate) {
            nextUpdate = now + frequency / 20;
            var hover = progress < .35 && runnerUp is not null ? runnerUp : chosen;
            bool targeting = ReferenceEquals(hover, chosen) && chosen.Target is not null &&
                chosen.Action is AiAdvisor.DecisionAction.PlayCard or AiAdvisor.DecisionAction.UsePotion;
            float motion = (float)(progress * progress * (3 - 2 * progress));
            position = origin is { } start && destination is { } end ? start.Lerp(end, motion) : destination ?? origin;
            submit(Input(hover, screen, position, targeting));
            if (chosen.Action == AiAdvisor.DecisionAction.RestOption && !restHovered) {
                var input = System.Text.Json.JsonSerializer.Deserialize<RecordedAction>(chosen.Binding, RecordingFormat.Json)!;
                submit(new RestSiteOptionHoveredMessage { optionIndex = (uint)input.index!.Value, Location = location });
                restHovered = true;
            }
            if (!pinged && progress >= .7 && chosen.Action == AiAdvisor.DecisionAction.Move &&
                chosen.Option is MapPoint point) {
                submit(new MapPingMessage { coord = point.coord });
                pinged = true;
            }
        }
        return Ready(now);
    }

    internal static PeerInputMessage Input(AiAdvisor.LiveOption? hover, NetScreenType screen,
        Vector2? position, bool targeting) => new() {
            screenType = screen, netMousePos = position, isTargeting = targeting,
            hoveredModelData = HoveredModelData.FromModel(hover?.Option is CardModel { Pile: { IsCombatPile: true } } or PotionModel or RelicModel
                ? (AbstractModel)hover.Option : null)
        };

    internal void Clear(NetScreenType screen, RunLocation location, long now, long frequency)
    {
        Hide(screen, location, now, frequency);
        decision = runnerUp = null;
    }

    internal void Hide(NetScreenType screen, RunLocation location, long now, long frequency)
    {
        bool wasVisible = Active && !hidden;
        hidden = true;
        if (restHovered) {
            submit(new RestSiteOptionHoveredMessage { optionIndex = null, Location = location });
            restHovered = false;
        }
        if (wasVisible || Screen != screen || now >= nextUpdate) {
            if (Screen != screen) position = null;
            Screen = screen;
            submit(Input(null, screen, position, false));
            // A neutral heartbeat also clears a lost unreliable hover/arrow packet.
            nextUpdate = now + frequency / 4;
        }
    }

    internal void Celebrate(Vector2? at) => submit(new ReactionMessage {
        type = ReactionType.ThumbUp, normalizedPosition = at ?? Vector2.Zero
    });
}
