//! Write-ahead backups precede every mutation. Conflicts never overwrite new data.
use crate::{
    model::{Backup, Entry, Inventory, Operation, Source, Status},
    registry, settings,
    windows_sources::{self as system, hash},
};
use anyhow::{Context, Result, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub struct Engine {
    pub directory: PathBuf,
    startup_folders: Vec<PathBuf>,
    registry_roots: Vec<String>,
}
fn set_approval(
    machine: bool,
    name: &str,
    approval: &crate::model::Approval,
    enable: bool,
) -> Result<()> {
    ensure!(
        approval.data.len() >= 12,
        "Unrecognized Windows startup marker"
    );
    let mut enabled = approval.data.clone();
    enabled[0] = 2;
    enabled[1..].fill(0);
    let expected = if enable { &approval.data } else { &enabled };
    let target = if enable { &enabled } else { &approval.data };
    ensure!(
        registry::read(machine, &approval.key, 64, name)? == Some((3, expected.clone())),
        "Windows startup state changed. Refresh and retry."
    );
    registry::write(machine, &approval.key, 64, name, 3, target)
}
fn check_original_approval(
    machine: bool,
    key: &str,
    name: &str,
    approval: &Option<crate::model::Approval>,
) -> Result<()> {
    let expected = approval.as_ref().map(|value| (3, value.data.clone()));
    ensure!(
        registry::read(machine, key, 64, name)? == expected,
        "Windows startup approval changed since the backup. Refresh and review it before restoring."
    );
    Ok(())
}
impl Engine {
    pub fn new() -> Self {
        Self {
            directory: settings::directory().join("backups"),
            startup_folders: system::startup_directories()
                .into_iter()
                .map(|(path, _)| path)
                .collect(),
            registry_roots: ["Run", "RunOnce"]
                .map(|leaf| format!("{}\\{leaf}", system::RUN_ROOT))
                .to_vec(),
        }
    }
    fn validate(&self, entry: &Entry) -> Result<()> {
        let valid_approval = |approval: &Option<crate::model::Approval>| -> Result<()> {
            if let Some(approval) = approval {
                let prefix =
                    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\";
                ensure!(
                    ["Run", "Run32", "StartupFolder"].iter().any(|leaf| approval
                        .key
                        .eq_ignore_ascii_case(&format!("{prefix}{leaf}")))
                        && approval.data.len() >= 12
                        && approval.data.len() <= 1024,
                    "Invalid Windows startup backup marker"
                );
            }
            Ok(())
        };
        match &entry.source {
            Source::Registry {
                key,
                view,
                kind,
                data,
                name,
                approval,
                ..
            } => {
                ensure!(
                    self.registry_roots
                        .iter()
                        .any(|root| root.eq_ignore_ascii_case(key))
                        && [32, 64].contains(view)
                        && [1, 2].contains(kind)
                        && data.len() <= 1024 * 1024
                        && !name.contains('\0'),
                    "Backup points outside supported startup registry locations"
                );
                valid_approval(approval)?;
            }
            Source::Folder { path, approval, .. } => {
                let path = Path::new(path);
                ensure!(path.is_absolute(), "Startup file path must be absolute");
                let parent = path
                    .parent()
                    .context("Missing startup directory")?
                    .canonicalize()?;
                ensure!(
                    self.startup_folders
                        .iter()
                        .any(|root| root.canonicalize().is_ok_and(|root| root == parent)),
                    "Backup file is outside a supported startup folder"
                );
                valid_approval(approval)?;
            }
            Source::Task { path, xml } => ensure!(
                path.starts_with('\\') && !path.contains('\0') && xml.len() <= 4 * 1024 * 1024,
                "Invalid scheduled task backup"
            ),
            Source::Service { name, start, .. } => ensure!(
                !name.is_empty() && !name.contains(['\0', '\\', '/']) && [2, 4].contains(start),
                "Invalid service backup"
            ),
        }
        Ok(())
    }
    pub fn backups(&self) -> Result<Vec<Backup>> {
        if !self.directory.exists() {
            return Ok(vec![]);
        }
        let mut items = vec![];
        for file in std::fs::read_dir(&self.directory)? {
            let path = file?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let record: Backup =
                    serde_json::from_slice(&std::fs::read(&path)?).with_context(|| {
                        format!(
                            "Cannot read backup {}. Original backups have been preserved.",
                            path.display()
                        )
                    })?;
                ensure!(
                    path.file_stem().and_then(|s| s.to_str()) == Some(&record.id),
                    "Backup identity mismatch"
                );
                items.push(record);
            }
        }
        items.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        Ok(items)
    }
    fn record_path(&self, id: &str) -> Result<PathBuf> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "Invalid backup identity");
        Ok(self.directory.join(format!("{id}.json")))
    }
    fn save(&self, record: &Backup) -> Result<()> {
        settings::write_json(&self.record_path(&record.id)?, record)
    }
    pub fn merge_disabled(&self, report: &mut Inventory) -> Result<()> {
        for backup in self.backups()? {
            if backup.status == Status::Applied
                && backup.operation == Operation::Disable
                && matches!(
                    backup.entry.source,
                    Source::Registry { .. } | Source::Folder { .. }
                )
                && !report.entries.iter().any(|e| e.id == backup.entry.id)
            {
                let mut entry = backup.entry;
                entry.enabled = false;
                entry.backup_id = Some(backup.id);
                report.entries.push(entry);
            }
        }
        report.entries.sort_by_key(|e| e.name.to_lowercase());
        Ok(())
    }
    pub fn apply(&self, entry: &Entry, operation: Operation) -> Result<()> {
        self.validate(entry)?;
        ensure!(
            operation != Operation::Add,
            "Use Add app to create a startup entry"
        );
        if let Some(id) = &entry.backup_id {
            ensure!(
                operation == Operation::Enable,
                "This item is already backed up. Use Restore in History to recover it."
            );
            return self.restore(id);
        }
        ensure!(
            operation == Operation::Delete || entry.enabled != (operation == Operation::Enable),
            "The item already has the requested state"
        );
        if matches!(entry.source, Source::Service { .. }) {
            ensure!(
                operation != Operation::Delete,
                "Services can be disabled and restored; deletion is intentionally unavailable"
            );
        }
        if let Source::Task { xml, .. } = &entry.source
            && operation == Operation::Delete
        {
            ensure!(
                !xml.contains("<LogonType>Password</LogonType>")
                    && !xml.contains("<LogonType>InteractiveTokenOrPassword</LogonType>"),
                "Password-backed tasks cannot be restored without credentials. Disable this task instead."
            );
        }
        let mut record = Backup {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: rust_auto_updator::now(),
            operation,
            entry: entry.clone(),
            status: Status::Prepared,
            post_source: None,
        };
        // Validate and flush the payload before any removal.
        if let Source::Folder { path, digest, .. } = &entry.source {
            let bytes = system::file_bytes(Path::new(path))?;
            ensure!(
                hash(&bytes) == *digest,
                "File changed since the scan. Refresh and retry."
            );
            std::fs::create_dir_all(&self.directory)?;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(self.directory.join(format!("{}.bin", record.id)))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        self.save(&record)
            .context("Cannot save the recovery journal before the change")?;
        let result = self.mutate(entry, operation);
        if let Ok(source) = &result {
            record.post_source = source.clone();
        }
        record.status = match &result {
            Ok(_) => Status::Applied,
            Err(e) => Status::Failed(e.to_string()),
        };
        self.save(&record).context("The action finished but its journal could not be updated. Keep the backup and refresh before retrying.")?;
        result.map(|_| ())
    }
    fn mutate(&self, entry: &Entry, operation: Operation) -> Result<Option<Source>> {
        match &entry.source {
            Source::Registry {
                machine,
                view,
                key,
                name,
                kind,
                data,
                approval,
            } => {
                ensure!(
                    registry::read(*machine, key, *view, name)? == Some((*kind, data.clone())),
                    "Registry entry changed. Refresh and retry."
                );
                if operation == Operation::Enable {
                    let approval = approval
                        .as_ref()
                        .context("This entry has no Windows disable marker")?;
                    ensure!(
                        registry::read(*machine, &approval.key, 64, name)?.map(|(_, d)| d)
                            == Some(approval.data.clone()),
                        "Windows startup state changed. Refresh and retry."
                    );
                    let mut enabled = approval.data.clone();
                    ensure!(enabled.len() >= 12, "Unrecognized Windows startup marker");
                    enabled[0] = 2;
                    enabled[1..].fill(0);
                    registry::write(*machine, &approval.key, 64, name, 3, &enabled)?;
                } else {
                    registry::remove(*machine, key, *view, name)?;
                }
                Ok(None)
            }
            Source::Folder {
                path,
                digest,
                machine,
                approval,
            } => {
                ensure!(
                    hash(&system::file_bytes(Path::new(path))?) == *digest,
                    "File changed. Refresh and retry."
                );
                if operation == Operation::Enable {
                    let approval = approval
                        .as_ref()
                        .context("This startup file is already enabled")?;
                    let name = Path::new(path)
                        .file_name()
                        .context("Invalid startup file path")?
                        .to_string_lossy();
                    set_approval(*machine, &name, approval, true)?;
                } else {
                    std::fs::remove_file(path).context(
                        "Cannot remove startup file. Administrator rights may be required.",
                    )?;
                }
                Ok(None)
            }
            Source::Task { path, xml } => {
                let result = system::bridge(
                    &serde_json::json!({"action":"task","path":path,"xml":xml,"mode":match operation{Operation::Disable=>"disable",Operation::Enable=>"enable",Operation::Delete=>"delete",Operation::Add=>return Err(anyhow::anyhow!("Tasks cannot be created here"))}}),
                )?;
                if operation == Operation::Delete {
                    Ok(None)
                } else {
                    Ok(Some(Source::Task {
                        path: path.clone(),
                        xml: result["xml"]
                            .as_str()
                            .context("Missing post-change task snapshot")?
                            .into(),
                    }))
                }
            }
            Source::Service {
                name,
                start,
                delayed,
            } => {
                let result = system::bridge(
                    &serde_json::json!({"action":"service","name":name,"expectedStart":start,"expectedDelayed":delayed,"targetStart":if operation==Operation::Disable{4}else{2},"targetDelayed":if operation==Operation::Disable{false}else{*delayed}}),
                )?;
                Ok(Some(Source::Service {
                    name: name.clone(),
                    start: result["start"]
                        .as_u64()
                        .context("Missing post-change service snapshot")?
                        as u32,
                    delayed: result["delayed"]
                        .as_bool()
                        .context("Missing post-change service snapshot")?,
                }))
            }
        }
    }
    pub fn restore(&self, id: &str) -> Result<()> {
        let path = self.record_path(id)?;
        let mut record: Backup = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(record.id == id, "Backup identity mismatch");
        self.validate(&record.entry)?;
        ensure!(
            record.status == Status::Applied,
            "Only completed changes can be restored automatically. Inspect interrupted changes before recovery."
        );
        match &record.entry.source {
            Source::Registry {
                machine,
                view,
                key,
                name,
                kind,
                data,
                approval,
            } => {
                if record.operation == Operation::Add {
                    ensure!(
                        registry::read(*machine, key, *view, name)? == Some((*kind, data.clone())),
                        "The added entry changed since creation. It will not be removed automatically."
                    );
                    registry::remove(*machine, key, *view, name)?;
                } else if record.operation == Operation::Enable {
                    ensure!(
                        registry::read(*machine, key, *view, name)? == Some((*kind, data.clone())),
                        "The startup command changed since the action"
                    );
                    let approval = approval
                        .as_ref()
                        .context("Missing original Windows state")?;
                    let current = registry::read(*machine, &approval.key, 64, name)?
                        .context("Windows startup marker no longer exists")?;
                    let mut expected = approval.data.clone();
                    ensure!(expected.len() >= 12, "Invalid backup marker");
                    expected[0] = 2;
                    expected[1..].fill(0);
                    ensure!(
                        current.1 == expected,
                        "Windows startup state changed since the action"
                    );
                    registry::write(*machine, &approval.key, 64, name, 3, &approval.data)?;
                } else {
                    ensure!(
                        registry::read(*machine, key, *view, name)?.is_none(),
                        "A startup value already exists with this name. It will not be overwritten."
                    );
                    if key.eq_ignore_ascii_case(&format!("{}\\Run", system::RUN_ROOT)) {
                        let leaf = if *machine && *view == 32 {
                            "Run32"
                        } else {
                            "Run"
                        };
                        check_original_approval(
                            *machine,
                            &format!(
                                "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\{leaf}"
                            ),
                            name,
                            approval,
                        )?;
                    }
                    registry::write(*machine, key, *view, name, *kind, data)?;
                }
            }
            Source::Folder {
                path,
                digest,
                machine,
                approval,
            } => {
                if record.operation == Operation::Enable {
                    ensure!(
                        hash(&system::file_bytes(Path::new(path))?) == *digest,
                        "Startup file changed since the action"
                    );
                    let name = Path::new(path)
                        .file_name()
                        .context("Invalid startup file path")?
                        .to_string_lossy();
                    set_approval(
                        *machine,
                        &name,
                        approval
                            .as_ref()
                            .context("Missing Windows startup marker")?,
                        false,
                    )?;
                } else {
                    let name = Path::new(path)
                        .file_name()
                        .context("Invalid startup file path")?
                        .to_string_lossy();
                    check_original_approval(
                        *machine,
                        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder",
                        &name,
                        approval,
                    )?;
                    let bytes = std::fs::read(self.directory.join(format!("{id}.bin")))?;
                    ensure!(hash(&bytes) == *digest, "Backup file checksum mismatch");
                    let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(path).context("Cannot restore the original file; the destination may already exist or require administrator rights")?;
                    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                        drop(file);
                        let _ = std::fs::remove_file(path);
                        return Err(error.into());
                    }
                }
            }
            Source::Task { path, xml } => {
                if record.operation == Operation::Delete {
                    let logon = if xml.contains("<LogonType>ServiceAccount</LogonType>") {
                        5
                    } else if xml.contains("<LogonType>S4U</LogonType>") {
                        2
                    } else if xml.contains("<LogonType>Group</LogonType>") {
                        4
                    } else {
                        3
                    };
                    system::bridge(
                        &serde_json::json!({"action":"task","mode":"restore","path":path,"xml":xml,"logonType":logon}),
                    )?;
                } else {
                    system::bridge(
                        &serde_json::json!({"action":"task-undo","path":path,"expectedXml":match &record.post_source{Some(Source::Task{xml,..})=>xml,_=>anyhow::bail!("Missing post-change task snapshot; inspect the task before recovery")},"enabled":record.entry.enabled}),
                    )?;
                }
            }
            Source::Service {
                name,
                start,
                delayed,
            } => {
                system::bridge(
                    &serde_json::json!({"action":"service","name":name,"expectedStart":match &record.post_source{Some(Source::Service{start,..})=>*start,_=>anyhow::bail!("Missing post-change service snapshot")},"expectedDelayed":match &record.post_source{Some(Source::Service{delayed,..})=>*delayed,_=>anyhow::bail!("Missing post-change service snapshot")},"targetStart":start,"targetDelayed":delayed}),
                )?;
            }
        }
        record.status = Status::Restored;
        self.save(&record)
    }
    pub fn add(&self, name: &str, executable: &Path) -> Result<()> {
        let name = name.trim();
        ensure!(
            !name.is_empty() && name.len() <= 120 && !name.contains(['\0', '\\', '/']),
            "Enter a name of 1–120 characters without slashes"
        );
        ensure!(
            executable.is_absolute()
                && executable.is_file()
                && executable
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe")),
            "Choose an existing absolute .exe path"
        );
        let path = executable.to_string_lossy();
        ensure!(!path.contains(['\0', '"']), "Invalid executable path");
        let key = format!("{}\\Run", system::RUN_ROOT);
        ensure!(
            registry::read(false, &key, 64, name)?.is_none(),
            "A startup entry with this name already exists"
        );
        let approval_key =
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
        ensure!(
            registry::read(false, approval_key, 64, name)?
                .is_none_or(|(_, data)| !matches!(data.first(), Some(3 | 7))),
            "Windows has disabled a startup entry with this name. Choose another name or enable its existing entry first."
        );
        let command = format!("\"{path}\"");
        let data = registry::encode(&command);
        let entry = Entry {
            id: format!("reg:false:64:{key}:{name}"),
            name: name.into(),
            command,
            location: format!("HKCU\\{key}"),
            scope: "Current user".into(),
            enabled: true,
            source: Source::Registry {
                machine: false,
                view: 64,
                key: key.clone(),
                name: name.into(),
                kind: 1,
                data: data.clone(),
                approval: None,
            },
            backup_id: None,
        };
        let mut record = Backup {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: rust_auto_updator::now(),
            operation: Operation::Add,
            entry,
            status: Status::Prepared,
            post_source: None,
        };
        self.save(&record)?;
        let result = registry::write(false, &key, 64, name, 1, &data);
        record.status = match &result {
            Ok(()) => Status::Applied,
            Err(e) => Status::Failed(e.to_string()),
        };
        self.save(&record)?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sandbox() -> (PathBuf, Engine, Entry) {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-data")
            .join(format!("startup-manager-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.lnk");
        let bytes = b"fixture only, not a shortcut";
        std::fs::write(&path, bytes).unwrap();
        let entry = Entry {
            id: "fixture".into(),
            name: "Fixture".into(),
            command: "".into(),
            scope: "Test".into(),
            location: "Test".into(),
            enabled: true,
            source: Source::Folder {
                path: path.to_string_lossy().into(),
                digest: hash(bytes),
                machine: false,
                approval: None,
            },
            backup_id: None,
        };
        let engine = Engine {
            directory: root.join("backups"),
            startup_folders: vec![root.clone()],
            registry_roots: vec![],
        };
        (root, engine, entry)
    }
    #[test]
    fn native_registry_disable_restore_preserves_expand_strings_and_conflicts() {
        let key = registry::TestKey::create();
        let (root, mut engine, mut entry) = sandbox();
        engine.registry_roots = vec![key.0.clone()];
        let original = registry::encode("\"%LOCALAPPDATA%\\Пример\\app.exe\" /silent");
        registry::write(false, &key.0, 64, "Fixture", 2, &original).unwrap();
        entry.source = Source::Registry {
            machine: false,
            view: 64,
            key: key.0.clone(),
            name: "Fixture".into(),
            kind: 2,
            data: original.clone(),
            approval: None,
        };
        engine.apply(&entry, Operation::Disable).unwrap();
        assert!(
            registry::read(false, &key.0, 64, "Fixture")
                .unwrap()
                .is_none()
        );
        let id = engine.backups().unwrap()[0].id.clone();
        registry::write(
            false,
            &key.0,
            64,
            "Fixture",
            1,
            &registry::encode("new owner"),
        )
        .unwrap();
        assert!(engine.restore(&id).is_err());
        registry::remove(false, &key.0, 64, "Fixture").unwrap();
        engine.restore(&id).unwrap();
        assert_eq!(
            registry::read(false, &key.0, 64, "Fixture").unwrap(),
            Some((2, original))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recovery_rejects_sources_outside_startup_locations() {
        let (root, engine, mut entry) = sandbox();
        entry.source = Source::Registry {
            machine: true,
            view: 64,
            key: "SYSTEM\\CurrentControlSet\\Control".into(),
            name: "Fixture".into(),
            kind: 1,
            data: vec![],
            approval: None,
        };
        assert!(engine.apply(&entry, Operation::Delete).is_err());
        assert!(engine.backups().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "Creates one disposable scheduled task with no triggers; run explicitly on Windows"]
    fn native_task_enable_undo_delete_restore_and_conflict_check() {
        struct Fixture(String);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = system::bridge(
                    &serde_json::json!({"action":"task-fixture-cleanup","path":self.0}),
                );
            }
        }
        let path = format!("\\StartupManager-Test-{}", uuid::Uuid::new_v4());
        let created =
            system::bridge(&serde_json::json!({"action":"task-fixture-create","path":path}))
                .unwrap();
        let fixture = Fixture(path.clone());
        let (root, engine, mut entry) = sandbox();
        entry.id = format!("task:{path}");
        entry.enabled = false;
        entry.source = Source::Task {
            path: path.clone(),
            xml: created["xml"].as_str().unwrap().into(),
        };
        engine.apply(&entry, Operation::Enable).unwrap();
        let backup = engine.backups().unwrap().remove(0);
        assert!(system::bridge(&serde_json::json!({"action":"task-read","path":path})).unwrap()["enabled"].as_bool().unwrap());
        engine.restore(&backup.id).unwrap();
        let restored =
            system::bridge(&serde_json::json!({"action":"task-read","path":path})).unwrap();
        assert!(!restored["enabled"].as_bool().unwrap());
        entry.source = Source::Task {
            path: path.clone(),
            xml: restored["xml"].as_str().unwrap().into(),
        };
        engine.apply(&entry, Operation::Delete).unwrap();
        let deleted = engine
            .backups()
            .unwrap()
            .into_iter()
            .find(|b| b.operation == Operation::Delete)
            .unwrap();
        assert!(system::bridge(&serde_json::json!({"action":"task-read","path":path})).is_err());
        engine.restore(&deleted.id).unwrap();
        let current =
            system::bridge(&serde_json::json!({"action":"task-read","path":path})).unwrap();
        assert!(!current["enabled"].as_bool().unwrap());
        if let Source::Task { xml, .. } = &mut entry.source {
            xml.push_str("stale");
        }
        assert!(engine.apply(&entry, Operation::Enable).is_err());
        drop(fixture);
        assert!(system::bridge(&serde_json::json!({"action":"task-read","path":path})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_windows_approval_roundtrip_and_external_conflict() {
        let key = registry::TestKey::create();
        let mut data = vec![0u8; 12];
        data[0] = 3;
        data[4..].copy_from_slice(&12345678u64.to_le_bytes());
        let approval = crate::model::Approval {
            key: key.0.clone(),
            data: data.clone(),
        };
        registry::write(false, &key.0, 64, "Fixture", 3, &data).unwrap();
        set_approval(false, "Fixture", &approval, true).unwrap();
        let enabled = registry::read(false, &key.0, 64, "Fixture")
            .unwrap()
            .unwrap()
            .1;
        assert_eq!(enabled[0], 2);
        assert!(enabled[1..].iter().all(|b| *b == 0));
        set_approval(false, "Fixture", &approval, false).unwrap();
        assert_eq!(
            registry::read(false, &key.0, 64, "Fixture").unwrap(),
            Some((3, data))
        );
        assert!(set_approval(false, "Fixture", &approval, false).is_err());
    }
    #[test]
    fn disable_restore_preserves_bytes_and_refuses_overwrite() {
        let (root, engine, entry) = sandbox();
        engine.apply(&entry, Operation::Disable).unwrap();
        let backup = engine.backups().unwrap().remove(0);
        if let Source::Folder { path, .. } = &entry.source {
            assert!(!Path::new(path).exists());
            std::fs::write(path, b"new owner").unwrap();
            assert!(engine.restore(&backup.id).is_err());
            assert_eq!(std::fs::read(path).unwrap(), b"new owner");
            std::fs::remove_file(path).unwrap();
        }
        engine.restore(&backup.id).unwrap();
        assert_eq!(engine.backups().unwrap()[0].status, Status::Restored);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stale_scan_and_corrupt_backup_leave_existing_data_intact() {
        let (root, engine, entry) = sandbox();
        if let Source::Folder { path, .. } = &entry.source {
            std::fs::write(path, b"changed").unwrap();
            assert!(engine.apply(&entry, Operation::Delete).is_err());
            assert_eq!(std::fs::read(path).unwrap(), b"changed");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn backup_digest_and_identity_are_enforced() {
        let (root, engine, entry) = sandbox();
        engine.apply(&entry, Operation::Delete).unwrap();
        let backup = engine.backups().unwrap().remove(0);
        std::fs::write(
            engine.directory.join(format!("{}.bin", backup.id)),
            b"tampered",
        )
        .unwrap();
        assert!(engine.restore(&backup.id).is_err());
        assert!(engine.restore("../escape").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
