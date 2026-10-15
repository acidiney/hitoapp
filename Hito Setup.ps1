$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
$repository = $PSScriptRoot
$installation = Join-Path $env:LOCALAPPDATA 'Hito'
[IO.Directory]::CreateDirectory($installation) | Out-Null
$lock = Join-Path $installation 'setup.lock'
try { $lockHandle = [IO.File]::Open($lock, 'OpenOrCreate', 'ReadWrite', 'None') }
catch { [Windows.Forms.MessageBox]::Show('Hito Setup is already open.', 'Hito Setup') | Out-Null; exit 1 }
$stopped = $false
$container = $null
$encoding = New-Object Text.UTF8Encoding($false)
function Write-SetupEvent([string] $event) {
    [IO.File]::AppendAllText((Join-Path $installation 'setup.log'), ('{"event":"installation.' + $event + '"}' + "`n"), $encoding)
}
function Invoke-Docker([string] $step, [string[]] $arguments) {
    Write-SetupEvent "$step.started"
    & docker @arguments *> $null
    if ($LASTEXITCODE -ne 0) { Write-SetupEvent "$step.failed"; throw "Hito could not finish the $step step. Check Docker and try again." }
    Write-SetupEvent "$step.completed"
}
try {
    Invoke-Docker 'docker-check' @('info')
    $selected = New-Object 'Collections.Generic.List[string]'
    do {
        $picker = New-Object Windows.Forms.FolderBrowserDialog
        $picker.Description = 'Choose a music folder for Hito. Existing folders are kept.'
        $picker.ShowNewFolderButton = $false
        if ($picker.ShowDialog() -ne [Windows.Forms.DialogResult]::OK) { $picker.Dispose(); break }
        $folder = Get-Item -LiteralPath $picker.SelectedPath -Force
        $picker.Dispose()
        for ($ancestor = $folder; $null -ne $ancestor; $ancestor = $ancestor.Parent) {
            if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Choose the original folder rather than a shortcut or directory link.' }
        }
        $selected.Add($folder.FullName)
        $another = [Windows.Forms.MessageBox]::Show('Share another music folder?', 'Hito Setup', 'YesNo')
    } while ($another -eq [Windows.Forms.DialogResult]::Yes)
    if ($selected.Count -eq 0) { exit 0 }
    [IO.File]::WriteAllText((Join-Path $installation 'selection.json'), (ConvertTo-Json -InputObject @($selected.ToArray()) -Compress), $encoding)
    $savedPlan = Join-Path $installation 'plan.txt'
    if (Test-Path -LiteralPath $savedPlan) {
        $savedProject = (Get-Content -LiteralPath $savedPlan)[0]
        $container = @(& docker ps -a --filter 'label=com.docker.compose.service=server' --filter "label=com.docker.compose.project=$savedProject" --format '{{.ID}}')
    } else {
        $container = @(& docker ps -a --filter 'label=com.docker.compose.service=server' --filter "label=com.docker.compose.project.working_dir=$repository" --format '{{.ID}}')
    }
    if ($LASTEXITCODE -ne 0) { throw 'The existing Hito installation could not be checked.' }
    if ($container.Count -gt 1) { throw 'More than one Hito installation was found. No configuration was changed.' }
    $container = $container | Select-Object -First 1
    if ($container) {
        $probe = & docker inspect --format '{"project":{{json (index .Config.Labels "com.docker.compose.project")}},"mounts":{{json .Mounts}},"ports":{{json (index .HostConfig.PortBindings "8080/tcp")}}}' $container
        if ($LASTEXITCODE -ne 0) { throw 'The existing server could not be inspected.' }
    } else { $probe = 'null' }
    [IO.File]::WriteAllText((Join-Path $installation 'probe.json'), $probe, $encoding)
    Invoke-Docker 'configuration' @('run', '--rm', '-v', "${repository}:/workspace:ro", '-v', "${installation}:/setup", 'node:22-bookworm-slim', 'node', '/workspace/scripts/setup/prepare.mjs', '--installation', '/setup', '--host-installation', $installation, '--repository', $repository)
    $plan = Get-Content -LiteralPath (Join-Path $installation 'plan.txt')
    $project = $plan[0]; $volume = $plan[1]; $port = $plan[2]
    $compose = Join-Path $installation 'compose.json'
    Write-Host 'Building Hito in Docker. The first build can take several minutes…'
    Invoke-Docker 'build' @('compose', '-p', $project, '-f', $compose, 'build', 'server')
    & docker volume inspect $volume *> $null
    if ($LASTEXITCODE -eq 0) {
        $backups = Join-Path $installation 'backups'
        [IO.Directory]::CreateDirectory($backups) | Out-Null
        Invoke-Docker 'stop' @('compose', '-p', $project, '-f', $compose, 'stop', 'server')
        $stopped = $true
        $image = & docker image inspect "hito-local/server:$project" --format '{{.Id}}'
        if ($LASTEXITCODE -ne 0) { throw 'The backup image could not be found.' }
        $archive = 'hito-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + [Guid]::NewGuid().ToString('N') + '.tar.gz'
        Invoke-Docker 'backup' @('run', '--rm', '--user', '0', '--mount', "type=volume,source=$volume,target=/data,readonly", '-v', "${backups}:/backup", '--entrypoint', 'sh', $image, '-c', 'umask 077; exec tar -C /data -czf "/backup/$1" .', 'sh', $archive)
    }
    Invoke-Docker 'start' @('compose', '-p', $project, '-f', $compose, 'up', '-d', 'server')
    $stopped = $false
    Start-Process "http://localhost:$port/settings"
    Write-Host 'Hito is starting. Add the shared folders in Settings > Library.'
} catch {
    Write-SetupEvent 'failed'
    if ($stopped -and $container) { & docker start $container *> $null }
    [Windows.Forms.MessageBox]::Show('Hito setup failed. Check Docker and try again. Your original music and library data are preserved.', 'Hito Setup') | Out-Null
    exit 1
} finally {
    $lockHandle.Dispose()
}
