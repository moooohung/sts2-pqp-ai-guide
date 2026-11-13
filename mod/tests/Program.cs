using System.Text.Json;
using HumanPlayRecorder;

if (args is ["--hold-journal", var journalRoot, var journalId])
{
    await StorageSafetyTests.HoldJournal(journalRoot, journalId);
    return;
}

if (args.Length == 3 && args[0] == "--probe-disabled")
{
    string gameDir = Path.GetFullPath(args[1]);
    System.Runtime.Loader.AssemblyLoadContext.Default.Resolving += (_, name) => {
        string path = Path.Combine(gameDir, name.Name + ".dll");
        return File.Exists(path) ? System.Reflection.Assembly.LoadFrom(path) : null;
    };
    var assembly = System.Reflection.Assembly.LoadFrom(Path.GetFullPath(args[2]));
    var output = new StringWriter();
    var original = Console.Out;
    try
    {
        Console.SetOut(output);
        assembly.GetType("HumanPlayRecorder.ReleaseBootstrap", true)!
            .GetMethod("Init")!.Invoke(null, null);
    }
    finally { Console.SetOut(original); }
    string line = output.ToString();
    if (!line.StartsWith("[HumanPlayRecorder] disabled: unsupported game build ") ||
        line.TrimEnd().Contains('\n')) throw new Exception("unsupported build did not disable cleanly: " + line);
    Console.Write(line);
    Console.WriteLine("PASS: packaged beta DLL loaded and initializer returned without patching on unsupported build.");
    return;
}

static void Require(bool condition, string message)
{
    if (!condition) throw new Exception(message);
}
if (args.Length > 1 && args[0] == "--verify-recordings")
{
    foreach (var path in args.Skip(1))
    {
        var actual = JsonSerializer.Deserialize<RunPayload>(await File.ReadAllBytesAsync(path), RecordingFormat.Json)!;
        Require(actual.ascension == 10, "original recorder fixture is A10");
        var header = new RunHeader(actual.run_id, actual.mod_version, actual.game_version,
            actual.game_dll_sha256, actual.character, actual.seed, actual.active_mods, actual.started_at,
            actual.mode, actual.party_size, actual.local_seat, actual.party, actual.shared_run_id) {ascension=actual.ascension,training=actual.training};
        string fixtureDirectory = Path.Combine(Path.GetTempPath(), "hpr-fixture-" + Guid.NewGuid().ToString("N"));
        await using var queue = new UploadQueue(fixtureDirectory); // No worker or HTTP attempt.
        await CompletedRun.QueueAsync(queue, header, actual.actions, actual.floor_digests,
            actual.result, actual.ended_at, true);
        Require((await File.ReadAllBytesAsync(Path.Combine(fixtureDirectory, actual.run_id + ".json"))).AsSpan()
            .SequenceEqual(JsonSerializer.SerializeToUtf8Bytes(actual, RecordingFormat.Json)), "real fixture queue is lossless");
        Console.WriteLine($"PASS: real A10 recording assemble/queue, seats={actual.party_size}, actions={actual.actions.Length}, floors={actual.floor_digests.Length}");
    }
    return;
}
var card = new RecordedAction("PlayCard", 0, hand_index: 2,
    card: new("STRIKE_IRONCLAD", 1), target: new("enemy", 1, "JAW_WORM", 2));
RecordingFormat.Validate(card);
var bytes = JsonSerializer.SerializeToUtf8Bytes(card, RecordingFormat.Json);
var roundTrip = JsonSerializer.Deserialize<RecordedAction>(bytes)!;
Require(roundTrip == card, "lossless hand/card/upgrade/target round trip");
var orderedCardJson = JsonSerializer.Serialize(card with {
    InputSequence = 7, ParentSequence = 3 }, RecordingFormat.Json);
Require(!orderedCardJson.Contains("InputSequence") && !orderedCardJson.Contains("ParentSequence"),
    "local input-order bookkeeping never enters action JSON");
var invalid = false;
try { RecordingFormat.Validate(card with { hand_index = null }); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "missing hand rejected");
invalid = false;
try { RecordingFormat.Validate(new("UsePotion", 0, slot_index: 0)); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "missing potion identity rejected");
var purchase = new RecordedAction("BuyCard", 0, card: new("BASH", 1), index: 4,
    slot_index: 4, price: 97);
RecordingFormat.Validate(purchase);
Require(JsonSerializer.Deserialize<RecordedAction>(
    JsonSerializer.Serialize(purchase, RecordingFormat.Json)) == purchase,
    "shop stock index identity upgrade and price round trip");
invalid = false;
try { RecordingFormat.Validate(purchase with { index = -1 }); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "unknown shop stock index rejected");
invalid = false;
try { RecordingFormat.Validate(new("PickPotion", 0, potion_id: "FIRE_POTION")); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "missing reward index rejected");
foreach (var item in new[] {
    new RecordedAction("BuyRelic", 0, index: 1, slot_index: 1, relic_id: "VAJRA", price: 150),
    new RecordedAction("BuyPotion", 0, index: 2, slot_index: 2, potion_id: "FIRE_POTION", price: 50),
    new RecordedAction("PickPotion", 0, reward_index: 2, reward_type: "potion", potion_id: "FIRE_POTION"),
    new RecordedAction("PickRelicReward", 0, reward_index: 1, reward_type: "relic", relic_id: "VAJRA"),
    new RecordedAction("DiscardPotion", 0, slot_index: 0, potion_id: "FIRE_POTION")
})
{
    RecordingFormat.Validate(item);
    Require(JsonSerializer.Deserialize<RecordedAction>(
        JsonSerializer.Serialize(item, RecordingFormat.Json)) == item,
        "item replay arguments round trip");
}
invalid = false;
try { RecordingFormat.Validate(purchase with { slot_index = null }); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "missing shop slot rejected");
invalid = false;
try { RecordingFormat.Validate(new("DiscardPotion", 0, slot_index: 0,
    potion_id: "FIRE_POTION", target: card.target)); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "targeted discard rejected");
RecordingFormat.Validate(new("Choose", 0, indices: [1, 3],
    cards: [new("BASH", 0), new("DEFEND_IRONCLAD", 1)]));
RecordingFormat.Validate(new("PickCardBundle", 0, index: 1,
    cards: [new("STRIKE_IRONCLAD", 0), new("DEFEND_IRONCLAD", 0)]));
RecordingFormat.Validate(new("RerollCardReward", 0,
    reward_index: 0, reward_type: "card"));
RecordingFormat.Validate(new("Collect", 0,
    reward_index: 0, reward_type: "relic", relic_id: "VAJRA"));
RecordingFormat.Validate(new("OpenChest", 0));
RecordingFormat.Validate(new("SkipRelic", 0, reward_type: "relic"));
Require(RecordingFormat.Phase(new("SkipRelic", 0, reward_type: "relic")) == "Treasure" &&
    RecordingFormat.Phase(new("OpenChest", 0)) == "Treasure" &&
    RecordingFormat.Phase(new("SkipRelic", 0, reward_index: 1, reward_type: "relic")) == "CardReward",
    "the solo treasure Skip and chest button are the treasure room's; a relic reward's Skip is the reward screen's");
Require(RecordingFormat.KnownActionSchema("sts2-native-input-v1") && RecordingFormat.KnownActionSchema("sts2-native-input-v2") &&
    !RecordingFormat.KnownActionSchema("sts2-native-input-v3"), "readers accept v1 and v2 recordings");
invalid = false;
try { RecordingFormat.Validate(new("SkipPotion", 0, reward_type: "potion")); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "only the treasure relic Skip omits its reward index");
invalid = false;
try { RecordingFormat.Validate(new("PickCardBundle", 0, index: 0)); }
catch (ArgumentException) { invalid = true; }
Require(invalid, "bundle without selected card identities rejected");
var digest = new StateDigest(1, 2, [new(0, 77, 99, 1, [new("BASH", 0)], [], [null])]);
string sharedId = RecordingFormat.SharedRunId("70004", "shared-start-nonce");
Require(sharedId == RecordingFormat.SharedRunId("70004", "shared-start-nonce"),
    "same replicated run identity groups clients identically");
Require(sharedId != RecordingFormat.SharedRunId("70004", "different-start-nonce"),
    "repeated seed in a reused lobby does not collapse distinct starts");
var mpDigest = new StateDigest(1, 2, [
    new(0, 77, 99, 1, [new("BASH", 0)], ["BURNING_BLOOD"], [null, "FIRE_POTION"]),
    new(1, 65, 88, 1, [new("NEUTRALIZE", 1)], ["RING_OF_THE_SNAKE"], ["DEXTERITY_POTION", null])
]);
using var mpStateJson = JsonDocument.Parse(JsonSerializer.Serialize(mpDigest, RecordingFormat.Json));
Require(mpStateJson.RootElement.GetProperty("players").GetArrayLength() == 2 &&
    mpStateJson.RootElement.GetProperty("players")[0].GetProperty("potions")[0].ValueKind == JsonValueKind.Null,
    "per-seat inventory and empty potion slots preserved");
RecordingFormat.Validate(new("Move", 1, sync_kind: "map_vote", generation: 0, cancelled: true));
RecordingFormat.Validate(new("Move", 1, coord: [2, 3], sync_kind: "map_vote", generation: 0));
Require(RecordingFormat.Hash(digest).Length == 64, "SHA256 hex width");
Require(RecordingFormat.Hash(digest) != RecordingFormat.Hash(digest with { floor = 3 }),
    "digest detects changed floor");
var payload = new RunPayload("sts2-human-run-v1", "sts2-native-input-v1",
    "01234567-89ab-4cde-8fab-0123456789ab", "0.1.0", "test-build",
    new string('a', 64), "IRONCLAD", 10, "70004", [],
    [new(0, "Combat", "2026-10-26T00:00:00Z", card, digest, RecordingFormat.Hash(digest))],
    [new(1, 2, 0, digest, RecordingFormat.Hash(digest))],
    new(false, 1, 2, false, 0), "2026-10-26T00:00:00Z", "2026-10-26T00:01:00Z",
    "solo", 1, 0, [new(0, "IRONCLAD")], new string('b', 64));
if (args is ["--storage-safety", "handoff"])
{
    await StorageSafetyTests.Handoff(payload);
    return;
}
if (args is ["--storage-safety", "oversize"])
{
    await StorageSafetyTests.Oversize(payload);
    return;
}
if (args is ["--storage-safety", "reservation"])
{
    await StorageSafetyTests.Reservation(payload);
    return;
}
if (args is ["--storage-safety", "ownership"])
{
    await StorageSafetyTests.Ownership();
    return;
}
if (args is ["--storage-safety", "retention"])
{
    await StorageSafetyTests.RetentionFailure();
    return;
}
if (args is ["--storage-safety", "delete-orphans"])
{
    await StorageSafetyTests.DeleteOrphans();
    return;
}
if (args is ["--storage-safety", "retry"])
{
    await StorageSafetyTests.RetryHandoff(payload);
    return;
}
if (args is ["--storage-safety", "capacity-warning"])
{
    await StorageSafetyTests.CapacityWarning(payload);
    return;
}
if (args is ["--storage-safety", "local-delete"])
{
    await StorageSafetyTests.LocalDelete(payload);
    return;
}
if (args is ["--storage-safety", "notice"])
{
    StorageSafetyTests.NoticeDeferral();
    return;
}
using var envelope = JsonDocument.Parse(JsonSerializer.Serialize(payload, RecordingFormat.Json));
string[] fields = ["schema", "action_schema", "run_id", "mod_version", "game_version",
    "game_dll_sha256", "character", "ascension", "seed", "active_mods", "actions",
    "floor_digests", "result", "started_at", "ended_at",
    "mode", "party_size", "local_seat", "party", "shared_run_id"];
Require(envelope.RootElement.EnumerateObject().Select(p => p.Name).ToHashSet().SetEquals(fields),
    "exact approved HTTP envelope (no local replay_certified)");
var mpPayload = payload with { mode = "multiplayer", party_size = 2, local_seat = 1,
    party = [new(0, "IRONCLAD"), new(1, "SILENT")], character = "SILENT", shared_run_id = sharedId };
using var mpEnvelope = JsonDocument.Parse(JsonSerializer.Serialize(mpPayload, RecordingFormat.Json));
Require(mpEnvelope.RootElement.GetProperty("local_seat").GetInt32() == 1 &&
    mpEnvelope.RootElement.GetProperty("party").GetArrayLength() == 2 &&
    !mpEnvelope.RootElement.TryGetProperty("run_start_token", out _),
    "MP roster/local seat are transmitted but raw shared start token is not");
Require(envelope.RootElement.GetProperty("floor_digests")[0].EnumerateObject()
    .Select(p => p.Name).ToHashSet().SetEquals(["act", "floor", "after_action", "state", "sha256"]),
    "exact authoritative floor digest shape");
var directory = Path.Combine(Path.GetTempPath(), "HumanPlayRecorder-" + Guid.NewGuid());
await using (var journal = new LocalJournal(directory, "test"))
{
    Require(journal.Append(new ActionRow(0, "Combat", "2026-10-26T00:00:00Z",
        card, digest, RecordingFormat.Hash(digest))), "queued without blocking");
}
var lines = await File.ReadAllLinesAsync(Path.Combine(directory, "test.jsonl"));
Require(lines.Length == 1, "durable one-row journal");
using var row = JsonDocument.Parse(lines[0]);
Require(row.RootElement.GetProperty("action").GetProperty("hand_index").GetInt32() == 2,
    "disk hand index preserved");
Require(!lines[0].Contains("NetId") && !lines[0].Contains("Steam") &&
    !lines[0].Contains("nickname"), "allowlisted DTO only");
Console.WriteLine("PASS: lossless combat DTO, required indices/identities, digest mutation, durable journal, privacy DTO");
Console.WriteLine("Test journal retained: " + directory);
var queueDir = Path.Combine(directory, "pending");
payload=payload with {game_dll_sha256=GameBuild.Sha256,
    training=new(GameBuild.VanillaFingerprint,"trainable")};
await AscensionRecordingTests.Run(payload);
var http = new QueueHandler();
var testNow=DateTimeOffset.UtcNow;
await using (var queue = new UploadQueue(queueDir, http,utcNow:()=>testNow,jitter:()=>0.5))
{
    var header = new RunHeader(payload.run_id, payload.mod_version, payload.game_version,
        payload.game_dll_sha256, payload.character, payload.seed, payload.active_mods, payload.started_at,
        payload.mode, payload.party_size, payload.local_seat, payload.party, payload.shared_run_id) {ascension=payload.ascension,training=payload.training};
    invalid = false;
    try { await CompletedRun.QueueAsync(queue, header, payload.actions, payload.floor_digests,
        payload.result, payload.ended_at, completeCapture: false); }
    catch (InvalidOperationException) { invalid = true; }
    Require(invalid && !Directory.Exists(queueDir), "incomplete run never queued");
    await CompletedRun.QueueAsync(queue, header, payload.actions, payload.floor_digests,
        payload.result, payload.ended_at, completeCapture: true);
    http.Code = 404;
    Require(!await queue.AttemptAsync(payload.run_id), "absent endpoint retained");
    Require(File.Exists(Path.Combine(queueDir, payload.run_id + ".json")), "404 keeps durable body");
    http.Code = 201; http.Body = """{"run_id":"wrong","status":"accepted"}""";
    testNow=testNow.AddHours(1);
    Require(!await queue.AttemptAsync(payload.run_id), "wrong acknowledgement rejected");
    http.Body = null;
    testNow=testNow.AddHours(1);
    Require(await queue.AttemptAsync(payload.run_id), "matching durable acknowledgement accepted");
    Require(!File.Exists(Path.Combine(queueDir, payload.run_id + ".json")), "accepted body removed");
    Require(http.CatboxCalls == 1, "persisted receipt avoids reupload after server/ack failures");
    Require(http.ValidRequest, "exact endpoint, content type and idempotency header");
    using var plain = JsonDocument.Parse(http.LastReceipt!);
    Require(!plain.RootElement.TryGetProperty("owner", out _) && !plain.RootElement.TryGetProperty("public_replay", out _),
        "receipt without replay stamp keeps the old key set");
}
Console.WriteLine("PASS: queue 404 retention, malformed acknowledgement retention, matching acknowledgement deletion");
{
    // Replay stamp: owner + public consent reach the receipt, and acceptance reports the stamped receipt.
    var stampDir = Path.Combine(directory, "stamped");
    var stampHttp = new QueueHandler { Code = 201, Body = null };
    string owner = new string('a', 64);
    UploadReceipt? accepted = null;
    await using var stamped = new UploadQueue(stampDir, stampHttp, utcNow: () => testNow, jitter: () => 0.5)
        { StampReceipt = r => r with { owner = owner, public_replay = true }, Accepted = r => accepted = r };
    var stampedPayload = payload with { run_id = Guid.NewGuid().ToString() };
    await stamped.EnqueueAsync(stampedPayload);
    Require(await stamped.AttemptAsync(stampedPayload.run_id), "stamped receipt accepted");
    using var sent = JsonDocument.Parse(stampHttp.LastReceipt!);
    Require(sent.RootElement.GetProperty("owner").GetString() == owner &&
        sent.RootElement.GetProperty("public_replay").GetBoolean() &&
        accepted is { owner: var o, public_replay: true } && o == owner &&
        accepted.sha256 == sent.RootElement.GetProperty("sha256").GetString(),
        "replay owner/consent sent and acceptance callback gets the stamped receipt");
    bool badOwner = false;
    try { UploadMetadata.Receipt(accepted with { owner = "not-a-hash" }); } catch (ArgumentException) { badOwner = true; }
    Require(badOwner, "malformed owner rejected before sending");
}
Console.WriteLine("PASS: replay owner/public consent stamped into receipts; acceptance callback");
{
    // Catbox refusing uploads (412 Invalid uploader) must not hold runs back: the server keeps the copy.
    var fallbackHttp = new QueueHandler { Code = 201, Body = null, CatboxCode = System.Net.HttpStatusCode.PreconditionFailed };
    await using var fallback = new UploadQueue(Path.Combine(directory, "server-copy"), fallbackHttp, utcNow: () => testNow, jitter: () => 0.5);
    var fallbackPayload = payload with { run_id = Guid.NewGuid().ToString() };
    await fallback.EnqueueAsync(fallbackPayload);
    bool fallbackOk = await fallback.AttemptAsync(fallbackPayload.run_id);
    using var fallbackReceipt = JsonDocument.Parse(fallbackHttp.LastReceipt ?? "{}"u8.ToArray());
    Require(fallbackOk && fallbackHttp.CatboxCalls == 1 && fallbackHttp.ServerCopyCalls == 1 && fallbackHttp.ReceiptCalls == 1 &&
        fallbackReceipt.RootElement.GetProperty("catbox_url").GetString() ==
            UploadQueue.ServerRecordingPrefix + fallbackReceipt.RootElement.GetProperty("sha256").GetString() + ".gz",
        "Catbox refusal sends the recording to the server and the receipt names the server copy");
}
{
    // If a server-copy receipt is delayed past the 24 h claim window, the server returns 422;
    // the queue drops the stale server-copy receipt, preserves backoff, and re-uploads later
    // instead of quarantining (.rejected), while a Catbox 422 still quarantines.
    var expiredDir = Path.Combine(directory, "server-copy-expired");
    var expiredHttp = new QueueHandler { Code = 503, Body = null, CatboxCode = System.Net.HttpStatusCode.PreconditionFailed };
    var expiredNow = DateTimeOffset.UtcNow;
    await using var expiredQueue = new UploadQueue(expiredDir, expiredHttp, utcNow: () => expiredNow, jitter: () => 0.5);
    var expiredPayload = payload with { run_id = Guid.NewGuid().ToString() };
    await expiredQueue.EnqueueAsync(expiredPayload);
    Require(!await expiredQueue.AttemptAsync(expiredPayload.run_id) &&
        expiredHttp.ServerCopyCalls == 1 && expiredHttp.ReceiptCalls == 1 &&
        File.Exists(Path.Combine(expiredDir, expiredPayload.run_id + ".json.receipt")),
        "transient receipt failure after server-copy upload retains receipt for retry");
    expiredNow = expiredNow.AddHours(25);
    expiredHttp.Code = 422;
    expiredHttp.Body = """{"error":"recording_missing"}""";
    Require(!await expiredQueue.AttemptAsync(expiredPayload.run_id) &&
        expiredHttp.ServerCopyCalls == 1 && expiredHttp.ReceiptCalls == 2 &&
        File.Exists(Path.Combine(expiredDir, expiredPayload.run_id + ".json")) &&
        !File.Exists(Path.Combine(expiredDir, expiredPayload.run_id + ".json.receipt")) &&
        !File.Exists(Path.Combine(expiredDir, expiredPayload.run_id + ".json.rejected")),
        "422 on expired server-copy receipt drops the receipt without quarantining the recording");
    expiredHttp.Code = 201;
    expiredHttp.Body = null;
    Require(!await expiredQueue.AttemptAsync(expiredPayload.run_id) && expiredHttp.ServerCopyCalls == 1,
        "dropped server-copy receipt still honors existing backoff before re-uploading");
    expiredNow = expiredNow.AddHours(1);
    Require(await expiredQueue.AttemptAsync(expiredPayload.run_id) &&
        expiredHttp.CatboxCalls == 2 && expiredHttp.ServerCopyCalls == 2 && expiredHttp.ReceiptCalls == 3 &&
        !File.Exists(Path.Combine(expiredDir, expiredPayload.run_id + ".json")),
        "next attempt after backoff re-uploads the recording and completes the receipt");

    var catboxRejectDir = Path.Combine(directory, "catbox-reject-422");
    var catboxRejectHttp = new QueueHandler { Code = 422, Body = null, CatboxCode = System.Net.HttpStatusCode.OK };
    await using var catboxRejectQueue = new UploadQueue(catboxRejectDir, catboxRejectHttp, utcNow: () => expiredNow, jitter: () => 0.5);
    var catboxRejectPayload = payload with { run_id = Guid.NewGuid().ToString() };
    await catboxRejectQueue.EnqueueAsync(catboxRejectPayload);
    Require(!await catboxRejectQueue.AttemptAsync(catboxRejectPayload.run_id) &&
        File.Exists(Path.Combine(catboxRejectDir, catboxRejectPayload.run_id + ".json.rejected")) &&
        File.Exists(Path.Combine(catboxRejectDir, catboxRejectPayload.run_id + ".json.receipt.rejected")),
        "Catbox receipt 422 still quarantines the item");

    // The server holds the bytes but they disagree with the receipt: a real rejection, never a re-upload loop.
    var mismatchDir = Path.Combine(directory, "server-copy-mismatch");
    var mismatchHttp = new QueueHandler { Code = 422, Body = """{"error":"invalid_receipt"}""", CatboxCode = System.Net.HttpStatusCode.PreconditionFailed };
    await using var mismatchQueue = new UploadQueue(mismatchDir, mismatchHttp, utcNow: () => expiredNow, jitter: () => 0.5);
    var mismatchPayload = payload with { run_id = Guid.NewGuid().ToString() };
    await mismatchQueue.EnqueueAsync(mismatchPayload);
    Require(!await mismatchQueue.AttemptAsync(mismatchPayload.run_id) &&
        File.Exists(Path.Combine(mismatchDir, mismatchPayload.run_id + ".json.rejected")) &&
        File.Exists(Path.Combine(mismatchDir, mismatchPayload.run_id + ".json.receipt.rejected")),
        "server-copy receipt rejected as invalid still quarantines the item");
}
Console.WriteLine("PASS: Catbox refusal falls back to the server copy and expired server-copy receipt re-uploads after backoff");
var liveId = Guid.NewGuid().ToString();
var liveHeader = new RunHeader(liveId, payload.mod_version, payload.game_version,
    "", payload.character, payload.seed, [], payload.started_at,
    "solo", 1, 0, payload.party, RecordingFormat.SharedRunId(payload.seed, liveId)) {ascension=payload.ascension};
await using (var liveQueue = new UploadQueue(Path.Combine(directory, "live-pending"), new QueueHandler()))
{
    var live = new LiveRunSession(liveHeader, typeof(RunPayload).Assembly.Location, [], liveQueue);
    var liveJournal = new LocalJournal(Path.Combine(directory, "live-journal"), liveId);
    live.Add(payload.actions[0]);
    live.Floor(digest, 0);
    live.End(liveJournal, payload.result, payload.ended_at, complete: true);
    await live.Completion;
    Require(live.Failure is null, "completed live session validation and durable queue handoff");
    using var queued = JsonDocument.Parse(await File.ReadAllTextAsync(
        Path.Combine(directory, "live-pending", liveId + ".json")));
    Require(queued.RootElement.GetProperty("run_id").GetString() == liveId &&
        !queued.RootElement.TryGetProperty("replay_certified", out _),
        "live queue body follows authoritative envelope, without local flags");
}
Console.WriteLine("PASS: terminal session -> validated RunPayload -> durable queue (mock transport only)");
var fullDir = Path.Combine(directory, "queue-full");
Directory.CreateDirectory(fullDir);
for (int i = 0; i < 100; i++) await File.WriteAllTextAsync(Path.Combine(fullDir, $"{i:D3}.json"), "{}");
string? queueFullFailure = null;
await using (var fullQueue = new UploadQueue(fullDir, new QueueHandler()))
{
    var fullId = Guid.NewGuid().ToString();
    var fullHeader = liveHeader with { run_id = fullId,
        shared_run_id = RecordingFormat.SharedRunId(payload.seed, fullId) };
    var full = new LiveRunSession(fullHeader, typeof(RunPayload).Assembly.Location, [], fullQueue,
        error => queueFullFailure = error.Message);
    var fullJournal = new LocalJournal(Path.Combine(directory, "queue-full-journal"), fullId);
    full.Add(payload.actions[0]);
    full.Floor(digest, 0);
    full.End(fullJournal, payload.result, payload.ended_at, complete: true);
    await full.Completion;
    Require(full.Failure is InvalidOperationException { Message: "queue_full" } &&
        queueFullFailure == "queue_full", "queue-full failure reaches the visible-notice callback");
}
Console.WriteLine("PASS: queue-full stops new durable bodies and reaches visible-notice callback");
await using (var cappedQueue = new UploadQueue(Path.Combine(directory, "oversize"), new QueueHandler()))
{
    // Synthetic unit input for byte-limit enforcement, not a real-run size measurement.
    var oversized = payload with { run_id = Guid.NewGuid().ToString(),
        game_version = new string('x', UploadQueue.MaxBodyBytes) };
    await cappedQueue.EnqueueAsync(oversized);
    Require(File.Exists(Path.Combine(directory, "oversize", oversized.run_id + ".json.oversize")) &&
        !File.Exists(Path.Combine(directory, "oversize", oversized.run_id + ".json")),
        "over-16MiB body retained locally as oversize and excluded from retry scan");
}
Console.WriteLine("PASS: synthetic byte-cap enforcement (not a gameplay payload-size measurement)");
await StorageBudgetTests.Run(payload);
await StorageSafetyTests.Handoff(payload);
await StorageSafetyTests.Oversize(payload);
await QueueResilienceTests.Run(payload);
await StorageSafetyTests.Reservation(payload);
await StorageSafetyTests.Ownership();
await SharingFlagsTests.Run(payload);
await StorageSafetyTests.RetentionFailure();
await StorageSafetyTests.DeleteOrphans();
await StorageSafetyTests.RetryHandoff(payload);
await StorageSafetyTests.CapacityWarning(payload);
await StorageSafetyTests.LocalDelete(payload);
StorageSafetyTests.NoticeDeferral();
Require(RecorderDisclosure.Text.Contains("catbox.moe") && RecorderDisclosure.Text.Contains("링크를 아는 누구나") &&
    RecorderDisclosure.Text.Contains("HTTPS") && RecorderDisclosure.Text.Contains("IP 주소가 전달") &&
    RecorderDisclosure.Text.Contains("30일간 로그에 보관") && RecorderDisclosure.Text.Contains("각자의 정책에 따라 로그를 처리"),
    "public notice discloses link access, IP exposure, our 30-day IP log and the other services' own logging policies");
Require(!RecorderDisclosure.Text.Contains("익명") && !RecorderDisclosure.Text.Contains("IP를 로그에 저장하지"),
    "the notice must not call uploads anonymous or promise that IPs are not logged");
Console.WriteLine("PASS: public Catbox/link-access/IP/logging disclosures");
RunStartDiagnosticsTests.Run();
StartupContentClassifierTests.Run();
AdviceRefusalTests.Run();
ReasonTipTests.Run();

sealed class QueueHandler : HttpMessageHandler
{
    public int Code = 404;
    public string? Body = "{}";
    public bool ValidRequest = true;
    public int CatboxCalls;
    public System.Net.HttpStatusCode CatboxCode = System.Net.HttpStatusCode.OK;
    public int ServerCopyCalls;
    public int ReceiptCalls;
    public byte[]? LastReceipt;
    // The run-clearance endpoint (a multiplayer run asks before any byte leaves the PC).
    public string RunState = "cleared";
    public int RunCode = 200;
    public int RunCalls;
    public byte[]? LastRun;
    public int? RetrySeconds;
    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
    {
        if(request.Headers.UserAgent.ToString()!=UploadQueue.UserAgent)
            throw new Exception("missing or identifying User-Agent");
        if (request.RequestUri == UploadQueue.CatboxEndpoint)
        {
            CatboxCalls++;
            var multipart = (MultipartFormDataContent)request.Content!;
            var parts = multipart.ToArray();
            ValidRequest &= parts.Length == 2 &&
                await parts[0].ReadAsStringAsync(ct) == "fileupload" &&
                parts.All(p => p.Headers.ContentDisposition?.Name?.Trim('"') != "userhash");
            using var compressed = await parts[1].ReadAsStreamAsync(ct);
            using var gzip = new System.IO.Compression.GZipStream(compressed, System.IO.Compression.CompressionMode.Decompress);
            using var recording = await JsonDocument.ParseAsync(gzip, cancellationToken: ct);
            ValidRequest &= recording.RootElement.GetProperty("schema").GetString() == "sts2-human-run-v2";
            return new HttpResponseMessage(CatboxCode) {
                Content = new StringContent(CatboxCode == System.Net.HttpStatusCode.OK ? "https://files.catbox.moe/abc123.gz" : "Invalid uploader") };
        }
        if (request.RequestUri == UploadQueue.RecordingEndpoint)
        {
            ServerCopyCalls++;
            ValidRequest &= request.Method == HttpMethod.Post && request.Content?.Headers.ContentType?.MediaType == "application/gzip";
            string sha = Convert.ToHexStringLower(System.Security.Cryptography.SHA256.HashData(await request.Content!.ReadAsByteArrayAsync(ct)));
            return new HttpResponseMessage(System.Net.HttpStatusCode.Created) {
                Content = new StringContent($$"""{"url":"{{UploadQueue.ServerRecordingPrefix}}{{sha}}.gz","sha256":"{{sha}}"}""") };
        }
        if (request.RequestUri == UploadQueue.RunEndpoint)
        {
            RunCalls++;
            ValidRequest &= request.Method == HttpMethod.Post &&
                request.Content?.Headers.ContentType?.ToString() == "application/json; charset=utf-8";
            LastRun = await request.Content!.ReadAsByteArrayAsync(ct);
            var runResponse = new HttpResponseMessage((System.Net.HttpStatusCode)RunCode)
                { Content = new StringContent($$"""{"run":"x","state":"{{RunState}}"}""") };
            if (RetrySeconds is { } runSeconds)
                runResponse.Headers.RetryAfter = new System.Net.Http.Headers.RetryConditionHeaderValue(TimeSpan.FromSeconds(runSeconds));
            return runResponse;
        }
        ReceiptCalls++;
        ValidRequest &= request.Method == HttpMethod.Post &&
            request.RequestUri == UploadQueue.Endpoint &&
            request.Content?.Headers.ContentType?.ToString() == "application/json; charset=utf-8";
        byte[] receiptBytes=await request.Content!.ReadAsByteArrayAsync(ct);
        LastReceipt=receiptBytes;
        using var receipt = JsonDocument.Parse(receiptBytes);
        string hash = receipt.RootElement.GetProperty("sha256").GetString()!;
        ValidRequest &= !receipt.RootElement.TryGetProperty("actions", out _) &&
            receipt.RootElement.GetProperty("catbox_url").GetString() == (CatboxCode == System.Net.HttpStatusCode.OK &&
                receipt.RootElement.GetProperty("party_size").GetInt32() == 1 ?
                "https://files.catbox.moe/abc123.gz" : UploadQueue.ServerRecordingPrefix + hash + ".gz");
        var response = new HttpResponseMessage((System.Net.HttpStatusCode)Code)
            { Content = new StringContent(Body ?? $$"""{"sha256":"{{hash}}","status":"accepted","receipt_sha256":"{{Convert.ToHexStringLower(System.Security.Cryptography.SHA256.HashData(receiptBytes))}}"}""") };
        if (RetrySeconds is { } seconds)
            response.Headers.RetryAfter = new System.Net.Http.Headers.RetryConditionHeaderValue(TimeSpan.FromSeconds(seconds));
        return response;
    }
}
