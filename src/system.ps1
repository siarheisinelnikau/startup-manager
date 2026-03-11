$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
function Tasks {
    param($folder)
    foreach ($task in $folder.GetTasks(1)) {
        $xml = [xml]$task.Xml
        if ($null -ne $xml.Task.Triggers.LogonTrigger -or $null -ne $xml.Task.Triggers.BootTrigger) {
            [pscustomobject]@{path=$task.Path;name=$task.Name;enabled=[bool]$task.Enabled;xml=$task.Xml;command=(@($xml.Task.Actions.Exec | ForEach-Object { "$($_.Command) $($_.Arguments)" }) -join '; ')}
        }
    }
    foreach ($child in $folder.GetFolders(0)) { Tasks $child }
}
try {
    if ($request.action -eq 'scan') {
        $warnings = @(); $tasks = @(); $services = @()
        try { $scheduler = New-Object -ComObject Schedule.Service; $scheduler.Connect(); $tasks = @(Tasks ($scheduler.GetFolder('\'))) } catch { $warnings += "Some scheduled tasks could not be read: $($_.Exception.Message)" }
        try { $services = @(Get-CimInstance Win32_Service -Filter "StartMode='Auto' OR StartMode='Disabled'" | ForEach-Object {
            $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Services\$($_.Name)")
            try { [pscustomobject]@{name=$_.Name;display=$_.DisplayName;command=$_.PathName;start=[int]$key.GetValue('Start');delayed=([int]$key.GetValue('DelayedAutoStart',0) -ne 0)} } finally {if($key){$key.Dispose()}}
        }) } catch { $warnings += "Services could not be read: $($_.Exception.Message)" }
        @{tasks=$tasks;services=$services;warnings=$warnings} | ConvertTo-Json -Depth 10 -Compress
    } elseif ($request.action -eq 'task') {
        $scheduler = New-Object -ComObject Schedule.Service; $scheduler.Connect()
        $path = [string]$request.path; $split = $path.LastIndexOf('\'); $folderName = $path.Substring(0,$split); if(!$folderName){$folderName='\'}
        $folder = $scheduler.GetFolder($folderName); $name=$path.Substring($split+1)
        if ($request.mode -eq 'restore') {
            # TASK_CREATE: never replace a task created since the backup.
            try { $existing=$folder.GetTask($name) } catch [System.Runtime.InteropServices.COMException] { if ($_.Exception.HResult -ne -2147024894) {throw}; $existing=$null }
            if($existing){throw 'A task already exists at this path. Refresh and review it before restoring.'}
            $null=$folder.RegisterTask($name,[string]$request.xml,2,$null,$null,[int]$request.logonType,$null)
        } else {
            $task=$folder.GetTask($name)
            if($task.Xml -cne [string]$request.xml){throw 'The task changed since the scan. Refresh and retry.'}
            switch($request.mode){'delete'{$folder.DeleteTask($name,0)} 'disable'{$task.Enabled=$false} 'enable'{$task.Enabled=$true} default{throw 'Invalid task action'}}
        }
        @{ok=$true}|ConvertTo-Json -Compress
    } elseif ($request.action -eq 'service') {
        $name=[string]$request.name
        if($name -notmatch '^[a-zA-Z0-9_.\- ]+$'){throw 'Unsupported service name'}
        $key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Services\$name")
        try{$start=[int]$key.GetValue('Start');$delayed=([int]$key.GetValue('DelayedAutoStart',0) -ne 0)}finally{$key.Dispose()}
        if($start -ne [int]$request.expectedStart -or $delayed -ne [bool]$request.expectedDelayed){throw 'Service configuration changed. Refresh and retry.'}
        $mode = if([int]$request.targetStart -eq 4){'disabled'}elseif([bool]$request.targetDelayed){'delayed-auto'}else{'auto'}
        $output = & "$env:SystemRoot\System32\sc.exe" config $name start= $mode 2>&1
        if($LASTEXITCODE -ne 0){throw "Service configuration failed. Administrator rights may be required. $output"}
        @{ok=$true}|ConvertTo-Json -Compress
    } else { throw 'Unknown system action' }
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
