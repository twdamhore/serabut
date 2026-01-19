//! Template service for rendering MiniJinja templates.
//!
//! Provides template rendering with standard variables.

use base64::prelude::*;
use crate::error::{AppError, AppResult};
use minijinja::{context, Environment, Error, ErrorKind};
use std::collections::HashMap;
use std::path::Path;

/// Base64 decode filter for templates.
///
/// Usage: `{{ value | b64decode }}`
fn b64decode(value: String) -> Result<String, Error> {
    let decoded = BASE64_STANDARD
        .decode(&value)
        .map_err(|e| Error::new(ErrorKind::InvalidOperation, format!("base64 decode error: {}", e)))?;
    String::from_utf8(decoded)
        .map_err(|e| Error::new(ErrorKind::InvalidOperation, format!("utf8 decode error: {}", e)))
}

/// Base64 encode filter for templates.
///
/// Usage: `{{ value | b64encode }}`
fn b64encode(value: String) -> String {
    BASE64_STANDARD.encode(value.as_bytes())
}

/// Context variables for template rendering.
#[derive(Debug, Clone)]
pub struct TemplateContext {
    pub host: String,
    pub port: u16,
    pub mac: String,
    pub iso: Option<String>,
    pub iso_image: Option<String>,
    pub automation: Option<String>,
    pub hostname: Option<String>,
    pub machine_id: Option<String>,
    pub timezone: Option<String>,
    /// Base64-encoded SSH host keys.
    pub base64_ssh_host_key_ecdsa_public: Option<String>,
    pub base64_ssh_host_key_ecdsa_private: Option<String>,
    pub base64_ssh_host_key_ed25519_public: Option<String>,
    pub base64_ssh_host_key_ed25519_private: Option<String>,
    pub base64_ssh_host_key_rsa_public: Option<String>,
    pub base64_ssh_host_key_rsa_private: Option<String>,
    /// Additional variables from hardware.cfg.
    pub extra: HashMap<String, String>,
}

impl TemplateContext {
    /// Create a new template context with required fields.
    pub fn new(host: String, port: u16, mac: String) -> Self {
        Self {
            host,
            port,
            mac,
            iso: None,
            iso_image: None,
            automation: None,
            hostname: None,
            machine_id: None,
            timezone: None,
            base64_ssh_host_key_ecdsa_public: None,
            base64_ssh_host_key_ecdsa_private: None,
            base64_ssh_host_key_ed25519_public: None,
            base64_ssh_host_key_ed25519_private: None,
            base64_ssh_host_key_rsa_public: None,
            base64_ssh_host_key_rsa_private: None,
            extra: HashMap::new(),
        }
    }

    /// Set the ISO name.
    pub fn with_iso(mut self, iso: String) -> Self {
        self.iso = Some(iso);
        self
    }

    /// Set the ISO image filename.
    pub fn with_iso_image(mut self, iso_image: String) -> Self {
        self.iso_image = Some(iso_image);
        self
    }

    /// Set the automation name.
    pub fn with_automation(mut self, automation: String) -> Self {
        self.automation = Some(automation);
        self
    }

    /// Set the hostname.
    pub fn with_hostname(mut self, hostname: String) -> Self {
        self.hostname = Some(hostname);
        self
    }

    /// Set the machine ID.
    pub fn with_machine_id(mut self, machine_id: String) -> Self {
        self.machine_id = Some(machine_id);
        self
    }

    /// Set the timezone.
    pub fn with_timezone(mut self, timezone: String) -> Self {
        self.timezone = Some(timezone);
        self
    }

    /// Set base64-encoded SSH host key (ECDSA public).
    pub fn with_base64_ssh_host_key_ecdsa_public(mut self, key: String) -> Self {
        self.base64_ssh_host_key_ecdsa_public = Some(key);
        self
    }

    /// Set base64-encoded SSH host key (ECDSA private).
    pub fn with_base64_ssh_host_key_ecdsa_private(mut self, key: String) -> Self {
        self.base64_ssh_host_key_ecdsa_private = Some(key);
        self
    }

    /// Set base64-encoded SSH host key (Ed25519 public).
    pub fn with_base64_ssh_host_key_ed25519_public(mut self, key: String) -> Self {
        self.base64_ssh_host_key_ed25519_public = Some(key);
        self
    }

    /// Set base64-encoded SSH host key (Ed25519 private).
    pub fn with_base64_ssh_host_key_ed25519_private(mut self, key: String) -> Self {
        self.base64_ssh_host_key_ed25519_private = Some(key);
        self
    }

    /// Set base64-encoded SSH host key (RSA public).
    pub fn with_base64_ssh_host_key_rsa_public(mut self, key: String) -> Self {
        self.base64_ssh_host_key_rsa_public = Some(key);
        self
    }

    /// Set base64-encoded SSH host key (RSA private).
    pub fn with_base64_ssh_host_key_rsa_private(mut self, key: String) -> Self {
        self.base64_ssh_host_key_rsa_private = Some(key);
        self
    }

    /// Add extra variables.
    pub fn with_extra(mut self, extra: HashMap<String, String>) -> Self {
        self.extra = extra;
        self
    }
}

/// Service for rendering templates.
pub struct TemplateService;

impl TemplateService {
    /// Create a new template service.
    pub fn new() -> Self {
        Self
    }

    /// Render a template file with the given context.
    pub fn render_file(&self, template_path: &Path, ctx: &TemplateContext) -> AppResult<String> {
        let template_content =
            std::fs::read_to_string(template_path).map_err(|e| AppError::FileRead {
                path: template_path.to_path_buf(),
                source: e,
            })?;

        self.render_string(&template_content, template_path, ctx)
    }

    /// Render a template string with the given context.
    pub fn render_string(
        &self,
        template: &str,
        template_path: &Path,
        ctx: &TemplateContext,
    ) -> AppResult<String> {
        let mut env = Environment::new();
        env.add_filter("b64decode", b64decode);
        env.add_filter("b64encode", b64encode);
        let template_name = template_path.to_string_lossy();

        env.add_template(&template_name, template)
            .map_err(|e| AppError::TemplateRender {
                template: template_name.to_string(),
                source: e,
            })?;

        let tmpl = env.get_template(&template_name).map_err(|e| AppError::TemplateRender {
            template: template_name.to_string(),
            source: e,
        })?;

        // Build context with all variables
        let rendered = tmpl
            .render(context! {
                host => ctx.host,
                port => ctx.port,
                mac => ctx.mac,
                iso => ctx.iso,
                iso_image => ctx.iso_image,
                automation => ctx.automation,
                hostname => ctx.hostname,
                machine_id => ctx.machine_id,
                timezone => ctx.timezone,
                base64_ssh_host_key_ecdsa_public => ctx.base64_ssh_host_key_ecdsa_public,
                base64_ssh_host_key_ecdsa_private => ctx.base64_ssh_host_key_ecdsa_private,
                base64_ssh_host_key_ed25519_public => ctx.base64_ssh_host_key_ed25519_public,
                base64_ssh_host_key_ed25519_private => ctx.base64_ssh_host_key_ed25519_private,
                base64_ssh_host_key_rsa_public => ctx.base64_ssh_host_key_rsa_public,
                base64_ssh_host_key_rsa_private => ctx.base64_ssh_host_key_rsa_private,
                ..ctx.extra.clone()
            })
            .map_err(|e| AppError::TemplateRender {
                template: template_name.to_string(),
                source: e,
            })?;

        Ok(rendered)
    }
}

impl Default for TemplateService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_render_simple_template() {
        let service = TemplateService::new();
        let template = "host={{ host }}, port={{ port }}, mac={{ mac }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "host=192.168.1.1, port=4123, mac=aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn test_render_with_optional_fields() {
        let service = TemplateService::new();
        let template = "iso={{ iso }}, automation={{ automation }}, hostname={{ hostname }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string())
            .with_iso("ubuntu-24.04".to_string())
            .with_automation("docker".to_string())
            .with_hostname("server01".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "iso=ubuntu-24.04, automation=docker, hostname=server01");
    }

    #[test]
    fn test_render_with_machine_id() {
        let service = TemplateService::new();
        let template = "hostname={{ hostname }}, machine_id={{ machine_id }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string())
            .with_hostname("server01".to_string())
            .with_machine_id("srv-001".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "hostname=server01, machine_id=srv-001");
    }

    #[test]
    fn test_render_with_extra_vars() {
        let service = TemplateService::new();
        let template = "role={{ role }}";
        let mut extra = HashMap::new();
        extra.insert("role".to_string(), "webserver".to_string());

        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string())
            .with_extra(extra);

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "role=webserver");
    }

    #[test]
    fn test_render_file() {
        let dir = setup_test_dir();
        let template_path = dir.path().join("test.j2");
        std::fs::write(&template_path, "Hello {{ hostname }}!").unwrap();

        let service = TemplateService::new();
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string())
            .with_hostname("server01".to_string());

        let result = service.render_file(&template_path, &ctx).unwrap();

        assert_eq!(result, "Hello server01!");
    }

    #[test]
    fn test_render_invalid_template() {
        let service = TemplateService::new();
        let template = "{{ invalid syntax";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string());

        let result = service.render_string(template, Path::new("test.j2"), &ctx);

        assert!(matches!(result, Err(AppError::TemplateRender { .. })));
    }

    #[test]
    fn test_boot_ipxe_template() {
        let service = TemplateService::new();
        let template = r#"#!ipxe
imgfetch http://{{ host }}:{{ port }}/done/{{ mac }} ||
kernel http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/vmlinuz
initrd http://{{ host }}:{{ port }}/iso/{{ iso }}/casper/initrd
boot"#;

        let ctx = TemplateContext::new("192.168.1.100".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string())
            .with_iso("ubuntu-24.04".to_string());

        let result = service
            .render_string(template, Path::new("boot.ipxe.j2"), &ctx)
            .unwrap();

        assert!(result.contains("http://192.168.1.100:4123/done/aa-bb-cc-dd-ee-ff"));
        assert!(result.contains("http://192.168.1.100:4123/iso/ubuntu-24.04/casper/vmlinuz"));
    }

    #[test]
    fn test_b64decode_filter() {
        let service = TemplateService::new();
        // "Hello, World!" in base64
        let template = "{{ 'SGVsbG8sIFdvcmxkIQ==' | b64decode }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_b64encode_filter() {
        let service = TemplateService::new();
        let template = "{{ 'Hello, World!' | b64encode }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_b64decode_with_variable() {
        let service = TemplateService::new();
        let template = "{{ base64_ssh_host_key_ed25519_public | b64decode }}";
        // "ssh-ed25519 AAAAC3..." in base64
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string())
            .with_base64_ssh_host_key_ed25519_public("c3NoLWVkMjU1MTkgQUFBQUMz".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "ssh-ed25519 AAAAC3");
    }

    #[test]
    fn test_b64decode_roundtrip() {
        let service = TemplateService::new();
        let template = "{{ 'test string' | b64encode | b64decode }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string());

        let result = service
            .render_string(template, Path::new("test.j2"), &ctx)
            .unwrap();

        assert_eq!(result, "test string");
    }

    #[test]
    fn test_b64decode_invalid_base64() {
        let service = TemplateService::new();
        let template = "{{ 'not-valid-base64!!!' | b64decode }}";
        let ctx = TemplateContext::new("192.168.1.1".to_string(), 4123, "aa-bb-cc-dd-ee-ff".to_string());

        let result = service.render_string(template, Path::new("test.j2"), &ctx);

        assert!(matches!(result, Err(AppError::TemplateRender { .. })));
    }
}
