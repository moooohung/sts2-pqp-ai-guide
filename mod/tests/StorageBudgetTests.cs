using HumanPlayRecorder;

static class StorageBudgetTests
{
    public static async Task Run(RunPayload payload)
    {
        string root = Path.Combine(Path.GetTempPath(), "hpr-budget-" + Guid.NewGuid().ToString("N"));
        var budget = new LocalStorageBudget(root, maxBytes: 16, maxFiles: 2);
        string journal = Path.Combine(root, "incomplete", "test.jsonl");
        await budget.WriteAsync(journal, new byte[8], true);
        await budget.WriteAsync(Path.Combine(root, "pending", "body.rejected"), new byte[8], false);
        bool rejected = false;
        try { await budget.WriteAsync(journal, new byte[1], true); }
        catch (InvalidOperationException e) when (e.Message == "queue_full") { rejected = true; }
        if (!rejected || new FileInfo(journal).Length != 8) throw new Exception("journal bypassed shared cap");
        File.SetLastWriteTimeUtc(Path.Combine(root, "pending", "body.rejected"), DateTime.UtcNow.AddDays(-8));
        await budget.WriteAsync(journal, new byte[1], true);
        if (File.Exists(Path.Combine(root, "pending", "body.rejected"))) throw new Exception("quarantine retention");
        var limited = new LocalStorageBudget(root, maxBytes: 1024, maxFiles: 1);
        // An open journal is never evicted, so the file quota still holds for other writes.
        limited.OpenJournal(journal);
        rejected = false;
        try { await limited.WriteAsync(Path.Combine(root, "pending", "extra"), [0], false); }
        catch (InvalidOperationException) { rejected = true; }
        limited.CloseJournal(journal);
        if (!rejected) throw new Exception("file quota");
        string ackRoot = Path.Combine(Path.GetTempPath(), "hpr-ack-budget-" + Guid.NewGuid().ToString("N"));
        var ackBudget = new LocalStorageBudget(ackRoot);
        string ackJournal = Path.Combine(ackRoot, "incomplete", payload.run_id + ".jsonl");
        await ackBudget.WriteAsync(ackJournal, [0], false);
        await using var queue = new UploadQueue(Path.Combine(ackRoot, "pending"), new QueueHandler { Code = 201, Body = null }, ackBudget);
        await queue.EnqueueAsync(payload);
        if (!File.Exists(ackJournal)) throw new Exception("journal deleted before acknowledgement");
        if (!await queue.AttemptAsync(payload.run_id) || File.Exists(ackJournal)) throw new Exception("ack journal cleanup");
        Console.WriteLine("PASS: shared journal/queue byte+file budget, seven-day quarantine retention, journal deletion only after receipt acknowledgement");
        string reservationRoot=Path.Combine(Path.GetTempPath(),"hpr-reserve-"+Guid.NewGuid().ToString("N"));
        var reservationBudget=new LocalStorageBudget(reservationRoot,20000,2);
        string slot=Path.Combine(reservationRoot,"candidate.receipt");
        await reservationBudget.ReserveReceiptAsync(slot);
        if(new FileInfo(slot).Length!=16384)throw new Exception("receipt capacity not reserved");
        await reservationBudget.FillReceiptAsync(slot,System.Text.Encoding.UTF8.GetBytes("{\"url\":\"candidate\"}"));
        if(new FileInfo(slot).Length!=16384 || !(await File.ReadAllTextAsync(slot)).StartsWith("{\"url\""))
            throw new Exception("reservation was not filled durably in place");
        bool full=false;
        try{await reservationBudget.ReserveReceiptAsync(Path.Combine(reservationRoot,"second.receipt"));}
        catch(InvalidOperationException){full=true;}
        if(!full)throw new Exception("receipt reservations bypassed shared storage limit");
        await StaleJournals(payload);
    }
    // An install left over budget by stale incomplete/<run>.jsonl files (crashed, abandoned,
    // uncertified runs) recovers: they expire, are evicted oldest-first, and never block
    // the open run's journal or the upload schedule. Queued runs keep their journals.
    static async Task StaleJournals(RunPayload payload)
    {
        string root = Path.Combine(Path.GetTempPath(), "hpr-stale-" + Guid.NewGuid().ToString("N"));
        string incomplete = Path.Combine(root, "incomplete");
        Directory.CreateDirectory(incomplete);
        Directory.CreateDirectory(Path.Combine(root, "pending"));
        var now = DateTime.UtcNow;
        string Journal(string id, double ageHours)
        {
            string path = Path.Combine(incomplete, id + ".jsonl");
            File.WriteAllBytes(path, new byte[100]);
            File.SetLastWriteTimeUtc(path, now.AddHours(-ageHours));
            return path;
        }
        string expired = Journal("expired", 24 * 8);
        var recent = Enumerable.Range(0, 6).Select(i => Journal("stale" + i, 60 - i)).ToArray();
        string queuedJournal = Journal(payload.run_id, 100);
        await File.WriteAllBytesAsync(Path.Combine(root, "pending", payload.run_id + ".json"), new byte[100]);
        // 9 files / 900 bytes on disk against 500 bytes and 6 files: over budget before any run starts.
        var budget = new LocalStorageBudget(root, maxBytes: 500, maxFiles: 6);
        await budget.CompactAsync();
        if (File.Exists(expired)) throw new Exception("expired stale journal kept");
        if (!File.Exists(queuedJournal)) throw new Exception("queued run's journal evicted");
        if (recent.Take(3).Any(File.Exists) || !recent.Skip(3).All(File.Exists))
            throw new Exception("stale journals not evicted oldest-first down to the budget");
        // Fill the budget with files that are never evicted, then record a run and persist the schedule.
        string held = Path.Combine(root, "pending", "held.json.oversize");
        await File.WriteAllBytesAsync(held, new byte[400]);
        string active = Guid.NewGuid().ToString();
        var journal = new LocalJournal(incomplete, active, budget);
        for (int i = 0; i < 20; i++) journal.Append(new { kind = "action", seq = i, pad = new string('x', 64) });
        await journal.DisposeAsync();
        if (!journal.Complete || journal.Failure is not null || new FileInfo(Path.Combine(incomplete, active + ".jsonl")).Length < 20 * 64)
            throw new Exception("budget pressure blocked the open run's journal: " + journal.Failure?.Message);
        if (recent.Any(File.Exists)) throw new Exception("open journal did not evict older stale journals first");
        await using (var queue = new UploadQueue(Path.Combine(root, "pending"), new QueueHandler { Code = 503 }, budget))
            await queue.AttemptAsync(payload.run_id);
        if (!File.Exists(Path.Combine(root, "pending", ".upload-schedule.state")))
            throw new Exception("budget pressure blocked the upload schedule state");
        bool full = false;
        try { await budget.WriteAsync(Path.Combine(root, "pending", "other.json"), new byte[100], false); }
        catch (InvalidOperationException e) when (e.Message == "queue_full") { full = true; }
        if (!full) throw new Exception("non-essential write bypassed the budget");
        Console.WriteLine("PASS: stale incomplete journals expire and are evicted oldest-first; queued journals kept; open journal and schedule never blocked");
    }
}
