using System.IO.Compression;
using System.Net;
using System.Net.Http.Headers;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace HumanPlayRecorder;

public sealed record UploadReceipt(string schema, string action_schema, string catbox_url,
    string sha256, long bytes, string game_version, string game_dll_sha256,
    string character, int ascension, int party_size, EnvironmentMod[]? active_mods=null,
    string? vanilla_fingerprint=null,string? decision=null,PatchAuditSummary? patch_audit=null,
    // Replay fields (ReplayLinks): omitted, not null, so older receipts keep their exact bytes.
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] string? owner=null,
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] bool? public_replay=null,
    // Sharing choices (UploadPolicy): training = the player agreed that the recording may train the AI,
    // replay = the run may be listed in the player's replays. Always present on new receipts.
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] bool? training=null,
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] bool? replay=null,
    // Multiplayer runs: every participant's own install uploads its own recording and receipt. run = the
    // recording's shared run id, seat = this install's seat. The server counts the run only when all seats
    // have their own agreeing receipt; nothing the game or the host says about other players is trusted.
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] string? run=null,
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] int? seat=null,
    // When the recorded run began (unix seconds). After "delete all" the server refuses receipts of runs that
    // began before the deletion, so one still in flight cannot bring deleted data back.
    [property: System.Text.Json.Serialization.JsonIgnore(Condition=System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull)] long? started=null);

// A run's recording goes up when the player allowed training OR keeps replay saving on; both off
// means nothing leaves the PC. Training consent alone decides whether the server may train on it.
public static class UploadPolicy
{
    public static bool Allowed(bool training,bool replay)=>training||replay;
    // The receipt for the player's current choices; null while there is no durable install key,
    // because an upload nobody can delete is not made.
    public static UploadReceipt? Stamp(UploadReceipt r,string? owner,bool publicChoice,bool training,bool replay)=>
        owner is null?null:Flags(r with{owner=owner,public_replay=publicChoice?true:null},training,replay);
    // A replay link is announced only for a listed replay of this install.
    public static bool Announces(UploadReceipt r,string? installOwner)=>
        r.replay!=false&&r.owner is not null&&r.owner==installOwner;
    // The owner key hash stays on every receipt so "delete all my data" reaches uploads that are not
    // listed; an unlisted replay carries no public choice. A multiplayer run leaves the PC only while
    // this player's training consent still stands (PartyConsent).
    public static UploadReceipt Flags(UploadReceipt r,bool training,bool replay)
    {
        if(r.party_size>1&&!training)replay=false;
        return r with{training=training,replay=replay,public_replay=replay?r.public_replay:null};
    }
}

public static class UploadMetadata
{
    public const int MaxPatches=16384;
    public static string Seed(string seed)
    {
        if(!ulong.TryParse(seed,System.Globalization.NumberStyles.None,System.Globalization.CultureInfo.InvariantCulture,out var n)||
            n.ToString(System.Globalization.CultureInfo.InvariantCulture)!=seed)throw new ArgumentException("numeric_seed");
        return seed;
    }
    static void Text(string value,int bytes)
    {
        if(string.IsNullOrEmpty(value)||value.IndexOfAny(['\r','\n','\0'])>=0||
            new UTF8Encoding(false,true).GetByteCount(value)>bytes)throw new ArgumentException("metadata_limit");
    }
    public static void Validate(EnvironmentMod[] mods,PatchAuditEntry[]? patches)
    {
        if(mods.Length>128||mods.Select(m=>m.id).Distinct(StringComparer.Ordinal).Count()!=mods.Length)
            throw new ArgumentException("metadata_mods");
        foreach(var m in mods){Text(m.id,128);Text(m.version,128);}
        if(patches is null)return;
        if(patches.Length>MaxPatches)throw new ArgumentException("metadata_patches");
        foreach(var p in patches)
        {
            Text(p.mod_id,128);Text(p.method,2048);
            if(p.kind is not ("prefix" or "postfix" or "transpiler" or "finalizer"))
                throw new ArgumentException("metadata_patch_kind");
        }
    }
    public static EnvironmentMod[] Mods(RunPayload p)=>p.environment_mods??
        p.active_mods.Select(m=>new EnvironmentMod(m.id,m.version)).Distinct().OrderBy(m=>m.id,StringComparer.Ordinal).ToArray();
    public static void Receipt(UploadReceipt r)
    {
        if(r.active_mods is null||r.schema is not ("sts2-human-run-v1" or "sts2-human-run-v2")||!RecordingFormat.KnownActionSchema(r.action_schema)||
            r.game_dll_sha256!=GameBuild.Sha256||
            r.vanilla_fingerprint!=GameBuild.VanillaFingerprint||
            r.decision!="trainable"||r.ascension is <0 or >10||r.party_size is <1 or >4||
            r.bytes is <1 or >16777216+65536||
            !UploadQueue.RecordingURL(r.catbox_url,r.sha256)||
            !Regex.IsMatch(r.character,@"\A[A-Z][A-Z0-9_.-]{0,255}\z")||
            !Regex.IsMatch(r.sha256,@"\A[0-9a-f]{64}\z")||
            (r.owner is not null&&!Regex.IsMatch(r.owner,@"\A[0-9a-f]{64}\z"))||
            r.public_replay==false||
            (r.training==false&&r.replay==false)||(r.replay==false&&r.public_replay==true)||
            // A party receipt is the sender's own "yes": run and seat come together, and only for a party.
            r.started is <1||
            (r.party_size>1)!=(r.run is not null)||(r.run is null)!=(r.seat is null)||
            (r.run is not null&&(!Regex.IsMatch(r.run,@"\A[0-9a-f]{64}\z")||r.seat is <0||r.seat>=r.party_size||
                r.owner is null||r.training!=true)))throw new ArgumentException("receipt_schema");
        Validate(r.active_mods??[],null);
        Text(r.game_version,128);Text(r.character,256);
        if(r.patch_audit is not {} a||a.entries<0||a.entries>MaxPatches||
            a.prefix<0||a.postfix<0||a.transpiler<0||a.finalizer<0||
            (long)a.entries!=(long)a.prefix+a.postfix+a.transpiler+a.finalizer||
            a.sha256.Length!=64||a.sha256.Any(c=>c is not (>= '0' and <= '9' or >= 'a' and <= 'f')))
            throw new ArgumentException("metadata_audit");
        if(JsonSerializer.SerializeToUtf8Bytes(r).Length>16384)throw new ArgumentException("receipt_limit");
    }
}

// Catbox multipart upload (no Catbox account), followed by a small durable receipt. When Catbox refuses or
// cannot be reached, the gzip goes to sts.pqp.me, which keeps it under its SHA-256.
// No API keys, userhash, cookies, credentials, redirects, or file fetches.
public sealed class UploadQueue : IAsyncDisposable
{
    // The uncompressed JSON cap is the binding one: sts.pqp.me rejects a recording whose gzip body
    // inflates past 16 MiB (humanRecordingRawCap), and its gzip wire cap (16 MiB + 64 KiB) is never
    // the tighter limit below that. Larger runs stay local as .oversize.
    public const int MaxBodyBytes = 16_777_216;
    public const string UserAgent = "HumanPlayRecorder/0.1.0";
    public string? LastDiagnostic {get;private set;}
    public Action? CapacityWarning { get; set; }
    // Called on the worker after the server accepted a receipt (replay notice).
    public Action<UploadReceipt>? Accepted {get;set;}
    // Restates the player's current choices on every receipt before it is used, new or saved: replay
    // fields, owner key, training and replay flags. Null holds the upload (no durable owner key yet).
    public Func<UploadReceipt,UploadReceipt?>? StampReceipt {get;set;}
    // False once the player has switched all sharing off: nothing is stored, sent or kept queued.
    public Func<bool>? Allowed {get;set;}
    sealed class ChoicesChanged:Exception;
    volatile CancellationTokenSource? inflight;
    volatile bool choicesCancelled;
    static bool SameChoices(UploadReceipt a,UploadReceipt b)=>
        a.training==b.training&&a.replay==b.replay&&a.owner==b.owner&&a.public_replay==b.public_replay;
    // Right before each network step: the receipt must carry exactly what the player allows now. One that
    // does not (saved earlier, or built before the choices changed) is restated with the current flags, in
    // place when it is already on disk, and never sent with the old ones; its recording is not uploaded again.
    // Nothing allowed any more, or a multiplayer run whose training consent is gone, drops the run; with no
    // durable install key the run is held.
    async Task<UploadReceipt> Recheck(UploadReceipt receipt,string runId,string receiptPath,bool onDisk,CancellationToken ct)
    {
        if(Allowed?.Invoke()==false){Purge(runId);throw new ChoicesChanged();}
        if(StampReceipt is not {} stamp)return receipt;
        if(stamp(receipt) is not {} current)throw new ChoicesChanged();
        if(SameChoices(receipt,current))return receipt;
        if(current is {training:false,replay:false}){Purge(runId);throw new ChoicesChanged();}
        UploadMetadata.Receipt(current);
        if(onDisk)await budget.FillReceiptAsync(receiptPath,JsonSerializer.SerializeToUtf8Bytes(current),ct);
        return current;
    }
    void Purge(string runId)
    {
        string path=Path.Combine(directory,runId+".json");
        deadlines.Remove(runId);
        foreach(var file in new[]{path,path+".oversize",path+".receipt"})
            if(File.Exists(file))File.Delete(file);
        budget.DeleteAcknowledgedJournal(runId);
    }
    static readonly Regex QueuedFile=new(@"\A([0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})\.json",RegexOptions.CultureInvariant);
    // The player's choices changed: stop the attempt in progress and bring what is queued in line. A saved
    // receipt that no longer matches is restated in place with the current flags (training=false after a
    // withdrawal) without uploading its recording again; a multiplayer run whose training consent is
    // gone, or any run once nothing is allowed, is purged.
    public async Task ApplyChoicesAsync()
    {
        choicesCancelled=true;
        try{inflight?.Cancel();}catch(ObjectDisposedException){}
        try
        {
            await gate.WaitAsync().ConfigureAwait(false);
            try
            {
                choicesCancelled=false;
                if(!Directory.Exists(directory))return;
                bool none=Allowed?.Invoke()==false;
                foreach(var file in Directory.GetFiles(directory,"*.json*"))
                {
                    if(QueuedFile.Match(Path.GetFileName(file)) is not {Success:true} queued)continue;
                    string name=Path.GetFileName(file);
                    if(none){File.Delete(file);budget.DeleteAcknowledgedJournal(queued.Groups[1].Value);continue;}
                    if(!name.EndsWith(".json.receipt",StringComparison.Ordinal))continue;
                    // An unreadable slot (a reservation whose upload is unconfirmed) is left to the explicit
                    // recovery; the attempt never sends it.
                    UploadReceipt? saved=null;
                    try{saved=JsonSerializer.Deserialize<UploadReceipt>(File.ReadAllBytes(file));}catch(JsonException){}
                    if(saved is null||StampReceipt?.Invoke(saved) is not {} current||SameChoices(saved,current))continue;
                    if(current is {training:false,replay:false}){Purge(queued.Groups[1].Value);continue;}
                    await budget.FillReceiptAsync(file,JsonSerializer.SerializeToUtf8Bytes(current));
                }
            }
            finally{gate.Release();}
        }
        catch(Exception e) when(e is IOException or UnauthorizedAccessException or ObjectDisposedException)
        {
            Console.Error.WriteLine("[HumanPlayRecorder] queue purge failed: "+e.GetType().Name);
        }
    }
    // "Delete all my data" on this install: stops the attempt in progress, removes everything queued for
    // upload and refuses, from now on, every run that began at or before `at`. A run still being played
    // is never queued; the server applies the same cutoff to anything already in flight.
    public async Task PurgeAllAsync(DateTimeOffset at)
    {
        choicesCancelled=true;
        try{inflight?.Cancel();}catch(ObjectDisposedException){}
        await gate.WaitAsync().ConfigureAwait(false);
        try
        {
            choicesCancelled=false;
            Directory.CreateDirectory(directory);
            barrier=Math.Max(Barrier(),at.ToUnixTimeSeconds());
            File.WriteAllText(BarrierPath+".partial",barrier.ToString(System.Globalization.CultureInfo.InvariantCulture));
            File.Move(BarrierPath+".partial",BarrierPath,true);
            foreach(var file in Directory.GetFiles(directory,"*.json*"))
            {
                if(QueuedFile.Match(Path.GetFileName(file)) is not {Success:true} queued)continue;
                File.Delete(file);
                budget.DeleteAcknowledgedJournal(queued.Groups[1].Value);
            }
            await budget.DeleteAllJournalsAsync();
        }
        finally{gate.Release();}
    }
    internal bool IsAt(string path)=>Path.GetFullPath(path)==Path.GetFullPath(directory);
    string BarrierPath=>Path.Combine(directory,".delete-barrier");
    long barrier=-1;
    // Unix seconds of the latest "delete all" on this install (0 when never).
    long Barrier()
    {
        barrier=Math.Max(barrier,File.Exists(BarrierPath)&&long.TryParse(File.ReadAllText(BarrierPath),System.Globalization.NumberStyles.None,
            System.Globalization.CultureInfo.InvariantCulture,out var saved)?saved:0);
        return barrier;
    }
    static long StartedUnix(RunPayload p)=>DateTimeOffset.Parse(p.started_at,System.Globalization.CultureInfo.InvariantCulture,
        System.Globalization.DateTimeStyles.AssumeUniversal).ToUnixTimeSeconds();
    DateTimeOffset EndedAt(RunPayload p,string path)=>
        DateTimeOffset.TryParse(p.ended_at,System.Globalization.CultureInfo.InvariantCulture,System.Globalization.DateTimeStyles.AssumeUniversal,out var ended)?
            ended:new DateTimeOffset(File.GetLastWriteTimeUtc(path),TimeSpan.Zero);
    bool HoldOver(RunPayload p,string path)=>p.party_size>1&&now()-EndedAt(p,path)>=PartyHold;
    // The hold ends a run on its own clock: neither a retry back-off nor a server Retry-After defers it. Called
    // under the gate before any schedule check, for one run or for everything queued.
    // Deadlines already read (run id -> when its hold ends; MaxValue for a run that is never held), so a sweep
    // deserializes a recording once, not on every scan. An entry is valid only for the file it was read from (its
    // length and last-write time): a file replaced under the same run id is read again. Used under the gate only.
    readonly Dictionary<string,(DateTimeOffset Deadline,long Length,long Written)> deadlines=new();
    bool ExpireHeld(string runId)
    {
        string path=Path.Combine(directory,runId+".json");
        var info=new FileInfo(path);
        if(!info.Exists||info.Length>MaxBodyBytes){deadlines.Remove(runId);return false;}
        var identity=(info.Length,info.LastWriteTimeUtc.Ticks);
        if(deadlines.TryGetValue(runId,out var known)&&(known.Length,known.Written)==identity&&now()<known.Deadline)return false;
        RunPayload? payload;
        try{payload=JsonSerializer.Deserialize<RunPayload>(File.ReadAllBytes(path),RecordingFormat.Json);}
        catch(JsonException){return false;}
        if(payload is null)return false;
        if(HoldOver(payload,path)){Purge(runId);return true;}
        deadlines[runId]=(payload.party_size>1?EndedAt(payload,path)+PartyHold:DateTimeOffset.MaxValue,identity.Length,identity.Item2);
        return false;
    }
    internal async Task ExpireHeldAsync(CancellationToken ct=default)
    {
        await gate.WaitAsync(ct).ConfigureAwait(false);
        try
        {
            if(!Directory.Exists(directory))return;
            var present=new HashSet<string>(StringComparer.Ordinal);
            foreach(var file in Directory.GetFiles(directory,"*.json"))
            {
                if(QueuedFile.Match(Path.GetFileName(file)) is not {Success:true} queued)continue;
                string runId=queued.Groups[1].Value;
                present.Add(runId);
                // One unreadable file (locked, being replaced) never stops the files after it.
                try{ExpireHeld(runId);}
                catch(Exception e) when(e is IOException or UnauthorizedAccessException)
                {
                    Console.Error.WriteLine("[HumanPlayRecorder] expiry check failed: "+e.GetType().Name);
                }
            }
            foreach(var gone in deadlines.Keys.Where(id=>!present.Contains(id)).ToList())deadlines.Remove(gone);
        }
        catch(Exception e) when(e is IOException or UnauthorizedAccessException)
        {
            Console.Error.WriteLine("[HumanPlayRecorder] expiry sweep failed: "+e.GetType().Name);
        }
        finally{gate.Release();}
    }
    enum RunClearance{Cleared,Pending,Refused,Unavailable}
    // The server's answer for this install's seat of the run; anything but a clear answer is "unavailable".
    async Task<RunClearance> ClearanceAsync(UploadReceipt receipt,CancellationToken ct)
    {
        using var request=new HttpRequestMessage(HttpMethod.Post,RunEndpoint)
        {
            Content=new ByteArrayContent(JsonSerializer.SerializeToUtf8Bytes(new
                {run=receipt.run,seat=receipt.seat,party_size=receipt.party_size,owner=receipt.owner,started=receipt.started}))
        };
        request.Content.Headers.ContentType=new MediaTypeHeaderValue("application/json"){CharSet="utf-8"};
        using var response=await client.SendAsync(request,HttpCompletionOption.ResponseHeadersRead,ct);
        Retry(response);
        if(response.StatusCode!=HttpStatusCode.OK)return RunClearance.Unavailable;
        using var reply=JsonDocument.Parse(await BoundedBody(response,ct));
        return reply.RootElement.ValueKind==JsonValueKind.Object&&reply.RootElement.TryGetProperty("state",out var state)&&state.ValueKind==JsonValueKind.String?
            state.GetString() switch{"cleared"=>RunClearance.Cleared,"pending"=>RunClearance.Pending,"refused"=>RunClearance.Refused,_=>RunClearance.Unavailable}:
            RunClearance.Unavailable;
    }
    public static readonly Uri Endpoint = new("https://sts.pqp.me/api/human-play/receipt");
    public static readonly Uri CatboxEndpoint = new("https://catbox.moe/user/api.php");
    public static readonly Uri RecordingEndpoint = new("https://sts.pqp.me/api/human-play/recording");
    // A multiplayer run asks here, before any byte of it leaves the PC, whether every seat has declared its
    // consent: the server answers cleared, pending or refused for the run.
    public static readonly Uri RunEndpoint = new("https://sts.pqp.me/api/human-play/receipt?kind=run");
    // A pending run (a player without the mod can never declare) is kept locally this long, then deleted;
    // the server drops an unfinished run after the same time.
    public static readonly TimeSpan PartyHold = TimeSpan.FromHours(72);
    static readonly TimeSpan PendingPoll = TimeSpan.FromMinutes(10);
    public const string ServerRecordingPrefix = "https://sts.pqp.me/api/human-play/recordings/";
    static readonly Regex CatboxURL = new(@"\Ahttps://files\.catbox\.moe/[A-Za-z0-9_-]{1,64}\.gz\z",
        RegexOptions.CultureInvariant);
    // A Catbox link, or the server's content-addressed copy of exactly these bytes.
    public static bool RecordingURL(string url,string sha256)=>
        CatboxURL.IsMatch(url)||Regex.IsMatch(sha256,@"\A[0-9a-f]{64}\z")&&url==ServerRecordingPrefix+sha256+".gz";
    readonly string directory;
    readonly HttpClient client;
    readonly LocalStorageBudget budget;
    readonly SemaphoreSlim gate = new(1, 1);
    readonly CancellationTokenSource stop = new();
    Task? worker;
    DateTimeOffset retryAfter;
    DateTimeOffset serverRetryAfter;
    readonly Func<DateTimeOffset> now;
    readonly Func<double> jitter;
    int failures;
    bool scheduleLoaded;
    string SchedulePath=>Path.Combine(directory,".upload-schedule.state");
    string? cursor;
    sealed record Schedule(DateTimeOffset next,int failures,string? cursor=null);
    public UploadQueue(string directory, HttpMessageHandler? handler = null, LocalStorageBudget? budget = null,
        Func<DateTimeOffset>? utcNow=null,Func<double>? jitter=null)
    {
        this.directory = directory;
        now=utcNow??(()=>DateTimeOffset.UtcNow);
        this.jitter=jitter??(()=>System.Random.Shared.NextDouble());
        this.budget = budget ?? new LocalStorageBudget(directory);
        client = new HttpClient(handler ?? new HttpClientHandler {
            AllowAutoRedirect = false, UseCookies = false, UseDefaultCredentials = false
        }) { Timeout = TimeSpan.FromSeconds(60) };
        client.DefaultRequestHeaders.UserAgent.ParseAdd(UserAgent);
    }
    public static string TransportDiagnostic(Exception error)
    {
        var parts=new List<string>();
        for(int i=0;i<4&&error is not null;i++,error=error.InnerException!)
        {
            // Never log arbitrary exception text (filesystem paths, user data,
            // or remote content). These are fixed .NET transport messages.
            string message=error.Message switch {
                "An error occurred while sending the request." or
                "The response ended prematurely. (ResponseEnded)" or
                "The response ended prematurely." or
                "The SSL connection could not be established, see inner exception." or
                "Unable to read data from the transport connection: An existing connection was forcibly closed by the remote host." or
                "An existing connection was forcibly closed by the remote host." => error.Message,
                _ => "[redacted: message may contain private data]"
            };
            parts.Add(error.GetType().Name+": "+message+
                (error is HttpRequestException h?"; error="+h.HttpRequestError:""));
        }
        return string.Join(" -> ",parts);
    }
    // Explicit operator recovery, NOT an automatic retry policy. Call only after
    // independent confirmation that the remote upload was not accepted.
    // Filled/corrupt receipts and conflicts can never be reset through this API.
    public async Task<bool> RecoverUnacceptedReservationAsync(string runId,bool confirmedNotAccepted,CancellationToken ct=default)
    {
        if(!confirmedNotAccepted||!Guid.TryParseExact(runId,"D",out var id)||id.ToString()!=runId)return false;
        await gate.WaitAsync(ct);
        try {
            string body=Path.Combine(directory,runId+".json"),slot=body+".receipt";
            if(!File.Exists(body)||!File.Exists(slot)||new FileInfo(slot).Length!=16384)return false;
            byte[] reservation=await File.ReadAllBytesAsync(slot,ct);
            if(reservation.Any(b=>b!=0))return false;
            File.Delete(slot);
            // Leave the existing shared schedule/backoff and payload untouched.
            return true;
        } finally {gate.Release();}
    }
    async Task PersistSchedule()
    {
        byte[] bytes=JsonSerializer.SerializeToUtf8Bytes(new Schedule(retryAfter,failures,cursor));
        string temp=SchedulePath+".partial";
        if(File.Exists(temp))File.Delete(temp);
        await budget.WriteAsync(temp,bytes,false,essential:true);
        File.Move(temp,SchedulePath,true);
    }
    public Task StartAsync()
    {
        worker ??= Task.Run(Loop);
        return Task.CompletedTask;
    }
    public Task EnqueueAsync(RunPayload payload, CancellationToken ct = default) => EnqueueDurableAsync(payload, null, ct);
    internal string RetainedPayloadPath(string runId) =>
        Path.Combine(budget.Root, "incomplete", runId + ".jsonl." + Path.GetFileName(directory) + ".payload");
    internal async Task<string?> StageHandoffAsync(RunPayload payload)
    {
        string saved = RetainedPayloadPath(payload.run_id);
        byte[] bytes = JsonSerializer.SerializeToUtf8Bytes(payload, RecordingFormat.Json);
        string temp = saved + ".partial";
        await budget.WriteAsync(temp, bytes, false, essential: true);
        File.Move(temp, saved);
        return saved;
    }
    internal async Task EnqueueDurableAsync(RunPayload payload, string? durable, CancellationToken ct = default)
    {
        if (payload.ai_teammate_seats is { Length: > 0 })
            throw new InvalidOperationException("AI teammate runs cannot upload as human runs");
        if(payload.input_seed?.StartsWith("old",StringComparison.Ordinal)==true)throw new ArgumentException("legacy seed excluded");
        payload=payload with{seed=UploadMetadata.Seed(payload.seed),input_seed=payload.input_seed is null?null:UploadMetadata.Seed(payload.seed)};
        UploadMetadata.Validate(UploadMetadata.Mods(payload),payload.patch_audit);
        byte[] bytes = JsonSerializer.SerializeToUtf8Bytes(payload, RecordingFormat.Json);
        if (!Guid.TryParseExact(payload.run_id, "D", out var id) ||
            id.ToString() != payload.run_id || payload.run_id[14] != '4')
            throw new ArgumentException("run_id");
        await gate.WaitAsync(ct).ConfigureAwait(false);
        try
        {
            // A run that began before "delete all" is not queued at all (a capture that was still running then).
            // Checked under the lock "delete all" takes to set the barrier, so a write cannot slip in behind it.
            if(StartedUnix(payload)<=Barrier()){budget.DeleteAcknowledgedJournal(payload.run_id);return;}
            Directory.CreateDirectory(directory);
            string path = Path.Combine(directory, payload.run_id + ".json");
            if (bytes.Length > MaxBodyBytes) path += ".oversize";
            if (File.Exists(path))
            {
                if (!(await File.ReadAllBytesAsync(path, ct)).AsSpan().SequenceEqual(bytes))
                    throw new InvalidOperationException("local idempotency conflict");
                if (durable is not null) File.Delete(durable);
                return;
            }
            var pending = Directory.GetFiles(directory, "*.json*");
            if (pending.Length >= 100 || pending.Sum(p => new FileInfo(p).Length) + bytes.Length > 256L * 1024 * 1024)
            {
                CapacityWarning?.Invoke();
                throw new InvalidOperationException("queue_full");
            }
            if (durable is null) await SaveNew(path, bytes, ct);
            else
            {
                string expected = RetainedPayloadPath(payload.run_id);
                if (Path.GetFullPath(durable) != Path.GetFullPath(expected) ||
                    !(await File.ReadAllBytesAsync(durable, ct)).AsSpan().SequenceEqual(bytes))
                    throw new InvalidOperationException("durable handoff mismatch");
                // Same filesystem: promote existing durable bytes, no second allocation/quota.
                File.Move(durable, path);
            }
        }
        finally { gate.Release(); }
    }
    async Task SaveNew(string path, byte[] bytes, CancellationToken ct)
    {
        string temp = path + "." + Guid.NewGuid().ToString("N") + ".partial";
        await budget.WriteAsync(temp, bytes, append: false, ct);
        File.Move(temp, path);
    }
    async Task<byte[]> BoundedBody(HttpResponseMessage response, CancellationToken ct)
    {
        await using var input = await response.Content.ReadAsStreamAsync(ct);
        using var output = new MemoryStream();
        byte[] buffer = new byte[4097];
        int n;
        while ((n = await input.ReadAsync(buffer.AsMemory(0, Math.Min(buffer.Length, 4097 - (int)output.Length)), ct)) > 0)
        {
            await output.WriteAsync(buffer.AsMemory(0, n), ct);
            if (output.Length > 4096) throw new JsonException("acknowledgement limit");
        }
        return output.ToArray();
    }
    void Retry(HttpResponseMessage response)
    {
        if ((int)response.StatusCode is not (429 or 503)) return;
        var delay = response.Headers.RetryAfter?.Delta ??
            (response.Headers.RetryAfter?.Date - now());
        if (delay is { } value && value > TimeSpan.Zero)
            serverRetryAfter = now().Add(value > TimeSpan.FromDays(1) ? TimeSpan.FromDays(1) : value);
    }
    public async Task<bool> AttemptAsync(string runId, CancellationToken ct = default)
    {
        using var deadline = CancellationTokenSource.CreateLinkedTokenSource(ct);
        deadline.CancelAfter(TimeSpan.FromSeconds(120));
        ct = deadline.Token;
        if (!Guid.TryParseExact(runId, "D", out var id) || id.ToString() != runId) throw new ArgumentException("run_id");
        inflight = deadline;
        await gate.WaitAsync(ct);
        bool attempted=false,success=false,stale=false,waiting=false;
        string? reserved=null; // a receipt slot this attempt created and has not filled
        try
        {
            if(Allowed?.Invoke()==false){Purge(runId);success=true;return true;}
            // Before the schedule: a back-off or Retry-After never keeps a run past its hold.
            if(ExpireHeld(runId)){success=true;return true;}
            if(!scheduleLoaded)
            {
                if(File.Exists(SchedulePath))
                {
                    if(new FileInfo(SchedulePath).Length>1024)return false;
                    var saved=JsonSerializer.Deserialize<Schedule>(await File.ReadAllBytesAsync(SchedulePath,ct))!;
                    retryAfter=saved.next;failures=Math.Clamp(saved.failures,0,10);cursor=saved.cursor;
                }
                scheduleLoaded=true;
            }
            if (now() < retryAfter) return false;
            attempted=true;
            cursor=runId; // Persist fair rotation even when this candidate is ambiguous.
            // Persist a crash-safe lease before any network attempt.
            retryAfter=now().AddMinutes(3);
            await PersistSchedule();
            string path = Path.Combine(directory, runId + ".json");
            if (new FileInfo(path).Length > MaxBodyBytes) { File.Move(path, path + ".oversize"); return false; }
            byte[] body = await File.ReadAllBytesAsync(path, ct);
            if (body.Length > MaxBodyBytes) { File.Move(path, path + ".oversize"); return false; }
            string receiptPath = path + ".receipt";
            UploadReceipt receipt;
            if (File.Exists(receiptPath))
            {
                if (new FileInfo(receiptPath).Length > 16384) return false;
                receipt = JsonSerializer.Deserialize<UploadReceipt>(await File.ReadAllBytesAsync(receiptPath, ct))!;
                if (receipt is null || !RecordingURL(receipt.catbox_url,receipt.sha256) ||
                    receipt.sha256 is not { Length: 64 } || receipt.sha256.Any(c => c is not (>= '0' and <= '9' or >= 'a' and <= 'f')) ||
                    receipt.bytes is < 1 or > MaxBodyBytes + 65536) return false;
                if(receipt.started is {} began&&began<=Barrier()){Purge(runId);success=true;return true;}
                // A saved receipt waits for the server like a new run does: the same limit, before any network contact.
                if(HoldOver(JsonSerializer.Deserialize<RunPayload>(body,RecordingFormat.Json)!,path)){Purge(runId);success=true;return true;}
                // A receipt saved earlier carries the choices of its time, not necessarily today's.
                receipt=await Recheck(receipt,runId,receiptPath,true,ct);
            }
            else
            {
                var payload = JsonSerializer.Deserialize<RunPayload>(body, RecordingFormat.Json)!;
                if (payload.ai_teammate_seats is { Length: > 0 }) return false;
                if(payload.input_seed?.StartsWith("old",StringComparison.Ordinal)==true)return false;
                payload=payload with{seed=UploadMetadata.Seed(payload.seed),input_seed=payload.input_seed is null?null:UploadMetadata.Seed(payload.seed)};
                body=JsonSerializer.SerializeToUtf8Bytes(payload,RecordingFormat.Json);
                if(payload.training is not {decision:"trainable"} ||
                    payload.game_dll_sha256!=GameBuild.Sha256 ||
                    payload.training.vanilla_fingerprint!=GameBuild.VanillaFingerprint)
                    return false; // Pre-gate captures are not silently promoted.
                UploadMetadata.Validate(UploadMetadata.Mods(payload),payload.patch_audit);
                // A run that began before "delete all" never leaves the PC.
                if(StartedUnix(payload)<=Barrier()){Purge(runId);success=true;return true;}
                // A multiplayer run is held locally for a limited time only. The limit does not depend on what the
                // server answers: a pending answer, an outage, a malformed answer and a missing install key all end
                // with the run deleted once the hold is over.
                if(HoldOver(payload,path)){Purge(runId);success=true;return true;}
                using var compressed = new MemoryStream();
                using (var gzip = new GZipStream(compressed, CompressionLevel.SmallestSize, true))
                    await gzip.WriteAsync(body, ct);
                byte[] bytes = compressed.ToArray();
                string sha=Convert.ToHexStringLower(SHA256.HashData(bytes));
                // The server URL is the longest either path can return.
                receipt = new(payload.schema,payload.action_schema,
                    ServerRecordingPrefix+sha+".gz",sha,bytes.Length,
                    payload.game_version,payload.game_dll_sha256,payload.character,payload.ascension,payload.party_size,
                    UploadMetadata.Mods(payload),payload.training?.vanilla_fingerprint,payload.training?.decision,
                    RecordingFormat.AuditSummary(payload.patch_audit));
                receipt=receipt with{started=StartedUnix(payload)};
                if(payload.party_size>1)receipt=receipt with{run=payload.shared_run_id,seat=payload.local_seat};
                if(StampReceipt is {} stamp)
                {
                    // No durable owner key yet: hold the recording rather than upload what could never be deleted.
                    if(stamp(receipt) is not {} stamped)return false;
                    receipt=stamped;
                }
                if(receipt is {training:false,replay:false})
                {
                    // Consent withdrawn after the run: the recording never leaves the PC.
                    budget.DeleteAcknowledgedJournal(runId);
                    File.Delete(path);
                    success=true;
                    return true;
                }
                UploadMetadata.Receipt(receipt);
                // The largest possible URL must fit before creating any remote file.
                receipt=await Recheck(receipt,runId,receiptPath,false,ct);
                // No byte of a multiplayer run leaves the PC until the server confirms that every seat of the run
                // has declared its consent (this install's declaration included).
                if(receipt.party_size>1)
                {
                    switch(await ClearanceAsync(receipt,ct))
                    {
                        case RunClearance.Cleared: break;
                        case RunClearance.Refused: Purge(runId);success=true;return true;
                        case RunClearance.Pending:
                            // Kept locally while the other seats may still declare (the hold above ends it).
                            waiting=true;return false;
                        default: return false;
                    }
                }
                await budget.ReserveReceiptAsync(receiptPath,ct);
                reserved=receiptPath;
                // A multiplayer recording goes only to the server's copy: the server reads its party size there, and
                // Catbox files cannot be deleted.
                string? url = receipt.party_size>1 ? null : await CatboxAsync(bytes, ct);
                // The server copy is content-addressed, so a failed or ambiguous attempt is safe to repeat.
                receipt=await Recheck(receipt,runId,receiptPath,false,ct);
                try { url ??= await ServerCopyAsync(bytes, sha, ct); }
                catch { File.Delete(receiptPath); reserved=null; throw; }
                if (url is null) {File.Delete(receiptPath);reserved=null;return false;}
                receipt = receipt with{catbox_url=url};
                receipt=await Recheck(receipt,runId,receiptPath,false,ct);
                await budget.FillReceiptAsync(receiptPath, JsonSerializer.SerializeToUtf8Bytes(receipt), ct);
                reserved=null;
            }
            receipt=await Recheck(receipt,runId,receiptPath,true,ct);
            using var receiptRequest = new HttpRequestMessage(HttpMethod.Post, Endpoint);
            UploadMetadata.Receipt(receipt);
            byte[] receiptBytes=JsonSerializer.SerializeToUtf8Bytes(receipt);
            receiptRequest.Content = new ByteArrayContent(receiptBytes);
            receiptRequest.Content.Headers.ContentType = new MediaTypeHeaderValue("application/json") { CharSet = "utf-8" };
            using var acknowledgement = await client.SendAsync(receiptRequest, HttpCompletionOption.ResponseHeadersRead, ct);
            Retry(acknowledgement);
            int code = (int)acknowledgement.StatusCode;
            if(code==409) return false; // Candidate conflict is retryable; never delete/quarantine it.
            if (code == 422 && receipt.catbox_url == ServerRecordingPrefix + receipt.sha256 + ".gz" &&
                await ReceiptError(acknowledgement, ct) == "recording_missing")
            {
                // An unclaimed server copy expires after 24 h; drop the stale server-copy receipt
                // so a later attempt re-uploads the recording with the existing backoff.
                File.Delete(receiptPath);
                return false;
            }
            if (code is 400 or 413 or 422)
            {
                // Terminal: the retained journal stays for the player but is no storage problem (no warning).
                budget.MarkServerRejected(runId);
                File.Move(path, path + ".rejected");
                File.Move(receiptPath, receiptPath + ".rejected");
                File.SetLastWriteTimeUtc(path + ".rejected", DateTime.UtcNow);
                File.SetLastWriteTimeUtc(receiptPath + ".rejected", DateTime.UtcNow);
                return false;
            }
            if (code is not (200 or 201)) return false;
            using var ack = JsonDocument.Parse(await BoundedBody(acknowledgement, ct));
            var root = ack.RootElement;
            if (root.ValueKind != JsonValueKind.Object || root.EnumerateObject().Count() != 3 ||
                !root.TryGetProperty("receipt_sha256",out var receiptAck) ||
                receiptAck.GetString()!=Convert.ToHexStringLower(SHA256.HashData(receiptBytes)) ||
                !root.TryGetProperty("sha256", out var hash) || hash.GetString() != receipt.sha256 ||
                !root.TryGetProperty("status", out var status) ||
                status.GetString() != (code == 201 ? "accepted" : "duplicate")) return false;
            budget.DeleteAcknowledgedJournal(runId);
            File.Delete(path);
            File.Delete(receiptPath);
            success=true;
            try {Accepted?.Invoke(receipt);}
            catch(Exception e){Console.Error.WriteLine("[HumanPlayRecorder] accepted callback failed: "+e.GetType().Name);}
            return true;
        }
        // The player changed what may be shared mid-attempt: nothing of this attempt is kept, and it is
        // not a failure that earns a back-off.
        catch (Exception error) when (error is ChoicesChanged || choicesCancelled && error is OperationCanceledException)
        {
            stale=true;
            if(reserved is not null&&File.Exists(reserved))File.Delete(reserved);
            return false;
        }
        // Corrupt bodies, missing/locked files and network failures are item
        // failures, never worker-task failures or gameplay exceptions.
        catch (Exception error)
        {
            LastDiagnostic="[HumanPlayRecorder] upload failed: "+TransportDiagnostic(error);
            Console.Error.WriteLine(LastDiagnostic);
            return false;
        }
        finally {
            inflight=null;
            if(attempted&&!stale) {
                failures=success||waiting?0:Math.Min(failures+1,10);
                var delay=TimeSpan.FromSeconds(success?60:waiting?PendingPoll.TotalSeconds*(0.8+0.4*jitter()):
                    Math.Min(21600,60*Math.Pow(2,failures-1))*(0.8+0.4*jitter()));
                retryAfter=now().Add(delay);
                if(!success&&serverRetryAfter>retryAfter)retryAfter=serverRetryAfter;
                try {await PersistSchedule();}catch(Exception){retryAfter=now().AddHours(6);}
            }
            gate.Release();
        }
    }
    // Null when Catbox refuses (412/503 since 2026-11-02), answers with no link, or cannot be reached.
    // Its own deadline leaves the 120 s attempt enough time for the server copy.
    async Task<string?> CatboxAsync(byte[] bytes, CancellationToken attempt)
    {
        using var limit = CancellationTokenSource.CreateLinkedTokenSource(attempt);
        limit.CancelAfter(TimeSpan.FromSeconds(45));
        var ct = limit.Token;
        try
        {
            using var form = new MultipartFormDataContent();
            form.Add(new StringContent("fileupload"), "reqtype");
            var file = new ByteArrayContent(bytes);
            file.Headers.ContentType = new MediaTypeHeaderValue("application/gzip");
            form.Add(file, "fileToUpload", "recording.json.gz");
            using var request = new HttpRequestMessage(HttpMethod.Post, CatboxEndpoint) { Content = form };
            using var response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct);
            if (response.StatusCode != HttpStatusCode.OK) return null;
            string url = new UTF8Encoding(false, true).GetString(await BoundedBody(response, ct)).Trim();
            return CatboxURL.IsMatch(url) ? url : null;
        }
        catch (Exception error) when (!attempt.IsCancellationRequested)
        {
            LastDiagnostic="[HumanPlayRecorder] catbox upload failed, using server copy: "+TransportDiagnostic(error);
            Console.Error.WriteLine(LastDiagnostic);
            return null;
        }
    }
    async Task<string?> ReceiptError(HttpResponseMessage response, CancellationToken ct)
    {
        try
        {
            using var reply = JsonDocument.Parse(await BoundedBody(response, ct));
            return reply.RootElement.ValueKind == JsonValueKind.Object &&
                reply.RootElement.TryGetProperty("error", out var e) && e.ValueKind == JsonValueKind.String ? e.GetString() : null;
        }
        catch (JsonException) { return null; }
    }
    async Task<string?> ServerCopyAsync(byte[] bytes, string sha, CancellationToken ct)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, RecordingEndpoint) { Content = new ByteArrayContent(bytes) };
        request.Content.Headers.ContentType = new MediaTypeHeaderValue("application/gzip");
        using var response = await client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct);
        Retry(response);
        if ((int)response.StatusCode is not (200 or 201)) return null;
        using var reply = JsonDocument.Parse(await BoundedBody(response, ct));
        string url = ServerRecordingPrefix + sha + ".gz";
        return reply.RootElement.ValueKind == JsonValueKind.Object &&
            reply.RootElement.TryGetProperty("url", out var u) && u.ValueKind == JsonValueKind.String && u.GetString() == url ? url : null;
    }
    internal async Task<bool> ScanOnceAsync(CancellationToken ct = default)
    {
        try
        {
            Directory.CreateDirectory(directory);
            await RetryRetainedAsync(ct);
            await ExpireHeldAsync(ct);
            if(!scheduleLoaded&&File.Exists(SchedulePath))
            {
                if(new FileInfo(SchedulePath).Length>1024)return false;
                var saved=JsonSerializer.Deserialize<Schedule>(await File.ReadAllBytesAsync(SchedulePath,ct))!;
                retryAfter=saved.next;failures=Math.Clamp(saved.failures,0,10);cursor=saved.cursor;
                scheduleLoaded=true;
            }
            bool complete = true;
            foreach (var path in Directory.GetFiles(directory, "*.json")
                .OrderBy(p=>cursor is not null&&StringComparer.Ordinal.Compare(Path.GetFileNameWithoutExtension(p),cursor)<=0?1:0)
                .ThenBy(p=>p,StringComparer.Ordinal))
            {
                if (now() < retryAfter || ct.IsCancellationRequested) break;
                try
                {
                    complete &= await AttemptAsync(Path.GetFileNameWithoutExtension(path), ct);
                    break; // One item per interval; failures cannot fan out across backlog.
                }
                catch (Exception) { complete = false; }
            }
            return complete;
        }
        catch (Exception) { return false; }
    }
    internal async Task RetryRetainedAsync(CancellationToken ct = default)
    {
        string incomplete = Path.Combine(budget.Root, "incomplete");
        if (!Directory.Exists(incomplete)) return;
        string suffix = "." + Path.GetFileName(directory) + ".payload";
        foreach (string durable in Directory.GetFiles(incomplete, "*.jsonl" + suffix).Order(StringComparer.Ordinal))
        {
            string journal = durable[..^suffix.Length];
            try { budget.OpenJournal(journal); }
            catch (IOException) { continue; } // Active handoff or another retry owns it.
            try
            {
                var payload = JsonSerializer.Deserialize<RunPayload>(await File.ReadAllBytesAsync(durable, ct), RecordingFormat.Json)
                    ?? throw new JsonException("retained payload missing");
                await EnqueueDurableAsync(payload, durable, ct);
            }
            catch (Exception e) when (e is IOException or InvalidOperationException or JsonException or UnauthorizedAccessException)
            {
                // A full queue has already raised the capacity warning (EnqueueDurableAsync); nothing else is about space.
                Console.Error.WriteLine("[HumanPlayRecorder] retained handoff retry failed: " + e.GetType().Name);
            }
            finally { budget.CloseJournal(journal); }
        }
        // A retained journal without a payload in any queue is never fabricated into one or discarded:
        // the player removes it in the local storage window.
        if (budget.HasUnqueuedRetainedJournal()) CapacityWarning?.Invoke();
    }
    // Launch: every staged handoff is retried into its own queue; the warning is raised only when a queue had
    // no room or a retained journal has no payload left in any queue.
    internal static async Task RecoverRetainedAsync(LocalStorageBudget storage, Action warning)
    {
        foreach (var name in new[] { "pending", "uncertified" })
        {
            await using var recovery = new UploadQueue(Path.Combine(storage.Root, name), budget: storage)
                { CapacityWarning = warning };
            await recovery.RetryRetainedAsync();
        }
    }
    // The local storage window: removes runs from this PC only. The attempt in progress is stopped first, so a
    // run is never deleted under its own upload; the server and the "delete all" cutoff are untouched.
    // Returns how many runs were left because their journal is still owned (recording or handoff).
    public async Task<int> DeleteLocalAsync(IReadOnlyCollection<string> runIds)
    {
        choicesCancelled=true;
        try{inflight?.Cancel();}catch(ObjectDisposedException){}
        await gate.WaitAsync().ConfigureAwait(false);
        try
        {
            choicesCancelled=false;
            int left=0;
            foreach(string id in runIds)if(!await budget.DeleteRunAsync(id))left++;
            return left;
        }
        finally{gate.Release();}
    }
    async Task Loop()
    {
        while (!stop.IsCancellationRequested)
        {
            try { await ScanOnceAsync(stop.Token); }
            catch (Exception) { /* Keep the lifetime task alive even on a scan failure. */ }
            var wait = TimeSpan.FromSeconds(60);
            if (retryAfter - now() > wait) wait = retryAfter - now();
            // Expiry has its own clock: wake at least this often to end held runs, whatever the back-off says.
            if (wait > PendingPoll) wait = PendingPoll;
            try { await Task.Delay(wait, stop.Token); }
            catch (OperationCanceledException) when (stop.IsCancellationRequested) { break; }
        }
    }
    public async ValueTask DisposeAsync()
    {
        stop.Cancel();
        if (worker is not null) await worker.ConfigureAwait(false);
        client.Dispose(); stop.Dispose(); gate.Dispose();
    }
}
