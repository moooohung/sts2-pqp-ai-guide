using HumanPlayRecorder;

static class QueueResilienceTests
{
    public static async Task Run(RunPayload payload)
    {
        using(var http=new HttpClient(new DownloadFallback())){
            var receipt=new UploadReceipt("","","https://files.catbox.moe/test.gz",
                Convert.ToHexStringLower(System.Security.Cryptography.SHA256.HashData(new byte[]{1,2,3})),3,"","","",10,1);
            var bytes=await RecordingDownload.Fetch(http,receipt);
            if(!bytes.SequenceEqual(new byte[]{1,2,3}))throw new Exception("proxy fallback bytes");
        }
        foreach(string invalid in new[]{"http://files.catbox.moe/x.gz","https://files.catbox.moe.evil/x.gz","https://files.catbox.moe/a/../x.gz","https://files.catbox.moe/x.gz?q=1"})
        {bool denied=false;try{RecordingDownload.URLs(invalid);}catch(ArgumentException){denied=true;}if(!denied)throw new Exception("download allowlist");}
        var safe=UploadQueue.TransportDiagnostic(new HttpRequestException("An error occurred while sending the request.",
            new IOException("The response ended prematurely. (ResponseEnded)")));
        var privateError=UploadQueue.TransportDiagnostic(new IOException(@"C:\Users\private-user\record.json"));
        if(!safe.Contains("HttpRequestException: An error occurred")||!safe.Contains("IOException: The response ended prematurely.")||
            privateError.Contains("private-user")||privateError.Contains('\\'))throw new Exception("transport diagnostics privacy");
        string recovery=Path.Combine(Path.GetTempPath(),"hpr-recovery-"+Guid.NewGuid().ToString("N"));
        var recoveryClock=DateTimeOffset.UtcNow;
        var recoveryHandler=new QueueHandler{Code=201,Body=null};
        await using(var q=new UploadQueue(recovery,recoveryHandler,utcNow:()=>recoveryClock))
        {
            await q.EnqueueAsync(payload);
            string slot=Path.Combine(recovery,payload.run_id+".json.receipt");
            await File.WriteAllBytesAsync(slot,new byte[16384]);
            await q.AttemptAsync(payload.run_id);
            if(recoveryHandler.CatboxCalls!=0||await q.RecoverUnacceptedReservationAsync(payload.run_id,false))
                throw new Exception("ambiguous reservation automatically retried");
            if(!await q.RecoverUnacceptedReservationAsync(payload.run_id,true))
                throw new Exception("confirmed zero reservation not recovered");
            if(await q.AttemptAsync(payload.run_id)||recoveryHandler.CatboxCalls!=0)
                throw new Exception("recovery bypassed shared backoff");
            recoveryClock=recoveryClock.AddHours(7);
            if(!await q.AttemptAsync(payload.run_id)||recoveryHandler.CatboxCalls!=1||recoveryHandler.ReceiptCalls!=1||
                await q.RecoverUnacceptedReservationAsync(payload.run_id,true))
                throw new Exception("confirmed recovery did not upload exactly once/cleanup");
        }
        Console.WriteLine("PASS: fixed User-Agent on both endpoints; private transport diagnostics; explicit zero-reservation recovery preserves backoff");
        string failed=Path.Combine(Path.GetTempPath(),"hpr-transport-"+Guid.NewGuid().ToString("N"));
        await using(var q=new UploadQueue(failed,new FailingTransport()))
        {
            await q.EnqueueAsync(payload);
            // Catbox and the server copy both fail in transport: the body stays queued and the
            // reservation is released, so a later attempt uploads again instead of stalling.
            if(await q.AttemptAsync(payload.run_id)||q.LastDiagnostic is null||
                !q.LastDiagnostic.Contains("HttpRequestException: An error occurred")||
                File.Exists(Path.Combine(failed,payload.run_id+".json.receipt"))||
                !File.Exists(Path.Combine(failed,payload.run_id+".json")))
                throw new Exception("contained transport failure stranded the recording or lost its diagnostic");
        }
        foreach(string ambiguous in new[]{"reservation","corrupt","conflict"})
        {
            string fair=Path.Combine(Path.GetTempPath(),"hpr-fair-"+Guid.NewGuid().ToString("N"));
            var time=DateTimeOffset.UtcNow;
            var handler=new QueueHandler{Code=ambiguous=="conflict"?409:201,Body=null};
            string first="00000000-0000-4000-8000-000000000001";
            string later="ffffffff-ffff-4fff-8fff-ffffffffffff";
            await using(var q=new UploadQueue(fair,handler,utcNow:()=>time,jitter:()=>0.5))
            {
                await q.EnqueueAsync(payload with{run_id=first});
                await q.EnqueueAsync(payload with{run_id=later});
                if(ambiguous!="conflict")
                    await File.WriteAllBytesAsync(Path.Combine(fair,first+".json.receipt"),
                        ambiguous=="reservation"?new byte[16384]:System.Text.Encoding.UTF8.GetBytes("{}"));
                await q.AttemptAsync(first);
            }
            handler.Code=201;time=time.AddHours(7);
            await using(var restarted=new UploadQueue(fair,handler,utcNow:()=>time,jitter:()=>0.5))
            {
                // Load persisted cursor without retrying the ambiguous item.
                await restarted.ScanOnceAsync();
            }
            if(!File.Exists(Path.Combine(fair,first+".json"))||File.Exists(Path.Combine(fair,later+".json")))
                throw new Exception("ambiguous item starved later run: "+ambiguous);
        }
        string conflicts=Path.Combine(Path.GetTempPath(),"hpr-conflict-"+Guid.NewGuid().ToString("N"));
        var conflictTransport=new QueueHandler{Code=409};
        await using(var conflictQueue=new UploadQueue(conflicts,conflictTransport))
        {
            await conflictQueue.EnqueueAsync(payload);
            if(await conflictQueue.AttemptAsync(payload.run_id) ||
                !File.Exists(Path.Combine(conflicts,payload.run_id+".json")) ||
                !File.Exists(Path.Combine(conflicts,payload.run_id+".json.receipt")))
                throw new Exception("nonmatching duplicate/conflict lost local retry state");
            if(await conflictQueue.RecoverUnacceptedReservationAsync(payload.run_id,true))
                throw new Exception("filled conflict receipt reset by zero-reservation recovery");
        }
        string root = Path.Combine(Path.GetTempPath(), "hpr-resilience-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        string blocked = Path.Combine(root, "pending");
        await File.WriteAllTextAsync(blocked, "not a directory");
        var transport = new QueueHandler { Code = 201, Body = null };
        var clock=DateTimeOffset.UtcNow;
        await using (var queue = new UploadQueue(blocked, transport,utcNow:()=>clock))
        {
            if (await queue.ScanOnceAsync()) throw new Exception("scan failure hidden");
            File.Delete(blocked); // Only this test's own blocking file.
            Directory.CreateDirectory(blocked);
            await File.WriteAllTextAsync(Path.Combine(blocked, "invalid-id.json"), "{}");
            await queue.EnqueueAsync(payload);
            if (await queue.AttemptAsync(Guid.NewGuid().ToString())) throw new Exception("missing file accepted");
            clock=clock.AddHours(1);
            await queue.ScanOnceAsync();
            if (File.Exists(Path.Combine(blocked, payload.run_id + ".json")))
                throw new Exception("bad item or prior filesystem error killed later scan");
        }
        foreach (int status in new[] { 429, 503 })
        {
            var throttled = new QueueHandler { Code = status, RetrySeconds = 60 };
            await using var queue = new UploadQueue(Path.Combine(root, status.ToString()), throttled);
            await queue.EnqueueAsync(payload);
            if (await queue.AttemptAsync(payload.run_id)) throw new Exception("throttle accepted");
            throttled.Code = 201; throttled.Body = null;
            int calls = throttled.ReceiptCalls;
            if (await queue.AttemptAsync(payload.run_id) || throttled.ReceiptCalls != calls)
                throw new Exception("Retry-After not honored");
        }
        var corruptTransport = new QueueHandler { Code = 201, Body = null };
        string corruptDir = Path.Combine(root, "corrupt-receipt");
        await using (var queue = new UploadQueue(corruptDir, corruptTransport))
        {
            await queue.EnqueueAsync(payload);
            await File.WriteAllTextAsync(Path.Combine(corruptDir, payload.run_id + ".json.receipt"),
                """{"catbox_url":"https://files.catbox.moe/abc123.gz","sha256":null,"bytes":10}""");
            if (await queue.AttemptAsync(payload.run_id) || corruptTransport.ReceiptCalls != 0 ||
                !File.Exists(Path.Combine(corruptDir, payload.run_id + ".json")))
                throw new Exception("corrupt receipt caused upload or deletion");
        }
        Console.WriteLine("PASS: filesystem scan/item recovery, later item progress, 429 and 503 Retry-After");
        string persisted=Path.Combine(root,"persisted");
        var retryTransport=new QueueHandler{Code=503};
        await using(var first=new UploadQueue(persisted,retryTransport,utcNow:()=>clock,jitter:()=>0.5))
        {await first.EnqueueAsync(payload);await first.AttemptAsync(payload.run_id);}
        int persistedCalls=retryTransport.ReceiptCalls;
        await using(var restarted=new UploadQueue(persisted,retryTransport,utcNow:()=>clock,jitter:()=>0.5))
        {await restarted.AttemptAsync(payload.run_id);}
        if(retryTransport.ReceiptCalls!=persistedCalls)throw new Exception("restart ignored shared backoff");
        string noState=Path.Combine(root,"no-state");
        var noNetwork=new QueueHandler{Code=201,Body=null};
        var tight=new LocalStorageBudget(noState,maxBytes:4096,maxFiles:8);
        await using(var limited=new UploadQueue(noState,noNetwork,tight))
        {await limited.EnqueueAsync(payload);await limited.AttemptAsync(payload.run_id);}
        if(noNetwork.CatboxCalls!=0)throw new Exception("Catbox called without reserved state capacity");
        Console.WriteLine("PASS: persisted shared retry schedule, storage reservation precedes Catbox");
        foreach(var invalid in new[]{
            payload with{environment_mods=Enumerable.Range(0,129).Select(i=>new EnvironmentMod("mod"+i,"1")).ToArray()},
            payload with{environment_mods=[new("x",new string('한',43))]},
            payload with{patch_audit=[new("x",new string('m',2049),"prefix")]},
            payload with{patch_audit=Enumerable.Repeat(new PatchAuditEntry("x","m","prefix"),16385).ToArray()},
            payload with{environment_mods=Enumerable.Range(0,128).Select(i=>new EnvironmentMod(i.ToString()+new string('x',120),new string('v',128))).ToArray()}
        })
        {
            string dir=Path.Combine(root,Guid.NewGuid().ToString("N"));Directory.CreateDirectory(dir);
            await File.WriteAllBytesAsync(Path.Combine(dir,invalid.run_id+".json"),
                System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(invalid,RecordingFormat.Json));
            var handler=new QueueHandler{Code=201,Body=null};
            await using var q=new UploadQueue(dir,handler);
            await q.AttemptAsync(invalid.run_id);
            if(handler.CatboxCalls!=0)throw new Exception("invalid prospective metadata contacted Catbox");
        }
        Console.WriteLine("PASS: metadata and prospective receipt budgets checked before Catbox");
        string numericDir=Path.Combine(root,"numeric-seed");
        await using(var q=new UploadQueue(numericDir,new QueueHandler{Code=201,Body=null}))
        {
            await q.EnqueueAsync(payload with{input_seed="synthetic-private-seed-do-not-publish"});
            string json=await File.ReadAllTextAsync(Path.Combine(numericDir,payload.run_id+".json"));
            if(json.Contains("synthetic-private-seed")||
                System.Text.Json.JsonSerializer.Deserialize<RunPayload>(json,RecordingFormat.Json)!.input_seed!=payload.seed)
                throw new Exception("raw custom seed persisted");
        }
    }
    sealed class FailingTransport:HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken ct)
        {
            if(request.Headers.UserAgent.ToString()!=UploadQueue.UserAgent)throw new Exception("missing User-Agent");
            throw new HttpRequestException("An error occurred while sending the request.",
                new IOException("The response ended prematurely. (ResponseEnded)"));
        }
    }
    sealed class DownloadFallback:HttpMessageHandler
    {
        int calls;
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request,CancellationToken ct)
        {
            string expected=++calls==1?"https://mirror.example.org/test.gz":"https://files.catbox.moe/test.gz";
            if(calls>2||request.RequestUri!.AbsoluteUri!=expected)throw new Exception("download order");
            return Task.FromResult(new HttpResponseMessage(calls==1?System.Net.HttpStatusCode.BadGateway:System.Net.HttpStatusCode.OK)
                {Content=new ByteArrayContent([1,2,3])});
        }
    }
}
