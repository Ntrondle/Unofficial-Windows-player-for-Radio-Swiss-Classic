param([switch]$Playback)
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    New-Item -ItemType Directory -Force dist | Out-Null
    $mode = if ($Playback) { '--smoke-test' } else { '--self-test' }
    $label = if ($Playback) { 'playback' } else { 'ui' }
    Remove-Item windows-smoke-report.json -ErrorAction SilentlyContinue
    $process = Start-Process -FilePath (Resolve-Path dist/RadioSwissClassic.exe) -ArgumentList $mode -PassThru
    $samples = @()
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while (!$process.HasExited -and $timer.Elapsed.TotalSeconds -lt 60) {
        Start-Sleep -Milliseconds 1000
        $process.Refresh()
        if (!$process.HasExited) {
            $samples += @{ seconds = [math]::Round($timer.Elapsed.TotalSeconds, 1); working_set_bytes = $process.WorkingSet64; private_bytes = $process.PrivateMemorySize64 }
        }
        if (!$Playback -and $timer.Elapsed.TotalSeconds -ge 5 -and !(Test-Path dist/windows-native-preview.png)) {
            # Capture only this app's test window on the disposable Windows runner.
            Add-Type -AssemblyName System.Drawing
            if (!('RadioWindowCapture' -as [type])) {
                Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class RadioWindowCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
'@
            }
            $rect = New-Object RadioWindowCapture+Rect
            if ($process.MainWindowHandle -ne 0 -and [RadioWindowCapture]::GetWindowRect($process.MainWindowHandle, [ref]$rect)) {
                $bitmap = New-Object Drawing.Bitmap (($rect.Right-$rect.Left), ($rect.Bottom-$rect.Top))
                $graphics = [Drawing.Graphics]::FromImage($bitmap)
                $dc = $graphics.GetHdc()
                try { $captured = [RadioWindowCapture]::PrintWindow($process.MainWindowHandle, $dc, 2) }
                finally { $graphics.ReleaseHdc($dc); $graphics.Dispose() }
                if ($captured) { $bitmap.Save((Join-Path $PWD 'dist/windows-native-preview.png'), [Drawing.Imaging.ImageFormat]::Png) }
                $bitmap.Dispose()
            }
        }
    }
    if (!$process.HasExited) { $process.Kill(); throw 'Native app test timed out' }
    $process.WaitForExit()
    $samples | ConvertTo-Json -Depth 4 | Set-Content "dist/windows-$label-memory.json"
    if (Test-Path windows-smoke-report.json) {
        Copy-Item windows-smoke-report.json "dist/windows-$label-report.json" -Force
        Get-Content windows-smoke-report.json
    }
    if ($process.ExitCode -ne 0) { throw "Native $label test failed: exit $($process.ExitCode)" }
} finally {
    Pop-Location
}
