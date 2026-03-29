# copilot.ps1 — Wrapper for co-pilot HTTP calls to the WeaveLang GUI relay server.
# Usage:
#   .\copilot.ps1 ping
#   .\copilot.ps1 state
#   .\copilot.ps1 sentence <N>        (1-based, matches terminal)
#   .\copilot.ps1 cmd <terminal_cmd>
#   .\copilot.ps1 batch "cmd1" "cmd2" "cmd3"   (run multiple commands sequentially)
#   .\copilot.ps1 job_status                   (non-blocking: check LLM job progress)
#   .\copilot.ps1 wait [interval_sec]           (poll job_status until DONE/IDLE)
#   .\copilot.ps1 shutdown

param(
    [Parameter(Position=0, Mandatory=$true)]
    [string]$Action,

    [Parameter(Position=1, ValueFromRemainingArguments=$true)]
    [string[]]$Args
)

$port = 3030
$base = "http://127.0.0.1:$port/api/v1"

function Send-Cmd {
    param([string]$CmdText)
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($CmdText)
    $result = Invoke-RestMethod -Uri "$base/terminal" -Method Post -Body $bodyBytes -ContentType "text/plain; charset=utf-8" -TimeoutSec 660
    Write-Host $result
}

switch ($Action) {
    "ping" {
        Invoke-RestMethod -Uri "$base/ping" -Method Get | ConvertTo-Json
    }
    "state" {
        Invoke-RestMethod -Uri "$base/state" -Method Get | ConvertTo-Json -Depth 10
    }
    "sentence" {
        if (-not $Args) { Write-Error "Usage: .\copilot.ps1 sentence <index>"; exit 1 }
        Invoke-RestMethod -Uri "$base/state/sentence/$($Args[0])" -Method Get | ConvertTo-Json -Depth 10
    }
    "cmd" {
        if (-not $Args) { Write-Error "Usage: .\copilot.ps1 cmd <command text>"; exit 1 }
        $body = $Args -join " "
        Send-Cmd $body
    }
    "batch" {
        if (-not $Args) { Write-Error "Usage: .\copilot.ps1 batch `"cmd1`" `"cmd2`" ..."; exit 1 }
        foreach ($cmdText in $Args) {
            Write-Host ">> $cmdText"
            Send-Cmd $cmdText
        }
    }
    "shutdown" {
        Invoke-RestMethod -Uri "$base/shutdown" -Method Post | ConvertTo-Json
    }
    "job_status" {
        Send-Cmd "job_status"
    }
    "wait" {
        # Poll job_status until DONE or IDLE, with configurable interval (default 10s)
        $interval = if ($Args -and $Args[0]) { [int]$Args[0] } else { 10 }
        $maxPolls = 360  # safety cap: 360 * 10s = 1 hour
        $polls = 0
        while ($polls -lt $maxPolls) {
            $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes("job_status")
            $result = Invoke-RestMethod -Uri "$base/terminal" -Method Post -Body $bodyBytes -ContentType "text/plain; charset=utf-8"
            $resultStr = $result.ToString().Trim()
            if ($resultStr -match "^DONE" -or $resultStr -match "^IDLE") {
                Write-Host $resultStr
                break
            }
            # Still running — show progress and sleep
            Write-Host "[$([DateTime]::Now.ToString('HH:mm:ss'))] $resultStr"
            Start-Sleep -Seconds $interval
            $polls++
        }
        if ($polls -ge $maxPolls) {
            Write-Host "Wait timed out after $($maxPolls * $interval) seconds."
        }
    }
    default {
        Write-Host "Unknown action: $Action"
        Write-Host "Actions: ping, state, sentence <N>, cmd <text>, batch <cmds...>, job_status, wait [sec], shutdown"
        exit 1
    }
}
