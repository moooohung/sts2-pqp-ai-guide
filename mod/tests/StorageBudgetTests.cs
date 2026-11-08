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
        rejected = false;
        try { await limited.WriteAsync(Path.Combine(root, "pending", "extra"), [0], false); }
        catch (InvalidOperationException) { rejected = true; }
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
    }
}
