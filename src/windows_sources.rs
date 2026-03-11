use crate::{model::{Approval, Entry, Inventory, Source}, registry};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::{Read, Write}, os::windows::process::CommandExt, path::PathBuf, process::{Command, Stdio}, time::{Duration, Instant}};

pub const RUN_ROOT: &str = "Software\\Microsoft\\Windows\\CurrentVersion";

pub fn startup_directories() -> Vec<(PathBuf, &'static str)> {
    [("APPDATA", "Current user"),("ProgramData", "All users")].into_iter().filter_map(|(var,scope)| std::env::var_os(var).map(|root|(PathBuf::from(root).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"),scope))).collect()
}
pub fn hash(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
pub fn file_bytes(path: &std::path::Path) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    use std::os::windows::fs::MetadataExt;
    ensure!(metadata.is_file() && metadata.file_attributes() & 0x400 == 0, "Links and reparse points are not modified");
    ensure!(metadata.len() <= 16 * 1024 * 1024, "Startup file exceeds the 16 MiB backup limit");
    let bytes = std::fs::read(path)?;
    ensure!(bytes.len() <= 16*1024*1024,"Startup file grew beyond the backup limit"); Ok(bytes)
}
pub fn bridge(request: &serde_json::Value) -> Result<serde_json::Value> {
    let script: Vec<u8> = include_str!("system.ps1").encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut child = Command::new(std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(||PathBuf::from("C:\\Windows")).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe"))
        .args(["-NoLogo","-NoProfile","-NonInteractive","-EncodedCommand",&base64(&script)])
        .creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("Cannot start the Windows system reader")?;
    child.stdin.take().context("No system input pipe")?.write_all(&serde_json::to_vec(request)?)?;
    let out = child.stdout.take().context("No system output pipe")?;
    let err = child.stderr.take().context("No system error pipe")?;
    let read = |mut stream: Box<dyn Read + Send>| std::thread::spawn(move || { let mut bytes = vec![]; stream.by_ref().take(16*1024*1024+1).read_to_end(&mut bytes).map(|_|bytes) });
    let stdout = read(Box::new(out)); let stderr=read(Box::new(err)); let started=Instant::now();
    let status = loop { if let Some(status)=child.try_wait()? { break status; } if started.elapsed()>Duration::from_secs(40) { let _=child.kill(); let _=child.wait(); anyhow::bail!("Windows did not finish this operation within 40 seconds. Refresh before retrying."); } std::thread::sleep(Duration::from_millis(50)); };
    let stdout=stdout.join().map_err(|_|anyhow::anyhow!("System reader failed"))??;
    let stderr=stderr.join().map_err(|_|anyhow::anyhow!("System reader failed"))??;
    ensure!(status.success(),"{}",String::from_utf8_lossy(&stderr).trim());
    ensure!(stdout.len()<=16*1024*1024,"System response exceeded the safety limit");
    serde_json::from_slice(&stdout).context("Windows returned an invalid system response")
}
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output=String::new(); for chunk in bytes.chunks(3) { let a=chunk[0] as usize; let b=chunk.get(1).copied().unwrap_or(0) as usize; let c=chunk.get(2).copied().unwrap_or(0) as usize; output.push(ALPHABET[a>>2] as char); output.push(ALPHABET[(a&3)<<4|b>>4] as char); output.push(if chunk.len()>1 {ALPHABET[(b&15)<<2|c>>6] as char} else {'='}); output.push(if chunk.len()>2 {ALPHABET[c&63] as char} else {'='}); } output
}

pub fn scan() -> Result<Inventory> {
    let mut report=Inventory::default();
    for machine in [false,true] { for view in [64,32] { for leaf in ["Run","RunOnce"] {
        // The current-user Run key is shared between registry views.
        if !machine && view==32 {continue;}
        let key=format!("{RUN_ROOT}\\{leaf}");
        match registry::values(machine,&key,view) { Ok(values)=>for (name,kind,data) in values {
            if ![1,2].contains(&kind) {report.warnings.push(format!("{name}: unsupported registry value type {kind}; left unchanged"));continue;}
            let approval_leaf=if machine && view==32 {"Run32"}else{"Run"};
            let approval_key=format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\{approval_leaf}");
            let approval=if leaf=="Run" {registry::read(machine,&approval_key,64,&name).ok().flatten().map(|(_,data)|Approval{key:approval_key,data})}else{None};
            let enabled=approval.as_ref().is_none_or(|a|!matches!(a.data.first(),Some(3|7)));
            report.entries.push(Entry {id:format!("reg:{machine}:{view}:{key}:{name}"),name:name.clone(),command:registry::decode(&data),location:format!("{}\\{key} ({view}-bit)",if machine {"HKLM"}else{"HKCU"}),scope:if machine {"All users"}else{"Current user"}.into(),enabled,source:Source::Registry{machine,view,key:key.clone(),name,kind,data,approval},backup_id:None});
        },Err(error)=>report.warnings.push(format!("{key}: {error}"))}
    }}}
    for (directory,scope) in startup_directories() {
        if !directory.exists(){continue;}
        match std::fs::read_dir(&directory) {Ok(files)=>for file in files {
            let result=(||->Result<Entry>{let file=file?; let path=file.path(); let name=file.file_name().to_string_lossy().to_string(); ensure!(!name.eq_ignore_ascii_case("desktop.ini"),"skip"); let bytes=file_bytes(&path)?; let path=path.to_string_lossy().to_string(); Ok(Entry{id:format!("file:{path}"),name,command:path.clone(),location:directory.to_string_lossy().into(),scope:scope.into(),enabled:true,source:Source::Folder{path,digest:hash(&bytes)},backup_id:None})})();
            match result{Ok(entry)=>report.entries.push(entry),Err(e) if e.to_string()=="skip"=>{},Err(e)=>report.warnings.push(format!("Startup folder item: {e}"))}
        },Err(e)=>report.warnings.push(format!("{}: {e}",directory.display()))}
    }
    match bridge(&serde_json::json!({"action":"scan"})) {
        Ok(value)=>{
            #[derive(Deserialize)] struct Task {path:String,name:String,command:String,enabled:bool,xml:String}
            #[derive(Deserialize)] struct Service {name:String,display:String,command:String,start:u32,delayed:bool}
            #[derive(Deserialize)] struct Extra {tasks:Vec<Task>,services:Vec<Service>,warnings:Vec<String>}
            let extra:Extra=serde_json::from_value(value)?;
            report.warnings.extend(extra.warnings);
            for task in extra.tasks {report.entries.push(Entry{id:format!("task:{}",task.path),name:task.name,command:task.command,location:task.path.clone(),scope:"Task account".into(),enabled:task.enabled,source:Source::Task{path:task.path,xml:task.xml},backup_id:None});}
            for service in extra.services {report.entries.push(Entry{id:format!("service:{}",service.name),name:service.display,command:service.command,location:format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{}",service.name),scope:"All users".into(),enabled:service.start==2,source:Source::Service{name:service.name,start:service.start,delayed:service.delayed},backup_id:None});}
        },Err(error)=>report.warnings.push(error.to_string())
    }
    report.entries.sort_by_key(|entry|entry.name.to_lowercase()); Ok(report)
}

#[cfg(test)] mod tests {use super::*; #[test] fn script_transport_is_unambiguous(){ assert_eq!(base64(b"f"),"Zg=="); assert_eq!(base64(b"foo"),"Zm9v"); assert_eq!(registry::decode(&registry::encode("%APPDATA%\\тест.exe /quiet")),"%APPDATA%\\тест.exe /quiet");}}
