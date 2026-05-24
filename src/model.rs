use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Source {
    Registry {
        machine: bool,
        view: u32,
        key: String,
        name: String,
        kind: u32,
        data: Vec<u8>,
        approval: Option<Approval>,
    },
    Folder {
        path: String,
        digest: String,
        #[serde(default)]
        machine: bool,
        #[serde(default)]
        approval: Option<Approval>,
    },
    Task {
        path: String,
        xml: String,
    },
    Service {
        name: String,
        start: u32,
        delayed: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Approval {
    pub key: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub location: String,
    pub scope: String,
    pub enabled: bool,
    pub source: Source,
    #[serde(default)]
    pub backup_id: Option<String>,
}

impl Entry {
    pub fn category(&self) -> &'static str {
        match &self.source {
            Source::Registry { .. } | Source::Folder { .. } => "Startup apps",
            Source::Task { .. } => "Scheduled tasks",
            Source::Service { .. } => "Services",
        }
    }
    pub fn source_name(&self) -> &'static str {
        match &self.source {
            Source::Registry { .. } => "Registry",
            Source::Folder { .. } => "Startup folder",
            Source::Task { .. } => "Scheduled task",
            Source::Service { .. } => "Service",
        }
    }
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        [&self.name, &self.command, &self.location, &self.scope]
            .iter()
            .any(|s| s.to_lowercase().contains(&query))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Operation {
    Add,
    Disable,
    Enable,
    Delete,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Prepared,
    Applied,
    Restored,
    Failed(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Backup {
    pub id: String,
    pub timestamp: u64,
    pub operation: Operation,
    pub entry: Entry,
    pub status: Status,
    #[serde(default)]
    pub post_source: Option<Source>,
}

pub fn fixtures() -> Vec<Entry> {
    let names = [
        (
            "Microsoft OneDrive",
            "\"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe\" /background",
            true,
        ),
        (
            "Spotify",
            "\"C:\\Users\\Alex\\AppData\\Roaming\\Spotify\\Spotify.exe\" --autostart",
            true,
        ),
        (
            "Discord",
            "\"C:\\Users\\Alex\\AppData\\Local\\Discord\\Update.exe\" --processStart Discord.exe",
            false,
        ),
        (
            "Steam",
            "\"C:\\Program Files (x86)\\Steam\\steam.exe\" -silent",
            true,
        ),
        (
            "Adobe Creative Cloud",
            "\"C:\\Program Files\\Adobe\\Creative Cloud\\ACC\\Creative Cloud.exe\"",
            true,
        ),
        (
            "Obsidian",
            "\"C:\\Users\\Alex\\AppData\\Local\\Obsidian\\Obsidian.exe\"",
            false,
        ),
    ];
    names
        .into_iter()
        .enumerate()
        .map(|(i, (name, command, enabled))| Entry {
            id: format!("fixture-{i}"),
            name: name.into(),
            command: command.into(),
            location: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
            scope: "Current user".into(),
            enabled,
            source: Source::Registry {
                machine: false,
                view: 64,
                key: "Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
                name: name.into(),
                kind: 1,
                data: vec![],
                approval: None,
            },
            backup_id: None,
        })
        .collect()
}

/// Real installed Windows executables, represented as isolated startup examples.
/// These paths supply their actual shell icons; no registration is performed.
pub fn screen_fixtures() -> Vec<Entry> {
    let windows = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "C:\\Windows".into());
    [
        ("File Explorer", "explorer.exe"),
        ("Notepad", "System32\\notepad.exe"),
        ("Task Manager", "System32\\Taskmgr.exe"),
        ("Registry Editor", "regedit.exe"),
        ("Command Prompt", "System32\\cmd.exe"),
        (
            "PowerShell",
            "System32\\WindowsPowerShell\\v1.0\\powershell.exe",
        ),
        ("Character Map", "System32\\charmap.exe"),
        ("Resource Monitor", "System32\\resmon.exe"),
        ("System Configuration", "System32\\msconfig.exe"),
        ("Magnifier", "System32\\Magnify.exe"),
        ("Disk Cleanup", "System32\\cleanmgr.exe"),
        ("Calculator", "System32\\calc.exe"),
        ("Services Console", "System32\\mmc.exe"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, path))| {
        let mut entry = fixtures()[0].clone();
        entry.id = format!("screen-{index}");
        entry.name = name.into();
        entry.command = format!("\"{}\"", windows.join(path).display());
        entry.enabled = index != 3 && index != 7;
        if let Source::Registry {
            machine,
            name: registry_name,
            ..
        } = &mut entry.source
        {
            *registry_name = name.into();
            *machine = index % 3 == 0;
        }
        entry.scope = if index % 3 == 0 {
            "All users"
        } else {
            "Current user"
        }
        .into();
        entry
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_includes_command_scope_and_location_case_insensitively() {
        let entry = &fixtures()[0];
        assert!(entry.matches("ONEDRIVE"));
        assert!(entry.matches("current USER"));
        assert!(entry.matches("CurrentVersion"));
        assert!(!entry.matches("unrelated"));
    }
}
