using HumanPlayRecorder;

static class StorageSafetyTests
{
    // A run recorded and ended through the live session and journal, handed off to `queue`.
    static async Task<string> FinishRun(string root, UploadQueue queue, LocalStorageBudget budget, RunPayload payload,
        Action<RunHeader>? setup = null)
    {
        string id = Guid.NewGuid().ToString();
        var h = new RunHeader(id, payload.mod_version, payload.game_version, "", payload.character,
            payload.seed, [], payload.started_at, payload.mode, payload.party_size, payload.local_seat, payload.party,
            payload.shared_run_id) { ascension = payload.ascension };
        setup?.Invoke(h);
        var j = new LocalJournal(Path.Combine(root, "incomplete"), id, budget);
        j.Append(new { kind = "entry", run_id = id, character = payload.character, ascension = payload.ascension,
            party_size = payload.party_size });
        var session = new LiveRunSession(h, typeof(RunPayload).Assembly.Location, [], queue);
        foreach (var row in payload.actions) session.Add(row);
        foreach (var floor in payload.floor_digests) session.Floor(floor.state, floor.after_action);
        session.End(j, payload.result, payload.ended_at, true);
        await session.Completion;
        return id;
    }

    public static async Task CapacityWarning(RunPayload payload)
    {
        string root = Directory.CreateTempSubdirectory("hpr-capacity-warning-").FullName;
        try
        {
            var budget = new LocalStorageBudget(root);
            string pending = Path.Combine(root, "pending"), incomplete = Path.Combine(root, "incomplete");
            string excluded;
            await using (var queue = new UploadQueue(pending, new QueueHandler(), budget))
            {
                string queued = await FinishRun(root, queue, budget, payload);
                string oversize = await FinishRun(root, queue, budget,
                    payload with { game_version = new string('x', UploadQueue.MaxBodyBytes) });
                excluded = await FinishRun(root, queue, budget, payload, h => h.Admission.Reject());
                if (!File.Exists(Path.Combine(pending, queued + ".json")) ||
                    !File.Exists(Path.Combine(pending, oversize + ".json.oversize")))
                    throw new Exception("test runs were not handed off");
            }
            string plain = Path.Combine(incomplete, excluded + ".jsonl");
            if (!File.Exists(plain) || LocalStorageBudget.HasRetentionFooter(plain) || File.Exists(plain + ".retained"))
                throw new Exception("a run that can never upload was retained beyond reclamation");
            bool warned = false;
            await UploadQueue.RecoverRetainedAsync(budget, () => warned = true);
            if (warned) throw new Exception("launch warned about runs waiting in pending/ or kept as oversize");
            File.WriteAllText(Path.Combine(incomplete, Guid.NewGuid() + ".jsonl.retained"), "{\"kind\":\"entry\"}\n");
            await UploadQueue.RecoverRetainedAsync(budget, () => warned = true);
            if (!warned) throw new Exception("a retained journal with no payload in any queue raised no warning");
            Console.WriteLine("PASS: storage warning ignores queued, oversize and excluded runs; flags payload-less retained journals");
        }
        finally { Directory.Delete(root, true); }
    }

    public static async Task LocalDelete(RunPayload payload)
    {
        string root = Directory.CreateTempSubdirectory("hpr-local-delete-").FullName;
        try
        {
            var budget = new LocalStorageBudget(root);
            string pending = Path.Combine(root, "pending"), incomplete = Path.Combine(root, "incomplete");
            var http = new QueueHandler();
            await using var queue = new UploadQueue(pending, http, budget);
            string queued = await FinishRun(root, queue, budget, payload);
            string oversize = await FinishRun(root, queue, budget,
                payload with { game_version = new string('x', UploadQueue.MaxBodyBytes) });
            string uncertified;
            await using (var other = new UploadQueue(Path.Combine(root, "uncertified"), new QueueHandler(), budget))
                uncertified = await FinishRun(root, other, budget, payload);
            string orphan = Guid.NewGuid().ToString();
            string orphanJournal = Path.Combine(incomplete, orphan + ".jsonl");
            File.WriteAllText(orphanJournal + ".retained", "{\"kind\":\"entry\",\"character\":\"SILENT\"}\n");
            // Never listed or touched: the run being recorded, another folder's cache, install settings.
            var active = new LocalJournal(incomplete, Guid.NewGuid().ToString(), budget);
            active.Append(new { kind = "entry" });
            string cache = Path.Combine(root, "policy-cache", new string('a', 64) + ".safetensors");
            Directory.CreateDirectory(Path.GetDirectoryName(cache)!);
            File.WriteAllText(cache, "cache");
            string settings = Path.Combine(root, "replay-key.txt");
            File.WriteAllText(settings, "key");

            var listed = budget.ListRuns().ToDictionary(r => r.RunId);
            if (listed.Count != 4 || listed[queued].State != LocalRunState.Pending ||
                listed[oversize].State != LocalRunState.Oversize || listed[uncertified].State != LocalRunState.Uncertified ||
                listed[orphan].State != LocalRunState.Retained || listed[queued].Character != payload.character ||
                listed[orphan].Character != "SILENT" || listed[oversize].Bytes <= UploadQueue.MaxBodyBytes)
                throw new Exception("local storage list does not show each waiting run once with its state");
            if (await queue.DeleteLocalAsync([queued]) != 0 ||
                Directory.GetFiles(root, queued + "*", SearchOption.AllDirectories).Length != 0 ||
                budget.ListRuns().Count != 3)
                throw new Exception("per-run delete left the run's body, sidecar or journal, or touched other runs");
            budget.OpenJournal(orphanJournal); // its handoff is running
            if (await queue.DeleteLocalAsync([orphan]) != 1 || !File.Exists(orphanJournal + ".retained"))
                throw new Exception("per-run delete tore a journal owned by a running handoff");
            budget.CloseJournal(orphanJournal);
            if (await queue.DeleteLocalAsync(budget.ListRuns().Select(r => r.RunId).ToList()) != 0 ||
                budget.ListRuns().Count != 0)
                throw new Exception("clearing the local queue left runs behind");
            active.Append(new { still = "recording" });
            await active.DisposeAsync();
            if (!File.Exists(cache) || !File.Exists(settings) || File.Exists(Path.Combine(pending, ".delete-barrier")) ||
                http.CatboxCalls + http.ServerCopyCalls + http.ReceiptCalls + http.RunCalls != 0 ||
                !Directory.GetFiles(incomplete, "*.jsonl").Any(p => File.ReadAllText(p).Contains("recording")))
                throw new Exception("local clear reached the server, the delete-all cutoff, other files or the run being recorded");
            Console.WriteLine("PASS: local storage lists waiting runs, deletes one or all on this PC only, never an owned journal");
        }
        finally { Directory.Delete(root, true); }
    }

    public static void NoticeDeferral()
    {
        var notice = new StorageNotice();
        int drawn = 0;
        if (notice.Request()) throw new Exception("storage note scheduled before the main menu exists");
        if (!notice.MenuReady()) throw new Exception("main menu did not pick up the earlier request");
        notice.Show(() => false); // no scene tree yet
        if (!notice.Request()) throw new Exception("a note that could not be drawn was lost for the session");
        notice.Show(() => { drawn++; return true; });
        notice.Show(() => { drawn++; return true; });
        if (notice.Request() || drawn != 1) throw new Exception("storage note not shown exactly once");
        Console.WriteLine("PASS: storage note waits for the main menu and survives a draw without a scene tree");
    }

    public static async Task RetryHandoff(RunPayload payload)
    {
        string root = Directory.CreateTempSubdirectory("hpr-retry-handoff-").FullName;
        try
        {
            string pending = Path.Combine(root, "pending");
            Directory.CreateDirectory(pending);
            for (int i = 0; i < 100; i++) File.WriteAllText(Path.Combine(pending, $"{i:D3}.json"), "{}");
            var budget = new LocalStorageBudget(root, maxBytes: 1, maxFiles: 1);
            string id = Guid.NewGuid().ToString();
            var h = new RunHeader(id, payload.mod_version, payload.game_version, "", payload.character,
                payload.seed, [], payload.started_at, "solo", 1, 0, payload.party, payload.shared_run_id)
                { ascension = payload.ascension };
            var j = new LocalJournal(Path.Combine(root, "incomplete"), id, budget);
            j.Append(new { sentinel = id });
            bool warned = false;
            await using (var blocked = new UploadQueue(pending, new QueueHandler(), budget)
                { CapacityWarning = () => warned = true })
            {
                var session = new LiveRunSession(h, typeof(RunPayload).Assembly.Location, [], blocked);
                foreach (var row in payload.actions) session.Add(row);
                foreach (var floor in payload.floor_digests) session.Floor(floor.state, floor.after_action);
                session.End(j, payload.result, payload.ended_at, true);
                await session.Completion;
                if (session.Failure is not InvalidOperationException { Message: "queue_full" })
                    throw new Exception("test did not fill the queue");
                if (!warned) throw new Exception("retained queue capacity did not surface the player-warning callback");
            }
            File.Delete(Path.Combine(pending, "000.json")); // This test's own queue slot.
            await using var restarted = new UploadQueue(pending, new QueueHandler(), new LocalStorageBudget(root, 1, 1));
            await restarted.ScanOnceAsync();
            string body = Path.Combine(pending, id + ".json");
            if (!File.Exists(body))
                throw new Exception("restart scan never retried the retained queue handoff");
            var recovered = System.Text.Json.JsonSerializer.Deserialize<RunPayload>(File.ReadAllBytes(body), RecordingFormat.Json)!;
            if (recovered.run_id != id ||
                System.Text.Json.JsonSerializer.Serialize(recovered.actions) != System.Text.Json.JsonSerializer.Serialize(payload.actions) ||
                File.Exists(restarted.RetainedPayloadPath(id)))
                throw new Exception("retry did not atomically transfer the original payload");
            Console.WriteLine("PASS: restart scan retries failed handoff at full shared budget without payload loss");
        }
        finally { Directory.Delete(root, true); }
    }

    public static async Task DeleteOrphans()
    {
        string root = Directory.CreateTempSubdirectory("hpr-delete-orphans-").FullName;
        try
        {
            var budget = new LocalStorageBudget(root);
            string incomplete = Path.Combine(root, "incomplete");
            Directory.CreateDirectory(incomplete);
            string orphan = Path.Combine(incomplete, Guid.NewGuid() + ".jsonl.retained");
            await File.WriteAllTextAsync(orphan, "failed before queue creation");
            var active = new LocalJournal(incomplete, Guid.NewGuid().ToString(), budget);
            active.Append(new { private_data = "active capture" });
            await active.FinishForHandoffAsync(); // ownership still held: queue handoff has not finished
            await using var queue = new UploadQueue(Path.Combine(root, "pending"), new QueueHandler(), budget);
            await queue.PurgeAllAsync(DateTimeOffset.UtcNow.AddSeconds(1));
            await active.DisposeAsync();
            if (Directory.GetFiles(incomplete).Any(p => !p.EndsWith(".lock", StringComparison.Ordinal)))
                throw new Exception("delete-all left orphaned or concurrently finishing journal data");
            Console.WriteLine("PASS: delete-all removes orphaned retained data and a concurrent active handoff");
        }
        finally { Directory.Delete(root, true); }
    }

    public static async Task RetentionFailure()
    {
        string root = Directory.CreateTempSubdirectory("hpr-retention-conflict-").FullName;
        try
        {
            string incomplete = Path.Combine(root, "incomplete");
            Directory.CreateDirectory(incomplete);
            string id = Guid.NewGuid().ToString();
            string path = Path.Combine(incomplete, id + ".jsonl");
            // This is a real File.Move collision, not an injected exception.
            await File.WriteAllTextAsync(path + ".retained", "existing retained copy");
            var journal = new LocalJournal(incomplete, id, new LocalStorageBudget(root));
            journal.Append(new { sentinel = id });
            try { await journal.RetainAsync(); }
            catch (IOException) { }
            await journal.DisposeAsync();
            await new LocalStorageBudget(root, maxBytes: 1, maxFiles: 1).CompactAsync();
            if (!File.Exists(path) || !(await File.ReadAllTextAsync(path)).Contains(id))
                throw new Exception("retention rename conflict released an evictable completed journal");
            if (await File.ReadAllTextAsync(path + ".retained") != "existing retained copy")
                throw new Exception("retention overwrote the prior durable copy");
            Console.WriteLine("PASS: retention rename collision cannot expose the only completed journal to reclamation");
        }
        finally { Directory.Delete(root, true); }
    }

    public static async Task HoldJournal(string root, string id)
    {
        var budget = new LocalStorageBudget(root, maxBytes: 1);
        string path = Path.Combine(root, "incomplete", id + ".jsonl");
        budget.OpenJournal(path);
        try
        {
            await budget.WriteAsync(path, System.Text.Encoding.UTF8.GetBytes("first\n"), false);
            Console.WriteLine("READY");
            if (await Console.In.ReadLineAsync() != "APPEND") throw new Exception("missing append signal");
            await budget.WriteAsync(path, System.Text.Encoding.UTF8.GetBytes("last\n"), true);
        }
        finally { budget.CloseJournal(path); }
    }

    public static async Task Ownership()
    {
        string root = Directory.CreateTempSubdirectory("hpr-cross-process-").FullName;
        var start = new System.Diagnostics.ProcessStartInfo("dotnet")
        {
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            UseShellExecute = false
        };
        string id = Guid.NewGuid().ToString();
        start.ArgumentList.Add(typeof(StorageSafetyTests).Assembly.Location);
        start.ArgumentList.Add("--hold-journal");
        start.ArgumentList.Add(root);
        start.ArgumentList.Add(id);
        using var child = System.Diagnostics.Process.Start(start)!;
        try
        {
            if (await child.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(30)) != "READY")
                throw new Exception("journal owner did not become ready");
            // The owner has closed the data stream, but not its journal lifecycle.
            var other = new LocalStorageBudget(root, maxBytes: 1, maxFiles: 1);
            await other.CompactAsync();
            await other.WriteAsync(Path.Combine(root, "pending", ".upload-schedule.state"), [1], false, essential: true);
            string journal = Path.Combine(root, "incomplete", id + ".jsonl");
            if (!File.Exists(journal) || await File.ReadAllTextAsync(journal) != "first\n")
                throw new Exception("second process reclaimed the first process's active journal");
            await child.StandardInput.WriteLineAsync("APPEND");
            await child.StandardInput.FlushAsync();
            await child.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(30));
            if (child.ExitCode != 0) throw new Exception(await child.StandardError.ReadToEndAsync());
            if (await File.ReadAllTextAsync(journal) != "first\nlast\n")
                throw new Exception("later append recreated a truncated journal");
            await other.CompactAsync();
            if (File.Exists(journal)) throw new Exception("closed abandoned journal could not be reclaimed");
            Console.WriteLine("PASS: cross-process ownership prevents reclamation/truncation between writes and releases on close");
        }
        finally
        {
            if (!child.HasExited) { child.Kill(entireProcessTree: true); await child.WaitForExitAsync(); }
            Directory.Delete(root, true);
        }
    }

    public static async Task Reservation(RunPayload payload)
    {
        string root = Directory.CreateTempSubdirectory("hpr-drain-pressure-").FullName;
        try
        {
            string pending = Path.Combine(root, "pending");
            var now = DateTimeOffset.UtcNow;
            var mp = payload with { run_id = Guid.NewGuid().ToString(), mode = "multiplayer",
                party_size = 2, local_seat = 1, character = "SILENT",
                party = [new(0, "IRONCLAD"), new(1, "SILENT")],
                game_dll_sha256 = GameBuild.Sha256, training = new(GameBuild.VanillaFingerprint, "trainable"),
                started_at = now.ToString("O"), ended_at = now.ToString("O") };
            await using (var producer = new UploadQueue(pending, new QueueHandler(), new LocalStorageBudget(root)))
                await producer.EnqueueAsync(mp);
            var http = new QueueHandler { Code = 503 };
            var pressure = new LocalStorageBudget(root, maxBytes: 1, maxFiles: 1);
            await using var drain = new UploadQueue(pending, http, pressure, utcNow: () => now, jitter: () => 0.5)
            {
                StampReceipt = r => UploadPolicy.Flags(r with { owner = new string('a', 64) }, true, true)
            };
            if (await drain.AttemptAsync(mp.run_id) || http.ServerCopyCalls != 1 || http.ReceiptCalls != 1)
                throw new Exception("full budget blocked the actual multiplayer upload: " + drain.LastDiagnostic);
            string receipt = Path.Combine(pending, mp.run_id + ".json.receipt");
            if (new FileInfo(receipt).Length != 16384 ||
                !File.ReadAllText(receipt).TrimEnd().EndsWith("}"))
                throw new Exception("upload did not preserve a filled crash-safe receipt reservation");
            now = now.AddHours(7);
            http.Code = 201;
            http.Body = null;
            if (!await drain.AttemptAsync(mp.run_id) || http.ServerCopyCalls != 1 ||
                http.ReceiptCalls != 2 || File.Exists(receipt) ||
                File.Exists(Path.Combine(pending, mp.run_id + ".json")))
                throw new Exception("saved receipt could not drain the queue under pressure");
            Console.WriteLine("PASS: full-budget MP upload reserves/fills receipt, survives retry, and drains without re-upload");
        }
        finally { Directory.Delete(root, true); }
    }
    public static async Task Oversize(RunPayload payload)
    {
        string root = Directory.CreateTempSubdirectory("hpr-oversize-retention-").FullName;
        try
        {
            string pending = Path.Combine(root, "pending");
            string incomplete = Path.Combine(root, "incomplete");
            Directory.CreateDirectory(incomplete);
            var large = payload with { run_id = Guid.NewGuid().ToString(),
                game_version = new string('x', UploadQueue.MaxBodyBytes) };
            await using (var queue = new UploadQueue(pending, new QueueHandler(), new LocalStorageBudget(root)))
                await queue.EnqueueAsync(large);
            string body = Path.Combine(pending, large.run_id + ".json.oversize");
            string journal = Path.Combine(incomplete, large.run_id + ".jsonl");
            await File.WriteAllTextAsync(journal, "only original journal");
            File.SetLastWriteTimeUtc(body, DateTime.UtcNow.AddDays(-30));
            File.SetLastWriteTimeUtc(journal, DateTime.UtcNow.AddDays(-30));
            long size = new FileInfo(body).Length;
            var pressure = new LocalStorageBudget(root, maxBytes: 1, maxFiles: 1);
            await pressure.CompactAsync();
            if (!File.Exists(body) || !File.Exists(journal))
                throw new Exception("compaction expired the oversize payload or its journal");
            await pressure.WriteAsync(Path.Combine(pending, ".upload-schedule.state"), [1], false, essential: true);
            await using (var next = new LocalJournal(incomplete, Guid.NewGuid().ToString(), pressure))
                next.Append(new { action = "next run" });
            await pressure.CompactAsync();
            if (new FileInfo(body).Length != size || await File.ReadAllTextAsync(journal) != "only original journal")
                throw new Exception("later writes reclaimed an oversize capture");
            Console.WriteLine("PASS: aged oversize payload and legacy journal survive compaction and later writes");
        }
        finally { Directory.Delete(root, true); }
    }
    public static async Task Handoff(RunPayload payload)
    {
        string root = Directory.CreateTempSubdirectory("hpr-handoff-").FullName;
        try
        {
            var budget = new LocalStorageBudget(root, maxBytes: 128, maxFiles: 10);
            string pending = Path.Combine(root, "pending");
            Directory.CreateDirectory(pending);
            // No reclaimable space remains, and queue slots are independently exhausted.
            await File.WriteAllBytesAsync(Path.Combine(pending, "protected.json"), new byte[256]);
            for (int i = 0; i < 99; i++) File.WriteAllText(Path.Combine(pending, $"{i:D3}.json"), "{}");
            string id = Guid.NewGuid().ToString();
            var header = new RunHeader(id, payload.mod_version, payload.game_version, "",
                payload.character, payload.seed, [], payload.started_at, payload.mode,
                payload.party_size, payload.local_seat, payload.party, payload.shared_run_id)
                { ascension = payload.ascension };
            await using var queue = new UploadQueue(pending, new QueueHandler(), budget);
            var journal = new LocalJournal(Path.Combine(root, "incomplete"), id, budget);
            journal.Append(new { durable_sentinel = id });
            var session = new LiveRunSession(header, typeof(RunPayload).Assembly.Location, [], queue);
            foreach (var row in payload.actions) session.Add(row);
            foreach (var floor in payload.floor_digests) session.Floor(floor.state, floor.after_action);
            session.End(journal, payload.result, payload.ended_at, complete: true);
            await session.Completion;
            if (session.Failure is not InvalidOperationException { Message: "queue_full" })
                throw new Exception("handoff did not exercise the full-budget queue failure: " + session.Failure);
            // A fresh budget models the next launch, not the process-local open-journal set.
            await new LocalStorageBudget(root, maxBytes: 1, maxFiles: 1).CompactAsync();
            if (!Directory.GetFiles(Path.Combine(root, "incomplete"))
                    .Any(p => File.ReadAllText(p).Contains(id)))
                throw new Exception("failed durable queue handoff lost the only journal copy");
            Console.WriteLine("PASS: failed full-budget terminal handoff retains the only journal across compaction");
        }
        finally { Directory.Delete(root, true); }
    }
}
