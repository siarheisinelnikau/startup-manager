//! Write-ahead backups precede every mutation. Conflicts never overwrite new data.
use crate::{model::{Backup, Entry, Inventory, Operation, Source, Status}, registry, settings, windows_sources::{self as system, hash}};
use anyhow::{Context, Result, bail, ensure};
use std::{io::Write, path::{Path, PathBuf}};

pub struct Engine { pub directory: PathBuf }
impl Engine {
    pub fn new() -> Self {Self{directory:settings::directory().join("backups")}}
    pub fn backups(&self) -> Result<Vec<Backup>> {
        if !self.directory.exists(){return Ok(vec![]);}
        let mut items=vec![];
        for file in std::fs::read_dir(&self.directory)? {let path=file?.path(); if path.extension().is_some_and(|e|e=="json"){let record:Backup=serde_json::from_slice(&std::fs::read(&path)?).with_context(||format!("Cannot read backup {}. Original backups have been preserved.",path.display()))?; ensure!(path.file_stem().and_then(|s|s.to_str())==Some(&record.id),"Backup identity mismatch"); items.push(record);}}
        items.sort_by(|a,b|b.timestamp.cmp(&a.timestamp).then_with(||b.id.cmp(&a.id))); Ok(items)
    }
    fn record_path(&self,id:&str)->Result<PathBuf>{ensure!(uuid::Uuid::parse_str(id).is_ok(),"Invalid backup identity");Ok(self.directory.join(format!("{id}.json")))}
    fn save(&self,record:&Backup)->Result<()>{settings::write_json(&self.record_path(&record.id)?,record)}
    pub fn merge_disabled(&self,report:&mut Inventory)->Result<()> {
        for backup in self.backups()? {if backup.status==Status::Applied && backup.operation==Operation::Disable && matches!(backup.entry.source,Source::Registry{..}|Source::Folder{..}) && !report.entries.iter().any(|e|e.id==backup.entry.id) {let mut entry=backup.entry;entry.enabled=false;entry.backup_id=Some(backup.id);report.entries.push(entry);}}
        report.entries.sort_by_key(|e|e.name.to_lowercase()); Ok(())
    }
    pub fn apply(&self,entry:&Entry,operation:Operation)->Result<()> {
        if let Some(id)=&entry.backup_id {
            ensure!(operation==Operation::Enable,"This item is already backed up. Use Restore in History to recover it.");
            return self.restore(id);
        }
        ensure!(operation==Operation::Delete || entry.enabled!=(operation==Operation::Enable),"The item already has the requested state");
        if matches!(entry.source,Source::Service{..}) {ensure!(operation!=Operation::Delete,"Services can be disabled and restored; deletion is intentionally unavailable");}
        if let Source::Task{xml,..}=&entry.source {if operation==Operation::Delete {ensure!(!xml.contains("<LogonType>Password</LogonType>") && !xml.contains("<LogonType>InteractiveTokenOrPassword</LogonType>"),"Password-backed tasks cannot be restored without credentials. Disable this task instead.");}}
        let mut record=Backup{id:uuid::Uuid::new_v4().to_string(),timestamp:rust_auto_updator::now(),operation,entry:entry.clone(),status:Status::Prepared};
        // Validate and flush the payload before any removal.
        if let Source::Folder{path,digest}=&entry.source { let bytes=system::file_bytes(Path::new(path))?;ensure!(hash(&bytes)==*digest,"File changed since the scan. Refresh and retry.");std::fs::create_dir_all(&self.directory)?;let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(self.directory.join(format!("{}.bin",record.id)))?;file.write_all(&bytes)?;file.sync_all()?; }
        self.save(&record)?;
        let result=self.mutate(entry,operation);
        record.status=match &result{Ok(())=>Status::Applied,Err(e)=>Status::Failed(e.to_string())};
        self.save(&record).context("The action finished but its journal could not be updated. Keep the backup and refresh before retrying.")?;
        result
    }
    fn mutate(&self,entry:&Entry,operation:Operation)->Result<()> {
        match &entry.source {
            Source::Registry{machine,view,key,name,kind,data,approval}=>{
                ensure!(registry::read(*machine,key,*view,name)?==Some((*kind,data.clone())),"Registry entry changed. Refresh and retry.");
                if operation==Operation::Enable {
                    let approval=approval.as_ref().context("This entry has no Windows disable marker")?;
                    ensure!(registry::read(*machine,&approval.key,64,name)?.map(|(_,d)|d)==Some(approval.data.clone()),"Windows startup state changed. Refresh and retry.");
                    let mut enabled=approval.data.clone();ensure!(enabled.len()>=12,"Unrecognized Windows startup marker");enabled[0]=2;enabled[1..].fill(0);registry::write(*machine,&approval.key,64,name,3,&enabled)
                }else{registry::remove(*machine,key,*view,name)}
            }
            Source::Folder{path,digest}=>{
                ensure!(operation!=Operation::Enable,"This file is already present");
                ensure!(hash(&system::file_bytes(Path::new(path))?)==*digest,"File changed. Refresh and retry.");std::fs::remove_file(path).context("Cannot remove startup file. Administrator rights may be required.")
            }
            Source::Task{path,xml}=>{system::bridge(&serde_json::json!({"action":"task","path":path,"xml":xml,"mode":match operation{Operation::Disable=>"disable",Operation::Enable=>"enable",Operation::Delete=>"delete"}}))?;Ok(())}
            Source::Service{name,start,delayed}=>{system::bridge(&serde_json::json!({"action":"service","name":name,"expectedStart":start,"expectedDelayed":delayed,"targetStart":if operation==Operation::Disable{4}else{2},"targetDelayed":if operation==Operation::Disable{false}else{*delayed}}))?;Ok(())}
        }
    }
    pub fn restore(&self,id:&str)->Result<()> {
        let path=self.record_path(id)?;let mut record:Backup=serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(record.id==id,"Backup identity mismatch");ensure!(record.status==Status::Applied,"Only completed changes can be restored automatically. Inspect interrupted changes before recovery.");
        match &record.entry.source {
            Source::Registry{machine,view,key,name,kind,data,approval}=>{
                if record.operation==Operation::Enable {
                    let approval=approval.as_ref().context("Missing original Windows state")?;
                    let current=registry::read(*machine,&approval.key,64,name)?.context("Windows startup marker no longer exists")?;
                    let mut expected=approval.data.clone();ensure!(expected.len()>=12,"Invalid backup marker");expected[0]=2;expected[1..].fill(0);
                    ensure!(current.1==expected,"Windows startup state changed since the action");registry::write(*machine,&approval.key,64,name,3,&approval.data)?;
                }else{ensure!(registry::read(*machine,key,*view,name)?.is_none(),"A startup value already exists with this name. It will not be overwritten.");registry::write(*machine,key,*view,name,*kind,data)?;}
            }
            Source::Folder{path,digest}=>{
                let bytes=std::fs::read(self.directory.join(format!("{id}.bin")))?;ensure!(hash(&bytes)==*digest,"Backup file checksum mismatch");
                let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(path).context("Cannot restore the original file; the destination may already exist or require administrator rights")?;
                if let Err(error)=file.write_all(&bytes).and_then(|_|file.sync_all()){drop(file);let _=std::fs::remove_file(path);return Err(error.into());}
            }
            Source::Task{path,xml}=>{
                if record.operation==Operation::Delete {
                    let logon=if xml.contains("<LogonType>ServiceAccount</LogonType>"){5}else if xml.contains("<LogonType>S4U</LogonType>"){2}else if xml.contains("<LogonType>Group</LogonType>"){4}else{3};
                    system::bridge(&serde_json::json!({"action":"task","mode":"restore","path":path,"xml":xml,"logonType":logon}))?;
                }else{
                    system::bridge(&serde_json::json!({"action":"task-undo","path":path,"xml":xml,"enabled":record.entry.enabled}))?;
                }
            }
            Source::Service{name,start,delayed}=>{
                system::bridge(&serde_json::json!({"action":"service","name":name,"expectedStart":if record.operation==Operation::Disable{4}else{2},"expectedDelayed":if record.operation==Operation::Disable{false}else{*delayed},"targetStart":start,"targetDelayed":delayed}))?;
            }
        }
        record.status=Status::Restored;self.save(&record)
    }
    pub fn add(&self,name:&str,executable:&Path)->Result<()> {
        let name=name.trim();ensure!(!name.is_empty() && name.len()<=120 && !name.contains(['\0','\\','/']),"Enter a name of 1–120 characters without slashes");
        ensure!(executable.is_absolute() && executable.is_file() && executable.extension().is_some_and(|e|e.eq_ignore_ascii_case("exe")),"Choose an existing absolute .exe path");
        let path=executable.to_string_lossy();ensure!(!path.contains(['\0','"']),"Invalid executable path");
        let key=format!("{}\\Run",system::RUN_ROOT);ensure!(registry::read(false,&key,64,name)?.is_none(),"A startup entry with this name already exists");
        registry::write(false,&key,64,name,1,&registry::encode(&format!("\"{path}\"")))
    }
}

#[cfg(test)] mod tests {
    use super::*;
    fn sandbox()->(PathBuf,Engine,Entry){let root=std::env::temp_dir().join(format!("startup-manager-test-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();let path=root.join("fixture.lnk");let bytes=b"fixture only, not a shortcut";std::fs::write(&path,bytes).unwrap();let entry=Entry{id:"fixture".into(),name:"Fixture".into(),command:"".into(),scope:"Test".into(),location:"Test".into(),enabled:true,source:Source::Folder{path:path.to_string_lossy().into(),digest:hash(bytes)},backup_id:None};let engine=Engine{directory:root.join("backups")};(root,engine,entry)}
    #[test] fn disable_restore_preserves_bytes_and_refuses_overwrite(){let(root,engine,entry)=sandbox();engine.apply(&entry,Operation::Disable).unwrap();let backup=engine.backups().unwrap().remove(0);if let Source::Folder{path,..}=&entry.source{assert!(!Path::new(path).exists());std::fs::write(path,b"new owner").unwrap();assert!(engine.restore(&backup.id).is_err());assert_eq!(std::fs::read(path).unwrap(),b"new owner");std::fs::remove_file(path).unwrap();}engine.restore(&backup.id).unwrap();assert_eq!(engine.backups().unwrap()[0].status,Status::Restored);std::fs::remove_dir_all(root).unwrap();}
    #[test] fn stale_scan_and_corrupt_backup_leave_existing_data_intact(){let(root,engine,entry)=sandbox();if let Source::Folder{path,..}=&entry.source{std::fs::write(path,b"changed").unwrap();assert!(engine.apply(&entry,Operation::Delete).is_err());assert_eq!(std::fs::read(path).unwrap(),b"changed");}std::fs::remove_dir_all(root).unwrap();}
    #[test] fn backup_digest_and_identity_are_enforced(){let(root,engine,entry)=sandbox();engine.apply(&entry,Operation::Delete).unwrap();let backup=engine.backups().unwrap().remove(0);std::fs::write(engine.directory.join(format!("{}.bin",backup.id)),b"tampered").unwrap();assert!(engine.restore(&backup.id).is_err());assert!(engine.restore("../escape").is_err());std::fs::remove_dir_all(root).unwrap();}
}
