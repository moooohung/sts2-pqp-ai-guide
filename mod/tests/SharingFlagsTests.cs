using System.Text.Json;
using HumanPlayRecorder;

// Training consent and replay saving decide what an upload carries (UploadPolicy): a decline
// uploads the recording flagged training=false, and with both off nothing leaves the PC.
static class SharingFlagsTests
{
    static void Require(bool value, string reason) { if (!value) throw new Exception(reason); }

    public static async Task Run(RunPayload payload)
    {
        Require(UploadPolicy.Allowed(true, false) && UploadPolicy.Allowed(false, true) &&
            UploadPolicy.Allowed(true, true) && !UploadPolicy.Allowed(false, false),
            "a recording is allowed when training consent or replay saving is on");
        string owner = new('a', 64);
        foreach (var (training, replay) in new[] { (false, true), (true, false), (true, true) })
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-sharing-" + Guid.NewGuid().ToString("N"));
            var handler = new QueueHandler { Code = 201, Body = null };
            await using var queue = new UploadQueue(dir, handler)
            {
                StampReceipt = r => UploadPolicy.Flags(r with { owner = owner, public_replay = true }, training, replay)
            };
            await queue.EnqueueAsync(payload);
            bool uploaded = await queue.AttemptAsync(payload.run_id);
            Require(uploaded && handler.ReceiptCalls == 1,
                $"upload with training={training}, replay={replay} was not accepted: uploaded={uploaded}, " +
                $"receipts={handler.ReceiptCalls}, {queue.LastDiagnostic}");
            using var sent = JsonDocument.Parse(handler.LastReceipt!);
            var receipt = sent.RootElement;
            Require(receipt.GetProperty("training").GetBoolean() == training &&
                receipt.GetProperty("replay").GetBoolean() == replay,
                $"receipt must carry training={training} and replay={replay}");
            Require(receipt.TryGetProperty("owner", out _) && receipt.TryGetProperty("public_replay", out _) == replay,
                "the owner key always rides along for deletion; the public choice only while the replay is saved");
            Require(!receipt.TryGetProperty("run", out _) && !receipt.TryGetProperty("seat", out _),
                "a solo receipt carries no party fields");
        }
        string offDir = Path.Combine(Path.GetTempPath(), "hpr-sharing-off-" + Guid.NewGuid().ToString("N"));
        var offHandler = new QueueHandler { Code = 201, Body = null };
        await using (var queue = new UploadQueue(offDir, offHandler) { StampReceipt = r => UploadPolicy.Flags(r, false, false) })
        {
            await queue.EnqueueAsync(payload);
            await queue.AttemptAsync(payload.run_id);
            Require(offHandler.CatboxCalls == 0 && offHandler.ServerCopyCalls == 0 && offHandler.ReceiptCalls == 0,
                "with training consent and replay saving both off nothing may be uploaded");
            Require(!File.Exists(Path.Combine(offDir, payload.run_id + ".json")) &&
                !File.Exists(Path.Combine(offDir, payload.run_id + ".json.receipt")),
                "with both off the queued recording is discarded, not kept for a later upload");
        }
        // Multiplayer: every human player must have agreed; AI seats count as the host.
        ulong host = 1, second = 2, third = 3, ai = 1000;
        bool Agreed(ulong[] players, params (ulong Id, bool Yes)[] answers) =>
            PartyConsent.AllAgreed(players, host, id => id == ai, answers.ToDictionary(a => a.Id, a => a.Yes));
        Require(Agreed([host, second, third], (host, true), (second, true), (third, true)),
            "a party where everyone agreed may upload");
        Require(!Agreed([host, second, third], (host, true), (second, true), (third, false)),
            "a peer who declined keeps the whole run on the PC");
        Require(!Agreed([host, second, third], (host, true), (second, true)),
            "a peer who never answered (vanilla game or no answer yet) keeps the whole run on the PC");
        Require(Agreed([host, second, ai], (host, true), (second, true)), "an AI seat counts as the host who agreed");
        Require(!Agreed([host, second, ai], (host, false), (second, true)), "an AI seat of a host who declined does not upload");
        var party = payload with { party_size = 2, started_at = DateTimeOffset.UtcNow.AddHours(-1).ToString("O"), ended_at = DateTimeOffset.UtcNow.AddMinutes(-5).ToString("O") };
        foreach (bool localAgreed in new[] { false, true })
        {
            string mpDir = Path.Combine(Path.GetTempPath(), "hpr-sharing-mp-" + Guid.NewGuid().ToString("N"));
            var mpHandler = new QueueHandler { Code = 201, Body = null };
            await using var queue = new UploadQueue(mpDir, mpHandler)
            {
                StampReceipt = r => UploadPolicy.Flags(r with { owner = owner }, localAgreed, true)
            };
            await queue.EnqueueAsync(party);
            await queue.AttemptAsync(party.run_id);
            if (!localAgreed)
                Require(mpHandler.CatboxCalls == 0 && mpHandler.ServerCopyCalls == 0 && mpHandler.ReceiptCalls == 0,
                    "a multiplayer run is not uploaded for replay when this player's training consent is gone");
            else
            {
                Require(mpHandler.ReceiptCalls == 1, "a multiplayer run everyone agreed to is uploaded");
                using var sent = JsonDocument.Parse(mpHandler.LastReceipt!);
                Require(sent.RootElement.GetProperty("training").GetBoolean() &&
                    sent.RootElement.GetProperty("replay").GetBoolean(), "the agreed party's receipt carries training and replay");
                // Authority is the server's: this install uploads its own receipt with the run's shared id and its seat.
                Require(sent.RootElement.GetProperty("run").GetString() == party.shared_run_id &&
                    sent.RootElement.GetProperty("seat").GetInt32() == party.local_seat &&
                    sent.RootElement.GetProperty("owner").GetString() == owner,
                    "a multiplayer receipt names the shared run, this install's seat and its owner key");
            }
        }
        Console.WriteLine("PASS: declined training uploads training=false, both off uploads nothing, public choice only with replay saving, multiplayer needs every player's agreement");
        await WithdrawalTests(payload);
        RunConsentTests();
        Console.WriteLine("PASS: queued uploads follow withdrawal, in-flight upload cancelled, no key holds the upload, run consent only gets stricter");
        await PartyClearanceTests(payload);
        Console.WriteLine("PASS: a multiplayer run uploads nothing until the server says cleared; pending is kept then deleted, refused is deleted");
        await DeleteAllTests(payload);
        Console.WriteLine("PASS: delete-all cancels and purges pending uploads and refuses runs that began before it");
        await CaptureLeaseTests(payload);
        Console.WriteLine("PASS: switching sharing fully off mid-run ends the capture for good, even if it is switched back on");
    }

    // Both settings off during a run stops the capture; turning one back on must not revive it, or the
    // recording would cover the time in which the player had asked for nothing to be recorded.
    static async Task CaptureLeaseTests(RunPayload payload)
    {
        bool training = true, replay = true;
        await using var journal = new LocalJournal(Path.Combine(Path.GetTempPath(), "hpr-lease-" + Guid.NewGuid().ToString("N")),
            Guid.NewGuid().ToString(), persist: false);
        var admission = new RecordingAdmission();
        int ends = 0;
        var lease = new CaptureLease(() => UploadPolicy.Allowed(training, replay), () => { ends++; admission.Reject(); journal.Invalidate(); });
        training = false;
        lease.Check();
        Require(!lease.Ended && journal.Complete && admission.Clean, "replay saving alone keeps the capture going");
        replay = false;
        lease.Check();
        Require(lease.Ended && !journal.Complete && !admission.Clean && ends == 1, "both off stops and invalidates the active capture");
        replay = true;
        lease.Check();
        replay = false;
        lease.Check();
        Require(lease.Ended && !journal.Complete && !admission.Clean && ends == 1, "switching it back on mid-run does not revive the capture");

        // The journal refuses rows once the lease invalidated it; the in-memory recording used to keep growing
        // regardless. The session now stops with the journal and queues nothing.
        string dir = Path.Combine(Path.GetTempPath(), "hpr-discard-" + Guid.NewGuid().ToString("N"));
        var id = Guid.NewGuid().ToString();
        var header = new RunHeader(id, payload.mod_version, payload.game_version, "", payload.character, payload.seed, [],
            payload.started_at, "solo", 1, 0, payload.party, RecordingFormat.SharedRunId(payload.seed, id)) { ascension = payload.ascension };
        await using var queue = new UploadQueue(Path.Combine(dir, "pending"), new QueueHandler());
        var session = new LiveRunSession(header, typeof(RunPayload).Assembly.Location, [], queue);
        var live = new LocalJournal(Path.Combine(dir, "journal"), id, persist: false);
        Require(session.Record(live, payload.actions[0]) && !session.Discarded, "a valid journal feeds the recording session");
        live.Invalidate(); // what the lease does when both settings go off
        Require(!session.Record(live, payload.actions[0]) && session.Discarded,
            "a row the journal refuses discards the recording session instead of being added to it");
        session.Floor(payload.floor_digests[0].state, 0);
        session.End(live, payload.result, payload.ended_at, complete: true);
        await session.Completion;
        bool Queued() => Directory.Exists(Path.Combine(dir, "pending")) && Directory.EnumerateFiles(Path.Combine(dir, "pending")).Any();
        Require(session.Failure is null && !Queued(), "a discarded recording is never queued");
        var direct = new LiveRunSession(header, typeof(RunPayload).Assembly.Location, [], queue);
        direct.Add(payload.actions[0]);
        direct.Discard();
        direct.Add(payload.actions[0]);
        direct.End(new LocalJournal(Path.Combine(dir, "journal2"), id, persist: false), payload.result, payload.ended_at, complete: true);
        await direct.Completion;
        Require(direct.Discarded && direct.Failure is null && !Queued(),
            "a discarded session ignores later rows and queues nothing");
    }

    // A multiplayer run asks the server before anything leaves the PC: only "cleared" lets bytes go, and then only
    // to the server's own copy. "pending" keeps the run locally for a while, "refused" deletes it.
    static async Task PartyClearanceTests(RunPayload payload)
    {
        string owner = new('a', 64);
        RunPayload Party(DateTimeOffset clock) => payload with
        {
            party_size = 2, run_id = Guid.NewGuid().ToString(),
            started_at = clock.AddHours(-1).ToString("O"), ended_at = clock.AddMinutes(-5).ToString("O")
        };
        UploadQueue Make(string dir, QueueHandler handler, Func<DateTimeOffset> clock) =>
            new(dir, handler, utcNow: clock, jitter: () => 0.5) { StampReceipt = r => UploadPolicy.Flags(r with { owner = owner }, true, true) };
        int Network(QueueHandler h) => h.CatboxCalls + h.ServerCopyCalls + h.ReceiptCalls;
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var run = Party(clock);
            var handler = new QueueHandler { RunState = "pending", Code = 201, Body = null };
            await using var queue = Make(dir, handler, () => clock);
            await queue.EnqueueAsync(run);
            Require(!await queue.AttemptAsync(run.run_id) && handler.RunCalls == 1 && Network(handler) == 0 &&
                File.Exists(Path.Combine(dir, run.run_id + ".json")) && !File.Exists(Path.Combine(dir, run.run_id + ".json.receipt")),
                "a pending multiplayer run uploads nothing and stays on the PC (no reservation, no bytes)");
            using (var asked = JsonDocument.Parse(handler.LastRun!))
            {
                var root = asked.RootElement;
                Require(root.GetProperty("run").GetString() == run.shared_run_id && root.GetProperty("seat").GetInt32() == run.local_seat &&
                    root.GetProperty("party_size").GetInt32() == 2 && root.GetProperty("owner").GetString() == owner &&
                    root.GetProperty("started").GetInt64() == DateTimeOffset.Parse(run.started_at).ToUnixTimeSeconds(),
                    "the declaration names the shared run, this install's seat, the party size, the owner and the run's start");
            }
            clock = clock.AddMinutes(30);
            handler.RunState = "cleared";
            Require(await queue.AttemptAsync(run.run_id) && handler.RunCalls == 2, "a cleared run uploads");
            using var sent = JsonDocument.Parse(handler.LastReceipt!);
            Require(handler.CatboxCalls == 0 && handler.ServerCopyCalls == 1 && handler.ReceiptCalls == 1 &&
                sent.RootElement.GetProperty("catbox_url").GetString()!.StartsWith(UploadQueue.ServerRecordingPrefix) &&
                sent.RootElement.GetProperty("started").GetInt64() == DateTimeOffset.Parse(run.started_at).ToUnixTimeSeconds(),
                "a multiplayer recording goes only to the server's copy, never to Catbox");
        }
        {
            // Pending beyond the hold: deleted. A server that cannot answer: kept, never uploaded.
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-hold-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var run = Party(clock);
            var handler = new QueueHandler { RunState = "pending" };
            await using var queue = Make(dir, handler, () => clock);
            await queue.EnqueueAsync(run);
            handler.RunCode = 503;
            Require(!await queue.AttemptAsync(run.run_id) && Network(handler) == 0 && File.Exists(Path.Combine(dir, run.run_id + ".json")),
                "no answer from the server: nothing is uploaded and the run is kept");
            handler.RunCode = 200;
            clock = clock.AddHours(7);
            Require(!await queue.AttemptAsync(run.run_id) && File.Exists(Path.Combine(dir, run.run_id + ".json")),
                "still pending inside the hold: kept");
            clock = clock.Add(UploadQueue.PartyHold);
            Require(await queue.AttemptAsync(run.run_id) && Network(handler) == 0 &&
                !Directory.EnumerateFiles(dir, run.run_id + ".json*").Any(), "a run still pending after the hold is deleted");
        }
        {
            // The hold ends a run whatever the server answers, fails to answer or whether there is an install key:
            // pending data is never kept beyond it.
            var cases = new (string Why, Action<QueueHandler> Setup, bool Key)[]
            {
                ("the server is unreachable", h => h.RunCode = 503, true),
                ("the server answers something that is not JSON", h => h.RunState = "x\"y\"", true),
                ("the server answers an unknown state", h => h.RunState = "maybe", true),
                ("there is no install key yet", h => { }, false),
            };
            foreach (var (why, setup, key) in cases)
            {
                string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-expiry-" + Guid.NewGuid().ToString("N"));
                var clock = DateTimeOffset.UtcNow;
                var run = Party(clock);
                var handler = new QueueHandler { RunState = "cleared", Code = 201, Body = null };
                setup(handler);
                await using var queue = new UploadQueue(dir, handler, utcNow: () => clock, jitter: () => 0.5)
                {
                    StampReceipt = r => key ? UploadPolicy.Flags(r with { owner = owner }, true, true) : null
                };
                await queue.EnqueueAsync(run);
                Require(!await queue.AttemptAsync(run.run_id) && File.Exists(Path.Combine(dir, run.run_id + ".json")) && Network(handler) == 0,
                    why + ": the run is kept inside the hold");
                clock = clock.Add(UploadQueue.PartyHold).AddHours(1);
                Require(await queue.AttemptAsync(run.run_id) && Network(handler) == 0 && !Directory.EnumerateFiles(dir, run.run_id + ".json*").Any(),
                    why + ": the run is deleted after the hold");
            }
        }
        {
            // The hold runs on its own clock: a server Retry-After (up to a day) that is still running when the hold
            // ends cannot keep the run, whether the worker scans the queue or the run is attempted directly.
            foreach (bool viaScan in new[] { true, false })
            {
                string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-backoff-" + Guid.NewGuid().ToString("N"));
                var clock = DateTimeOffset.UtcNow;
                var run = Party(clock);
                var handler = new QueueHandler { RunState = "pending", RetrySeconds = 86400 };
                await using var queue = Make(dir, handler, () => clock);
                await queue.EnqueueAsync(run);
                clock = clock.Add(UploadQueue.PartyHold).AddHours(-1);
                Require(!await queue.AttemptAsync(run.run_id) && handler.RunCalls == 1 && File.Exists(Path.Combine(dir, run.run_id + ".json")),
                    "fixture: the server is pending and asks for a day's back-off just before the hold ends");
                clock = clock.AddHours(2);
                int asked = handler.RunCalls;
                if (viaScan) await queue.ScanOnceAsync(); else await queue.AttemptAsync(run.run_id);
                Require(handler.RunCalls == asked && Network(handler) == 0 && !Directory.EnumerateFiles(dir, run.run_id + ".json*").Any(),
                    (viaScan ? "a scan" : "an attempt") + " ends the run at the hold although the back-off is still running, before any network contact");
            }
        }
        {
            // The expiry sweep handles each file on its own: one that cannot be read does not stop the files after
            // it, and a recording is deserialized once (50 queued recordings: later sweeps only look up deadlines).
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-sweep-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var handler = new QueueHandler { RunState = "pending" };
            await using var queue = Make(dir, handler, () => clock);
            var runs = Enumerable.Range(0, 50).Select(_ => Party(clock)).ToList();
            foreach (var r in runs) await queue.EnqueueAsync(r);
            string Body(RunPayload r) => Path.Combine(dir, r.run_id + ".json");
            // Wake a fresh queue over the same directory, as after a restart: nothing is known yet.
            await using var restarted = Make(dir, handler, () => clock);
            var watch = System.Diagnostics.Stopwatch.StartNew();
            await restarted.ExpireHeldAsync();
            long first = watch.ElapsedMilliseconds;
            watch.Restart();
            await restarted.ExpireHeldAsync();
            long later = watch.ElapsedMilliseconds;
            Console.WriteLine($"expiry sweep over 50 queued recordings: first {first} ms (each read once), later {later} ms");
            Require(runs.All(r => File.Exists(Body(r))), "nothing is purged inside the hold");
            clock = clock.Add(UploadQueue.PartyHold).AddHours(1);
            using var locked = new FileStream(Body(runs[0]), FileMode.Open, FileAccess.Read, FileShare.None);
            await restarted.ExpireHeldAsync();
            Require(File.Exists(Body(runs[0])) && runs.Skip(1).All(r => !File.Exists(Body(r))),
                "a file that cannot be read is skipped, and every other expired run after it is still purged");
            locked.Dispose();
            await restarted.ExpireHeldAsync();
            Require(!File.Exists(Body(runs[0])), "the skipped file is purged by the next sweep");
        }
        {
            // The remembered deadline belongs to the file it was read from: a queued file replaced under the same run
            // id (a solo body that was cached as never expiring, replaced by a multiplayer one that ended long ago) is
            // read again and expires on its own deadline.
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-replaced-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var handler = new QueueHandler { RunState = "pending" };
            await using var queue = Make(dir, handler, () => clock);
            var solo = Party(clock) with { party_size = 1 };
            await queue.EnqueueAsync(solo);
            await queue.ExpireHeldAsync();
            string file = Path.Combine(dir, solo.run_id + ".json");
            clock = clock.Add(UploadQueue.PartyHold).AddHours(1);
            await queue.ExpireHeldAsync();
            Require(File.Exists(file), "a solo run is never held, so its remembered deadline never expires");
            var replacement = solo with { party_size = 2, ended_at = clock.AddHours(-80).ToString("O"), started_at = clock.AddHours(-81).ToString("O") };
            File.WriteAllBytes(file, JsonSerializer.SerializeToUtf8Bytes(replacement, RecordingFormat.Json));
            await queue.ExpireHeldAsync();
            Require(!File.Exists(file), "a file replaced under the same run id is read again and expires on its own earlier deadline");
        }
        {
            // A receipt saved while the server could not take it waits under the same limit: past the hold the run is
            // deleted before any network contact, not retried for ever.
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-saved-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var run = Party(clock);
            var handler = new QueueHandler { RunState = "cleared", Code = 503 };
            await using var queue = Make(dir, handler, () => clock);
            await queue.EnqueueAsync(run);
            Require(!await queue.AttemptAsync(run.run_id) && handler.ReceiptCalls == 1 && File.Exists(Path.Combine(dir, run.run_id + ".json.receipt")),
                "fixture: the receipt is saved while the server cannot take it");
            clock = clock.AddHours(2);
            Require(!await queue.AttemptAsync(run.run_id) && handler.ReceiptCalls == 2 && File.Exists(Path.Combine(dir, run.run_id + ".json.receipt")),
                "a saved receipt is retried inside the hold");
            int calls = Network(handler), asked = handler.RunCalls;
            clock = clock.Add(UploadQueue.PartyHold);
            Require(await queue.AttemptAsync(run.run_id) && Network(handler) == calls && handler.RunCalls == asked &&
                !Directory.EnumerateFiles(dir, run.run_id + ".json*").Any(), "a saved receipt past the hold is deleted before any network contact");
        }
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-refused-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var run = Party(clock);
            var handler = new QueueHandler { RunState = "refused" };
            await using var queue = Make(dir, handler, () => clock);
            await queue.EnqueueAsync(run);
            Require(await queue.AttemptAsync(run.run_id) && Network(handler) == 0 && !Directory.EnumerateFiles(dir, run.run_id + ".json*").Any(),
                "a refused run is deleted and nothing is uploaded");
        }
        {
            // A solo run never asks.
            string dir = Path.Combine(Path.GetTempPath(), "hpr-clearance-solo-" + Guid.NewGuid().ToString("N"));
            var handler = new QueueHandler { Code = 201, Body = null };
            await using var queue = Make(dir, handler, () => DateTimeOffset.UtcNow);
            var solo = payload with { run_id = Guid.NewGuid().ToString() };
            await queue.EnqueueAsync(solo);
            Require(await queue.AttemptAsync(solo.run_id) && handler.RunCalls == 0 && handler.CatboxCalls == 1, "a solo run is not held for a party answer");
        }
    }

    // "Delete all my data": the pending queue is purged and an upload in flight cancelled, and a run that began
    // before the deletion (queued later, or still being played) is never queued or sent.
    static async Task DeleteAllTests(RunPayload payload)
    {
        string dir = Path.Combine(Path.GetTempPath(), "hpr-delete-all-" + Guid.NewGuid().ToString("N"));
        var now = DateTimeOffset.UtcNow;
        RunPayload Run(DateTimeOffset started) => payload with
        {
            run_id = Guid.NewGuid().ToString(), started_at = started.ToString("O"), ended_at = started.AddMinutes(30).ToString("O")
        };
        var queued = new[] { Run(now.AddHours(-3)), Run(now.AddHours(-2)) };
        var handler = new QueueHandler { Code = 503 };
        await using (var queue = Queue(dir, handler, new Choices(), () => now))
        {
            foreach (var run in queued) await queue.EnqueueAsync(run);
            await queue.AttemptAsync(queued[0].run_id);
            Require(File.Exists(Path.Combine(dir, queued[0].run_id + ".json.receipt")), "fixture: a receipt waits for the server");
            await queue.PurgeAllAsync(now);
            Require(!queued.Any(run => Directory.EnumerateFiles(dir, run.run_id + ".json*").Any()),
                "delete-all removes every queued body and receipt");
            int calls = handler.ReceiptCalls + handler.CatboxCalls + handler.ServerCopyCalls;
            await queue.AttemptAsync(queued[1].run_id);
            Require(handler.ReceiptCalls + handler.CatboxCalls + handler.ServerCopyCalls == calls, "nothing is sent after delete-all");
            var during = Run(now.AddMinutes(-10)); // still being played when the player deleted everything
            await queue.EnqueueAsync(during);
            Require(!Directory.EnumerateFiles(dir, during.run_id + ".json*").Any(), "a run that began before the deletion is not queued");
            var after = Run(now.AddMinutes(10));
            await queue.EnqueueAsync(after);
            Require(File.Exists(Path.Combine(dir, after.run_id + ".json")), "a run that began after the deletion is queued as usual");
        }
        await using (var again = Queue(dir, handler, new Choices(), () => now))
        {
            var late = Run(now.AddMinutes(-1));
            await again.EnqueueAsync(late);
            Require(!Directory.EnumerateFiles(dir, late.run_id + ".json*").Any(), "the deletion barrier survives a restart");
        }
        // A saved body that began before the barrier (a restored queue, another process) is purged by the attempt.
        {
            string dir2 = Path.Combine(Path.GetTempPath(), "hpr-delete-all2-" + Guid.NewGuid().ToString("N"));
            var old = Run(now.AddHours(-1));
            var h2 = new QueueHandler { Code = 201, Body = null };
            await using (var writer = Queue(dir2, h2, new Choices(), () => now)) await writer.EnqueueAsync(old);
            File.WriteAllText(Path.Combine(dir2, ".delete-barrier"), now.ToUnixTimeSeconds().ToString());
            await using var reader = Queue(dir2, h2, new Choices(), () => now);
            Require(await reader.AttemptAsync(old.run_id) && h2.CatboxCalls + h2.ReceiptCalls == 0 &&
                !Directory.EnumerateFiles(dir2, old.run_id + ".json*").Any(), "an attempt never sends a run that began before the barrier");
        }
        // "Delete all" racing a queue write: the barrier is checked under the lock the deletion takes, so a run that began
        // before it cannot be written behind it, even when the write was already waiting for the lock.
        {
            string dir4 = Path.Combine(Path.GetTempPath(), "hpr-delete-all4-" + Guid.NewGuid().ToString("N"));
            var transport = new GatedTransport();
            var inFlight = Run(now.AddHours(-1));
            await using var queue = Queue(dir4, transport, new Choices(), () => DateTimeOffset.UtcNow);
            await queue.EnqueueAsync(inFlight);
            var attempt = queue.AttemptAsync(inFlight.run_id); // holds the queue's lock inside a transport call
            await transport.Started.Task.WaitAsync(TimeSpan.FromSeconds(20));
            var purge = queue.PurgeAllAsync(now);              // waits for the lock
            var racing = Run(now.AddMinutes(-30));             // began before the deletion
            var write = queue.EnqueueAsync(racing);            // waits behind the deletion
            transport.Release.SetResult();
            await Task.WhenAll(attempt, purge, write).WaitAsync(TimeSpan.FromSeconds(30));
            Require(!Directory.EnumerateFiles(dir4, racing.run_id + ".json*").Any(),
                "a run that began before delete-all was written behind it");
            var later = Run(now.AddMinutes(10));
            await queue.EnqueueAsync(later);
            Require(File.Exists(Path.Combine(dir4, later.run_id + ".json")), "a run that began after the deletion is queued as usual");
        }
        // An upload in flight is cancelled by delete-all and leaves nothing behind.
        {
            string dir3 = Path.Combine(Path.GetTempPath(), "hpr-delete-all3-" + Guid.NewGuid().ToString("N"));
            var hang = new HangingTransport();
            var run = Run(now.AddHours(-1));
            await using var queue = Queue(dir3, hang, new Choices(), () => DateTimeOffset.UtcNow);
            await queue.EnqueueAsync(run);
            var attempt = queue.AttemptAsync(run.run_id);
            await hang.Started.Task.WaitAsync(TimeSpan.FromSeconds(20));
            await queue.PurgeAllAsync(now);
            Require(!await attempt.WaitAsync(TimeSpan.FromSeconds(20)) && hang.Cancelled &&
                !Directory.EnumerateFiles(dir3, run.run_id + ".json*").Any(), "delete-all cancels an upload in flight and removes its files");
        }
    }
    // The player's current choices, read at every use.
    sealed class Choices { public bool Training = true, Replay = true; public string? Owner = new string('a', 64); }

    static UploadQueue Queue(string dir, HttpMessageHandler handler, Choices c, Func<DateTimeOffset> clock) =>
        new(dir, handler, utcNow: clock, jitter: () => 0.5)
        {
            StampReceipt = r => UploadPolicy.Stamp(r, c.Owner, true, c.Training, c.Replay),
            Allowed = () => UploadPolicy.Allowed(c.Training, c.Replay)
        };

    static async Task WithdrawalTests(RunPayload payload)
    {
        // A receipt saved while consent stood (the server was down) is restated with training=false after
        // consent is withdrawn, whether the queue is told or the attempt notices. It is never sent with the
        // old flags, and its recording is not uploaded a second time.
        foreach (bool restateOnChange in new[] { true, false })
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-withdraw-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var choices = new Choices();
            var handler = new QueueHandler { Code = 503 };
            await using var queue = Queue(dir, handler, choices, () => clock);
            await queue.EnqueueAsync(payload);
            await queue.AttemptAsync(payload.run_id);
            string saved = Path.Combine(dir, payload.run_id + ".json.receipt");
            Require(File.Exists(saved) && handler.ReceiptCalls == 1, "fixture: a receipt was saved while the server was down");
            int uploads = handler.CatboxCalls + handler.ServerCopyCalls;
            choices.Training = false;
            if (restateOnChange)
            {
                await queue.ApplyChoicesAsync();
                using var restated = JsonDocument.Parse(File.ReadAllBytes(saved));
                Require(!restated.RootElement.GetProperty("training").GetBoolean() && restated.RootElement.GetProperty("replay").GetBoolean(),
                    "withdrawal restates the saved receipt with training=false instead of keeping training=true");
            }
            handler.Code = 201; handler.Body = null;
            clock = clock.AddHours(7);
            Require(await queue.AttemptAsync(payload.run_id), "the restated receipt is accepted");
            using var sent = JsonDocument.Parse(handler.LastReceipt!);
            Require(handler.ReceiptCalls == 2 && !sent.RootElement.GetProperty("training").GetBoolean() &&
                sent.RootElement.GetProperty("replay").GetBoolean(),
                "the receipt that goes out says training=false (restateOnChange=" + restateOnChange + ")");
            Require(handler.CatboxCalls + handler.ServerCopyCalls == uploads,
                "the recording is not uploaded again under the new flags (restateOnChange=" + restateOnChange + ")");
        }
        // A multiplayer run counts only with this install's own yes, so withdrawal purges its queue instead.
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-withdraw-mp-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var choices = new Choices();
            var handler = new QueueHandler { Code = 503 };
            var party = payload with { party_size = 2, started_at = DateTimeOffset.UtcNow.AddHours(-1).ToString("O"), ended_at = DateTimeOffset.UtcNow.AddMinutes(-5).ToString("O") };
            await using var queue = Queue(dir, handler, choices, () => clock);
            await queue.EnqueueAsync(party);
            await queue.AttemptAsync(party.run_id);
            Require(File.Exists(Path.Combine(dir, party.run_id + ".json.receipt")) && handler.ReceiptCalls == 1,
                "fixture: a multiplayer receipt was saved while the server was down");
            choices.Training = false;
            await queue.ApplyChoicesAsync();
            Require(!Directory.EnumerateFiles(dir, party.run_id + ".json*").Any(),
                "withdrawing training consent purges a queued multiplayer run");
            clock = clock.AddHours(7);
            await queue.AttemptAsync(party.run_id);
            Require(handler.ReceiptCalls == 1, "a purged multiplayer run is never sent");
        }
        // Nothing allowed any more: queued bodies and receipts go, and no attempt reaches the network.
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-withdraw-all-" + Guid.NewGuid().ToString("N"));
            var clock = DateTimeOffset.UtcNow;
            var choices = new Choices();
            var handler = new QueueHandler { Code = 503 };
            await using var queue = Queue(dir, handler, choices, () => clock);
            await queue.EnqueueAsync(payload);
            await queue.AttemptAsync(payload.run_id);
            choices.Training = choices.Replay = false;
            await queue.ApplyChoicesAsync();
            Require(!Directory.EnumerateFiles(dir, payload.run_id + ".json*").Any(), "withdrawing everything purges queued bodies and receipts");
            int calls = handler.ReceiptCalls + handler.CatboxCalls + handler.ServerCopyCalls;
            clock = clock.AddHours(7);
            await queue.AttemptAsync(payload.run_id);
            Require(handler.ReceiptCalls + handler.CatboxCalls + handler.ServerCopyCalls == calls, "no network after withdrawal");
        }
        // An upload in flight when consent is withdrawn is cancelled and leaves nothing behind.
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-inflight-" + Guid.NewGuid().ToString("N"));
            var choices = new Choices();
            var hang = new HangingTransport();
            await using var queue = Queue(dir, hang, choices, () => DateTimeOffset.UtcNow);
            await queue.EnqueueAsync(payload);
            var attempt = queue.AttemptAsync(payload.run_id);
            await hang.Started.Task.WaitAsync(TimeSpan.FromSeconds(20));
            choices.Training = choices.Replay = false;
            await queue.ApplyChoicesAsync();
            Require(!await attempt.WaitAsync(TimeSpan.FromSeconds(20)), "the cancelled attempt reports no upload");
            Require(hang.Cancelled && hang.Requests == 1 && !Directory.EnumerateFiles(dir, payload.run_id + ".json*").Any(),
                "in-flight upload cancelled and its recording and reservation removed");
        }
        // No durable install key: the recording is held, never uploaded.
        {
            string dir = Path.Combine(Path.GetTempPath(), "hpr-nokey-" + Guid.NewGuid().ToString("N"));
            var choices = new Choices { Owner = null };
            var handler = new QueueHandler { Code = 201, Body = null };
            await using var queue = Queue(dir, handler, choices, () => DateTimeOffset.UtcNow);
            await queue.EnqueueAsync(payload);
            await queue.AttemptAsync(payload.run_id);
            Require(handler.CatboxCalls + handler.ServerCopyCalls + handler.ReceiptCalls == 0 &&
                File.Exists(Path.Combine(dir, payload.run_id + ".json")), "without an install key nothing is uploaded and the recording stays queued");
        }
        // A replay link is announced only for a listed replay of this install.
        var receipt = new UploadReceipt("", "", "", new string('b', 64), 1, "", "", "", 10, 1, owner: new string('a', 64), replay: false);
        Require(!UploadPolicy.Announces(receipt, new string('a', 64)) &&
            UploadPolicy.Announces(receipt with { replay = true }, new string('a', 64)) &&
            !UploadPolicy.Announces(receipt with { replay = true }, new string('c', 64)),
            "no replay link for replay=false or a foreign owner");
    }

    // Holds the first request until released, deaf to cancellation, so the queue's lock stays taken meanwhile.
    sealed class GatedTransport : HttpMessageHandler
    {
        public readonly TaskCompletionSource Started = new(), Release = new();
        int calls;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            if (Interlocked.Increment(ref calls) == 1) { Started.TrySetResult(); await Release.Task; }
            return new HttpResponseMessage(System.Net.HttpStatusCode.ServiceUnavailable);
        }
    }

    sealed class HangingTransport : HttpMessageHandler
    {
        public readonly TaskCompletionSource Started = new();
        public bool Cancelled; public int Requests;
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            Requests++;
            Started.TrySetResult();
            try { await Task.Delay(Timeout.Infinite, ct); }
            catch (OperationCanceledException) { Cancelled = true; throw; }
            throw new InvalidOperationException("unreachable");
        }
    }

    static void RunConsentTests()
    {
        ulong host = 1, a = 2, b = 3, ai = 1000;
        bool Ai(ulong id) => id == ai;
        RunConsent Fresh(params (ulong, bool)[] answers) =>
            new(host, [host, a, b], Ai, answers.ToDictionary(x => x.Item1, x => x.Item2));
        Require(Fresh((host, true), (a, true), (b, true)).Uploadable, "a party that all agreed starts uploadable");
        Require(!Fresh((host, true), (a, true)).Uploadable, "a player who never answered (vanilla) leaves the run non-uploadable");
        var run = Fresh((host, true), (a, true), (b, true));
        int broke = 0;
        run.Broken += _ => broke++;
        run.Observe(a, true);
        Require(run.Uploadable && broke == 0, "a repeated yes changes nothing");
        run.Observe(b, false);
        Require(!run.Uploadable && broke == 1, "a withdrawal mid-run ends upload eligibility");
        run.Observe(b, true);
        Require(!run.Uploadable, "a later yes never restores a broken run");
        foreach (var change in new[] { "leave", "late join", "reconnect" })
        {
            var r = Fresh((host, true), (a, true), (b, true));
            r.MembershipChanged(change);
            Require(!r.Uploadable, "membership change (" + change + ") ends upload eligibility");
        }
        var swapped = Fresh((host, true), (a, true), (b, true));
        swapped.Verify([host, a, 4]);
        Require(!swapped.Uploadable, "a different set of players at completion is not uploadable");
        var same = Fresh((host, true), (a, true), (b, true));
        same.Verify([b, a, host]);
        Require(same.Uploadable, "the same players at completion stay uploadable");
        var unknown = Fresh((host, true), (a, true), (b, true));
        unknown.Observe(99, false);
        Require(unknown.Uploadable, "an answer from a stranger changes nothing");
    }
}
