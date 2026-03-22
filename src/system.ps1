$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
$script:taskWarnings=@()
function Tasks {
    param($folder)
    try {
    foreach ($task in $folder.GetTasks(1)) {
        try {
        $xml = [xml]$task.Xml
        if ($null -ne $xml.Task.Triggers.LogonTrigger -or $null -ne $xml.Task.Triggers.BootTrigger) {
            [pscustomobject]@{path=$task.Path;name=$task.Name;enabled=[bool]$task.Enabled;xml=$task.Xml;command=(@($xml.Task.Actions.Exec | ForEach-Object { "$($_.Command) $($_.Arguments)" }) -join '; ')}
        }
        } catch { $script:taskWarnings += "A task in $($folder.Path) could not be read: $($_.Exception.Message)" }
    }
    } catch { $script:taskWarnings += "Tasks in $($folder.Path) could not be read: $($_.Exception.Message)" }
    try { foreach ($child in $folder.GetFolders(0)) { Tasks $child } }
    catch { $script:taskWarnings += "Task folders in $($folder.Path) could not be read: $($_.Exception.Message)" }
}
try {
    if ($request.action -eq 'scan') {
        $warnings = @(); $tasks = @(); $services = @()
        try { $scheduler = New-Object -ComObject Schedule.Service; $scheduler.Connect(); $tasks = @(Tasks ($scheduler.GetFolder('\'))) } catch { $warnings += "Some scheduled tasks could not be read: $($_.Exception.Message)" }
        try { $services = @(Get-CimInstance Win32_Service -Filter "StartMode='Auto' OR StartMode='Disabled'" | ForEach-Object {
            $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Services\$($_.Name)")
            try { [pscustomobject]@{name=$_.Name;display=$_.DisplayName;command=$_.PathName;start=[int]$key.GetValue('Start');delayed=([int]$key.GetValue('DelayedAutoStart',0) -ne 0)} } finally {if($key){$key.Dispose()}}
        }) } catch { $warnings += "Services could not be read: $($_.Exception.Message)" }
        $shortcuts=@()
        try {
            $shell=New-Object -ComObject WScript.Shell
            $shortcuts=@($request.folders | ForEach-Object {
                if(Test-Path -LiteralPath $_ -PathType Container){
                    Get-ChildItem -LiteralPath $_ -Filter '*.lnk' -File | ForEach-Object {
                        try{$shortcut=$shell.CreateShortcut($_.FullName);[pscustomobject]@{path=$_.FullName;command=('"'+$shortcut.TargetPath+'" '+$shortcut.Arguments).Trim()}}
                        catch{$script:taskWarnings += "A shortcut target could not be read: $($_.Exception.Message)"}
                    }
                }
            })
        } catch { $warnings += "Shortcut targets could not be read: $($_.Exception.Message)" }
        @{tasks=$tasks;services=$services;shortcuts=$shortcuts;warnings=@($warnings+$script:taskWarnings|Select-Object -Unique)} | ConvertTo-Json -Depth 10 -Compress
    } elseif ($request.action -eq 'task-undo') {
        $scheduler=New-Object -ComObject Schedule.Service; $scheduler.Connect()
        $task=$scheduler.GetFolder('\').GetTask([string]$request.path)
        if($task.Xml -cne [string]$request.expectedXml){throw 'The task changed since the action. Its current configuration will not be overwritten.'}
        $task.Enabled=[bool]$request.enabled
        @{ok=$true;xml=$task.Xml}|ConvertTo-Json -Compress
    } elseif ($request.action -eq 'task') {
        $scheduler = New-Object -ComObject Schedule.Service; $scheduler.Connect()
        $path = [string]$request.path; $split = $path.LastIndexOf('\'); $folderName = $path.Substring(0,$split); if(!$folderName){$folderName='\'}
        $folder = $scheduler.GetFolder($folderName); $name=$path.Substring($split+1)
        if ($request.mode -eq 'restore') {
            # TASK_CREATE: never replace a task created since the backup.
            try { $existing=$folder.GetTask($name) } catch {
                $exception=$_.Exception
                while($exception.InnerException){$exception=$exception.InnerException}
                if($exception.HResult -ne -2147024894){throw}
                $existing=$null
            }
            if($existing){throw 'A task already exists at this path. Refresh and review it before restoring.'}
            $principal=([xml][string]$request.xml).Task.Principals.Principal
            $account=if($principal.UserId){[string]$principal.UserId}else{[string]$principal.GroupId}
            $null=$folder.RegisterTask($name,[string]$request.xml,2,$account,$null,[int]$request.logonType,$null)
        } else {
            $task=$folder.GetTask($name)
            if($task.Xml -cne [string]$request.xml){throw 'The task changed since the scan. Refresh and retry.'}
            switch($request.mode){'delete'{$folder.DeleteTask($name,0)} 'disable'{$task.Enabled=$false} 'enable'{$task.Enabled=$true} default{throw 'Invalid task action'}}
        }
        $afterXml=if($request.mode -eq 'delete'){$null}else{$folder.GetTask($name).Xml}
        @{ok=$true;xml=$afterXml}|ConvertTo-Json -Compress
    } elseif ($request.action -eq 'service') {
        $name=[string]$request.name
        if($name -notmatch '^[a-zA-Z0-9_.\- ]+$'){throw 'Unsupported service name'}
        $key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Services\$name")
        try{$start=[int]$key.GetValue('Start');$delayed=([int]$key.GetValue('DelayedAutoStart',0) -ne 0)}finally{$key.Dispose()}
        if($start -ne [int]$request.expectedStart -or ($null -ne $request.expectedDelayed -and $delayed -ne [bool]$request.expectedDelayed)){throw 'Service configuration changed. Refresh and retry.'}
        $mode = if([int]$request.targetStart -eq 4){'disabled'}elseif([bool]$request.targetDelayed){'delayed-auto'}else{'auto'}
        $output = & "$env:SystemRoot\System32\sc.exe" config $name start= $mode 2>&1
        if($LASTEXITCODE -ne 0){throw "Service configuration failed. Administrator rights may be required. $output"}
        $key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Services\$name")
        try { @{ok=$true;start=[int]$key.GetValue('Start');delayed=([int]$key.GetValue('DelayedAutoStart',0) -ne 0)} | ConvertTo-Json -Compress } finally { $key.Dispose() }
    } elseif ($request.action -eq 'task-read') {
        $scheduler=New-Object -ComObject Schedule.Service; $scheduler.Connect()
        $task=$scheduler.GetFolder('\').GetTask([string]$request.path)
        @{xml=$task.Xml;enabled=[bool]$task.Enabled}|ConvertTo-Json -Compress
    } elseif ($request.action -eq 'task-fixture-create' -or $request.action -eq 'task-fixture-cleanup') {
        if([string]$request.path -notmatch '^\\StartupManager-Test-[0-9a-fA-F-]{36}$'){throw 'Invalid isolated test task path'}
        $scheduler=New-Object -ComObject Schedule.Service; $scheduler.Connect()
        if($request.action -eq 'task-fixture-cleanup'){
            $scheduler.GetFolder('\').DeleteTask([string]$request.path,0)
            @{ok=$true}|ConvertTo-Json -Compress
        }else{
            $definition=$scheduler.NewTask(0)
            $definition.RegistrationInfo.Description='Disposable Startup Manager test with no triggers. Never runs automatically.'
            $definition.Settings.Enabled=$false
            $definition.Principal.LogonType=3
            $definition.Principal.UserId=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
            $action=$definition.Actions.Create(0)
            $action.Path="$env:SystemRoot\System32\where.exe"
            $action.Arguments='StartupManager-Disposable-Fixture-Never-Run'
            $task=$scheduler.GetFolder('\').RegisterTaskDefinition([string]$request.path,$definition,2,$definition.Principal.UserId,$null,3,$null)
            @{xml=$task.Xml;enabled=[bool]$task.Enabled}|ConvertTo-Json -Compress
        }
    } else { throw 'Unknown system action' }
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
