use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tracing::info;

// --- Template types ---

/// Detected format of a template file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateFormat {
    Ignition,
    Kickstart,
    CloudConfig,
}

impl TemplateFormat {
    /// Detect format from file extension.
    pub fn from_filename(name: &str) -> Self {
        if name.ends_with(".ign.json") {
            TemplateFormat::Ignition
        } else if name.ends_with(".ks") {
            TemplateFormat::Kickstart
        } else {
            TemplateFormat::CloudConfig
        }
    }

    pub fn as_content_type(&self) -> &'static str {
        match self {
            TemplateFormat::Ignition => "application/json",
            TemplateFormat::Kickstart => "text/plain",
            TemplateFormat::CloudConfig => "text/yaml",
        }
    }
}

/// Template execution mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateMode {
    /// First boot only, then reverts to local disk boot.
    Oneshot,
    /// Served on every boot.
    #[default]
    Forever,
}

/// A loaded template with metadata and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedTemplate {
    pub image_type: String,
    pub name: String,
    pub format: TemplateFormat,
    pub mode: TemplateMode,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Summary entry for listing templates (no content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSummary {
    pub image_type: String,
    pub name: String,
    pub format: TemplateFormat,
    pub mode: TemplateMode,
}

/// Response for listing templates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateListResponse {
    pub templates: Vec<TemplateSummary>,
}

/// Host-to-template assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub image_type: String,
    pub template: String,
}

/// Assignments file stored on PVC.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssignmentsFile {
    pub assignments: HashMap<String, Assignment>,
}

/// Oneshot completion state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OneshotState {
    pub completed: HashMap<String, String>, // hostname -> completion timestamp
}

/// Backup/restore bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBundle {
    pub version: u32,
    pub exported_at: String,
    pub templates: Vec<BundleEntry>,
}

/// A single template entry in a backup bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEntry {
    pub image_type: String,
    pub name: String,
    pub format: TemplateFormat,
    pub mode: TemplateMode,
    pub content: String,
}

/// PUT request body for creating/updating a template.
#[derive(Debug, Clone, Deserialize)]
pub struct TemplateCreateRequest {
    #[serde(default)]
    pub mode: TemplateMode,
    pub content: String,
}

/// PUT request body for assigning a template to a host.
#[derive(Debug, Clone, Deserialize)]
pub struct AssignmentRequest {
    pub image_type: String,
    pub template: String,
}

// --- Template metadata file (stored alongside each template) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TemplateMeta {
    mode: TemplateMode,
    created_at: String,
    updated_at: String,
}

// --- File-based template store ---

/// Template store backed by PVC filesystem.
pub struct TemplateStore {
    data_dir: PathBuf,
}

impl TemplateStore {
    pub fn new(data_dir: &str) -> Self {
        Self {
            data_dir: PathBuf::from(data_dir),
        }
    }

    fn templates_dir(&self) -> PathBuf {
        self.data_dir.join("templates")
    }

    fn assignments_path(&self) -> PathBuf {
        self.data_dir.join("assignments.json")
    }

    fn oneshot_path(&self) -> PathBuf {
        self.data_dir.join("oneshot.json")
    }

    fn template_path(&self, image_type: &str, name: &str) -> PathBuf {
        self.templates_dir().join(image_type).join(name)
    }

    fn meta_path(&self, image_type: &str, name: &str) -> PathBuf {
        self.templates_dir()
            .join(image_type)
            .join(format!("{}.meta.json", name))
    }

    /// Ensure base directories exist on PVC. Called at startup.
    pub async fn init(&self) -> anyhow::Result<()> {
        let data_dir = &self.data_dir;
        let templates_dir = self.templates_dir();

        // Create the data directory if it doesn't exist (PVC should provide it)
        if !data_dir.exists() {
            info!(path = %data_dir.display(), "creating data directory");
            fs::create_dir_all(data_dir).await?;
        }

        // Create the templates subdirectory
        if !templates_dir.exists() {
            info!(path = %templates_dir.display(), "creating templates directory");
            fs::create_dir_all(&templates_dir).await?;
        }

        info!(path = %data_dir.display(), "template store initialized");
        Ok(())
    }

    // --- Template CRUD ---

    /// List all templates across all image types.
    pub async fn list_all(&self) -> anyhow::Result<Vec<TemplateSummary>> {
        let mut result = Vec::new();
        let templates_dir = self.templates_dir();

        if !templates_dir.exists() {
            return Ok(result);
        }

        let mut image_types = fs::read_dir(&templates_dir).await?;
        while let Some(entry) = image_types.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }
            let image_type = entry.file_name().to_string_lossy().to_string();
            let mut files = fs::read_dir(entry.path()).await?;
            while let Some(file) = files.next_entry().await? {
                let fname = file.file_name().to_string_lossy().to_string();
                // Skip .meta.json files
                if fname.ends_with(".meta.json") {
                    continue;
                }
                let meta = self.read_meta(&image_type, &fname).await;
                let mode = meta.as_ref().map(|m| m.mode).unwrap_or_default();
                result.push(TemplateSummary {
                    image_type: image_type.clone(),
                    name: fname.clone(),
                    format: TemplateFormat::from_filename(&fname),
                    mode,
                });
            }
        }

        result.sort_by(|a, b| (&a.image_type, &a.name).cmp(&(&b.image_type, &b.name)));
        Ok(result)
    }

    /// List templates for a specific image type.
    pub async fn list_by_type(&self, image_type: &str) -> anyhow::Result<Vec<TemplateSummary>> {
        let mut result = Vec::new();
        let type_dir = self.templates_dir().join(image_type);

        if !type_dir.exists() {
            return Ok(result);
        }

        let mut files = fs::read_dir(&type_dir).await?;
        while let Some(file) = files.next_entry().await? {
            let fname = file.file_name().to_string_lossy().to_string();
            if fname.ends_with(".meta.json") {
                continue;
            }
            let meta = self.read_meta(image_type, &fname).await;
            let mode = meta.as_ref().map(|m| m.mode).unwrap_or_default();
            result.push(TemplateSummary {
                image_type: image_type.to_string(),
                name: fname.clone(),
                format: TemplateFormat::from_filename(&fname),
                mode,
            });
        }

        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }

    /// Get a single template with full content.
    pub async fn get(
        &self,
        image_type: &str,
        name: &str,
    ) -> anyhow::Result<Option<LoadedTemplate>> {
        let path = self.template_path(image_type, name);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path).await?;
        let meta = self
            .read_meta(image_type, name)
            .await
            .unwrap_or_else(|| TemplateMeta {
                mode: TemplateMode::default(),
                created_at: now_utc(),
                updated_at: now_utc(),
            });

        Ok(Some(LoadedTemplate {
            image_type: image_type.to_string(),
            name: name.to_string(),
            format: TemplateFormat::from_filename(name),
            mode: meta.mode,
            content,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
        }))
    }

    /// Create or update a template.
    pub async fn put(
        &self,
        image_type: &str,
        name: &str,
        req: &TemplateCreateRequest,
    ) -> anyhow::Result<LoadedTemplate> {
        let type_dir = self.templates_dir().join(image_type);
        fs::create_dir_all(&type_dir).await?;

        let path = self.template_path(image_type, name);
        let meta_path = self.meta_path(image_type, name);

        // Preserve created_at if updating
        let existing_meta = self.read_meta(image_type, name).await;
        let is_new = existing_meta.is_none();
        let created_at = existing_meta
            .map(|m| m.created_at)
            .unwrap_or_else(now_utc);
        let updated_at = now_utc();

        let meta = TemplateMeta {
            mode: req.mode,
            created_at: created_at.clone(),
            updated_at: updated_at.clone(),
        };

        fs::write(&path, &req.content).await?;
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        if is_new {
            info!(image_type, name, "template created");
        } else {
            info!(image_type, name, "template updated");
        }

        Ok(LoadedTemplate {
            image_type: image_type.to_string(),
            name: name.to_string(),
            format: TemplateFormat::from_filename(name),
            mode: req.mode,
            content: req.content.clone(),
            created_at,
            updated_at,
        })
    }

    /// Delete a template.
    pub async fn delete(&self, image_type: &str, name: &str) -> anyhow::Result<bool> {
        let path = self.template_path(image_type, name);
        let meta_path = self.meta_path(image_type, name);

        if !path.exists() {
            return Ok(false);
        }

        fs::remove_file(&path).await?;
        if meta_path.exists() {
            let _ = fs::remove_file(&meta_path).await;
        }

        info!(image_type, name, "template deleted");
        Ok(true)
    }

    async fn read_meta(&self, image_type: &str, name: &str) -> Option<TemplateMeta> {
        let path = self.meta_path(image_type, name);
        match fs::read_to_string(&path).await {
            Ok(data) => serde_json::from_str(&data).ok(),
            Err(_) => None,
        }
    }

    // --- Backup / Restore ---

    /// Export all templates as a bundle.
    pub async fn backup(&self) -> anyhow::Result<TemplateBundle> {
        let summaries = self.list_all().await?;
        let mut entries = Vec::new();

        for s in &summaries {
            if let Some(tpl) = self.get(&s.image_type, &s.name).await? {
                entries.push(BundleEntry {
                    image_type: tpl.image_type,
                    name: tpl.name,
                    format: tpl.format,
                    mode: tpl.mode,
                    content: tpl.content,
                });
            }
        }

        Ok(TemplateBundle {
            version: 1,
            exported_at: now_utc(),
            templates: entries,
        })
    }

    /// Import templates from a bundle. Overwrites existing templates with same name.
    pub async fn restore(&self, bundle: &TemplateBundle) -> anyhow::Result<usize> {
        let mut count = 0;
        for entry in &bundle.templates {
            self.put(
                &entry.image_type,
                &entry.name,
                &TemplateCreateRequest {
                    mode: entry.mode,
                    content: entry.content.clone(),
                },
            )
            .await?;
            count += 1;
        }
        info!(count, "templates restored from bundle");
        Ok(count)
    }

    // --- Assignments ---

    /// Load assignments from PVC.
    pub async fn load_assignments(&self) -> AssignmentsFile {
        let path = self.assignments_path();
        match fs::read_to_string(&path).await {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => AssignmentsFile::default(),
        }
    }

    /// Save assignments to PVC.
    pub async fn save_assignments(&self, assignments: &AssignmentsFile) -> anyhow::Result<()> {
        let path = self.assignments_path();
        fs::write(&path, serde_json::to_string_pretty(assignments)?).await?;
        Ok(())
    }

    // --- Oneshot state ---

    /// Load oneshot completion state from PVC.
    pub async fn load_oneshot(&self) -> OneshotState {
        let path = self.oneshot_path();
        match fs::read_to_string(&path).await {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => OneshotState::default(),
        }
    }

    /// Save oneshot state to PVC.
    pub async fn save_oneshot(&self, state: &OneshotState) -> anyhow::Result<()> {
        let path = self.oneshot_path();
        fs::write(&path, serde_json::to_string_pretty(state)?).await?;
        Ok(())
    }
}

/// Substitute template variables in content.
pub fn substitute_variables(
    content: &str,
    hostname: &str,
    ip: &str,
    instance_id: &str,
    availability_zone: &str,
    domain_suffix: &str,
    template_name: &str,
) -> String {
    let short_hostname = hostname
        .strip_suffix(domain_suffix)
        .unwrap_or(hostname)
        .trim_end_matches('.');
    let encoded_hostname = url_encode_simple(hostname);

    content
        .replace("{{HOSTNAME}}", hostname)
        .replace("{{SHORT_HOSTNAME}}", short_hostname)
        .replace("{{IP}}", ip)
        .replace("{{INSTANCE_ID}}", instance_id)
        .replace("{{AVAILABILITY_ZONE}}", availability_zone)
        .replace("{{HOSTNAME_ENCODED}}", &encoded_hostname)
        .replace("{{DOMAIN_SUFFIX}}", domain_suffix)
        .replace("{{TEMPLATE_NAME}}", template_name)
}

/// Minimal URL-encoding for data URIs.
fn url_encode_simple(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => encoded.push_str("%20"),
            '%' => encoded.push_str("%25"),
            '\n' => encoded.push_str("%0A"),
            '\r' => encoded.push_str("%0D"),
            '#' => encoded.push_str("%23"),
            _ => encoded.push(c),
        }
    }
    encoded
}

/// Current UTC timestamp as ISO 8601 string.
fn now_utc() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Simple UTC format: YYYY-MM-DDTHH:MM:SSZ
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to Y-M-D (simplified Gregorian)
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_detection() {
        assert_eq!(
            TemplateFormat::from_filename("agent-runner.ign.json"),
            TemplateFormat::Ignition
        );
        assert_eq!(
            TemplateFormat::from_filename("base-server.ks"),
            TemplateFormat::Kickstart
        );
        assert_eq!(
            TemplateFormat::from_filename("base-server.yaml"),
            TemplateFormat::CloudConfig
        );
    }

    #[test]
    fn test_substitute_variables() {
        let content = r#"{"hostname": "{{HOSTNAME}}", "short": "{{SHORT_HOSTNAME}}", "ip": "{{IP}}"}"#;
        let result = substitute_variables(
            content,
            "server1.g10.lo",
            "192.168.10.10",
            "server1",
            "gt",
            ".g10.lo",
            "agent-runner",
        );
        assert!(result.contains("server1.g10.lo"));
        assert!(result.contains("\"short\": \"server1\""));
        assert!(result.contains("192.168.10.10"));
    }

    #[test]
    fn test_now_utc_format() {
        let ts = now_utc();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[tokio::test]
    async fn test_template_store_crud() {
        let dir = std::env::temp_dir().join(format!("cloudid-test-{}", std::process::id()));
        let store = TemplateStore::new(dir.to_str().unwrap());
        store.init().await.unwrap();

        // Create
        let req = TemplateCreateRequest {
            mode: TemplateMode::Oneshot,
            content: r#"{"ignition":{"version":"3.4.0"}}"#.to_string(),
        };
        let tpl = store.put("fcos", "test.ign.json", &req).await.unwrap();
        assert_eq!(tpl.image_type, "fcos");
        assert_eq!(tpl.format, TemplateFormat::Ignition);
        assert_eq!(tpl.mode, TemplateMode::Oneshot);

        // Read
        let loaded = store.get("fcos", "test.ign.json").await.unwrap().unwrap();
        assert_eq!(loaded.content, req.content);

        // List
        let all = store.list_all().await.unwrap();
        assert_eq!(all.len(), 1);
        let by_type = store.list_by_type("fcos").await.unwrap();
        assert_eq!(by_type.len(), 1);

        // Delete
        assert!(store.delete("fcos", "test.ign.json").await.unwrap());
        assert!(store.get("fcos", "test.ign.json").await.unwrap().is_none());

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_backup_restore() {
        let dir = std::env::temp_dir().join(format!("cloudid-test-br-{}", std::process::id()));
        let store = TemplateStore::new(dir.to_str().unwrap());
        store.init().await.unwrap();

        // Create some templates
        store
            .put(
                "fcos",
                "a.ign.json",
                &TemplateCreateRequest {
                    mode: TemplateMode::Oneshot,
                    content: "content-a".to_string(),
                },
            )
            .await
            .unwrap();
        store
            .put(
                "fedora",
                "b.ks",
                &TemplateCreateRequest {
                    mode: TemplateMode::Forever,
                    content: "content-b".to_string(),
                },
            )
            .await
            .unwrap();

        // Backup
        let bundle = store.backup().await.unwrap();
        assert_eq!(bundle.version, 1);
        assert_eq!(bundle.templates.len(), 2);

        // Restore to a new store
        let dir2 = std::env::temp_dir().join(format!("cloudid-test-br2-{}", std::process::id()));
        let store2 = TemplateStore::new(dir2.to_str().unwrap());
        store2.init().await.unwrap();

        let count = store2.restore(&bundle).await.unwrap();
        assert_eq!(count, 2);

        let all = store2.list_all().await.unwrap();
        assert_eq!(all.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[tokio::test]
    async fn test_assignments_persistence() {
        let dir = std::env::temp_dir().join(format!("cloudid-test-asgn-{}", std::process::id()));
        let store = TemplateStore::new(dir.to_str().unwrap());
        store.init().await.unwrap();

        let mut assignments = AssignmentsFile::default();
        assignments.assignments.insert(
            "server1".to_string(),
            Assignment {
                image_type: "fcos".to_string(),
                template: "agent-runner".to_string(),
            },
        );

        store.save_assignments(&assignments).await.unwrap();
        let loaded = store.load_assignments().await;
        assert_eq!(loaded.assignments.len(), 1);
        assert_eq!(loaded.assignments["server1"].template, "agent-runner");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_oneshot_persistence() {
        let dir = std::env::temp_dir().join(format!("cloudid-test-os-{}", std::process::id()));
        let store = TemplateStore::new(dir.to_str().unwrap());
        store.init().await.unwrap();

        let mut state = OneshotState::default();
        state
            .completed
            .insert("server1".to_string(), now_utc());

        store.save_oneshot(&state).await.unwrap();
        let loaded = store.load_oneshot().await;
        assert_eq!(loaded.completed.len(), 1);
        assert!(loaded.completed.contains_key("server1"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
