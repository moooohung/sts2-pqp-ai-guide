using System.Net;
using HumanPlayRecorder;

static class RecordingDownload
{
    public static Uri[] URLs(string canonical)
    {
        const string prefix="https://files.catbox.moe/";
        if(canonical.StartsWith(UploadQueue.ServerRecordingPrefix,StringComparison.Ordinal)&&
            System.Text.RegularExpressions.Regex.IsMatch(canonical[UploadQueue.ServerRecordingPrefix.Length..],@"\A[0-9a-f]{64}\.gz\z"))
            return [new(canonical)];
        if(!canonical.StartsWith(prefix,StringComparison.Ordinal))throw new ArgumentException("download host");
        string name=canonical[prefix.Length..];
        if(!System.Text.RegularExpressions.Regex.IsMatch(name,@"\A[A-Za-z0-9_-]{1,64}\.gz\z"))
            throw new ArgumentException("download name");
        return [new("https://mirror.example.org/"+name),new(canonical)];
    }
    public static async Task<byte[]> Fetch(HttpClient client,UploadReceipt receipt)
    {
        foreach(var url in URLs(receipt.catbox_url))
        {
            try {
                using var response=await client.GetAsync(url,HttpCompletionOption.ResponseHeadersRead);
                if(response.StatusCode!=HttpStatusCode.OK||response.Content.Headers.ContentEncoding.Count!=0)continue;
                await using var input=await response.Content.ReadAsStreamAsync();
                using var output=new MemoryStream();
                byte[] buffer=new byte[65536];int n;
                using var deadline=new CancellationTokenSource(TimeSpan.FromSeconds(60));
                while((n=await input.ReadAsync(buffer,deadline.Token))>0){
                    if(output.Length+n>receipt.bytes)throw new IOException("download limit");
                    output.Write(buffer,0,n);
                }
                byte[] bytes=output.ToArray();
                if(bytes.LongLength==receipt.bytes&&Convert.ToHexStringLower(System.Security.Cryptography.SHA256.HashData(bytes))==receipt.sha256)
                    return bytes;
            }catch(HttpRequestException){}catch(IOException){}catch(OperationCanceledException){}
        }
        throw new IOException("both download hosts failed");
    }
}
