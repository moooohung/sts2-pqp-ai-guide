using System.Text.Json;
using HumanPlayRecorder;

static class AscensionRecordingTests
{
    public static async Task Run(RunPayload fixture)
    {
        string root = Path.Combine(Path.GetTempPath(), "hpr-ascension-" + Guid.NewGuid().ToString("N"));
        try
        {
            for (int ascension = 0; ascension <= 10; ascension++)
            {
                var header = new RunHeader(Guid.NewGuid().ToString(), fixture.mod_version,
                    fixture.game_version, fixture.game_dll_sha256, fixture.character, fixture.seed,
                    fixture.active_mods, fixture.started_at, fixture.mode, fixture.party_size,
                    fixture.local_seat, fixture.party, fixture.shared_run_id)
                    { ascension = ascension, training = fixture.training };
                var transport = new QueueHandler { Code = 201, Body = null };
                string directory = Path.Combine(root, ascension.ToString());
                await using var queue = new UploadQueue(directory, transport);
                await CompletedRun.QueueAsync(queue, header, fixture.actions, fixture.floor_digests,
                    fixture.result, fixture.ended_at, completeCapture: true);
                using var queued = JsonDocument.Parse(await File.ReadAllBytesAsync(
                    Path.Combine(directory, header.run_id + ".json")));
                if (queued.RootElement.GetProperty("ascension").GetInt32() != ascension)
                    throw new Exception($"A{ascension} recording lost its real ascension");
                if (!await queue.AttemptAsync(header.run_id))
                    throw new Exception($"A{ascension} recording was not uploaded");
                using var receipt = JsonDocument.Parse(transport.LastReceipt!);
                if (receipt.RootElement.GetProperty("ascension").GetInt32() != ascension ||
                    transport.CatboxCalls != 1 || transport.ReceiptCalls != 1)
                    throw new Exception($"A{ascension} receipt lost its real ascension");
            }
        }
        finally { if (Directory.Exists(root)) Directory.Delete(root, recursive: true); }
        Console.WriteLine("PASS: A0–A10 recording, durable queue and upload receipts preserve real ascension");
    }
}
